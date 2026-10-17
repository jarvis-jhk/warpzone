//! The launcher's UI and state.

use crate::art;
use crate::jobs::{Apply, Jobs};
use crate::lobby::{self, Lobby};
use crate::settings::Settings;
use crate::steam;
use crate::system;
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, TextureHandle, TextureOptions, Vec2};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use warpzone_core::api::Api;
use warpzone_core::gamedata::{self, Check, Found};
use warpzone_core::identity::Identity;
use warpzone_core::install::{self, Installed};
use warpzone_protocol::api::*;
use warpzone_protocol::descriptor::{DataRequirement, GameDescriptor};

pub const BG: Color32 = Color32::from_rgb(11, 11, 20);
pub const PANEL: Color32 = Color32::from_rgb(23, 23, 40);
pub const LINE: Color32 = Color32::from_rgb(42, 42, 68);
pub const ACCENT: Color32 = Color32::from_rgb(124, 92, 255);
pub const ACCENT2: Color32 = Color32::from_rgb(34, 211, 238);
pub const MUTED: Color32 = Color32::from_rgb(154, 154, 184);
pub const OK: Color32 = Color32::from_rgb(52, 211, 153);
pub const WARN: Color32 = Color32::from_rgb(251, 191, 36);

pub struct DataScan {
    pub req: DataRequirement,
    pub present: bool,
    pub found: Vec<(Found, Check)>,
    /// What the player picked (or the best automatic choice).
    pub chosen: Option<PathBuf>,
}

impl DataScan {
    /// Ready if already in place, a usable source was found, or the launcher
    /// can download it by itself (freeware with `fetch`).
    pub fn ready(&self) -> bool {
        self.present || self.chosen.is_some() || gamedata::fetchable(&self.req)
    }

    /// Nothing usable here yet, but the launcher can download it.
    pub fn needs_fetch(&self) -> bool {
        !self.present && self.chosen.is_none() && gamedata::fetchable(&self.req)
    }
}

/// Look for every requirement and pick the best automatic choice for each.
fn scan_reqs(reqs: Vec<DataRequirement>, inst_dir: Option<&Path>, extra: &[PathBuf]) -> Vec<DataScan> {
    reqs.into_iter()
        .map(|req| {
            let present = gamedata::present(&req, inst_dir);
            let found: Vec<(Found, Check)> = gamedata::find(&req, extra)
                .into_iter()
                .map(|f| {
                    let c = gamedata::check(&req, &f.path);
                    (f, c)
                })
                .collect();
            // Best automatic choice: verified first, then unchecked/unknown; never unsupported.
            let chosen = found
                .iter()
                .find(|(_, c)| matches!(c, Check::Verified(_)))
                .or_else(|| found.iter().find(|(_, c)| matches!(c, Check::Unchecked)))
                .map(|(f, _)| f.path.clone());
            DataScan { req, present, found, chosen }
        })
        .collect()
}

/// Progress of a data download, shown as the game's busy line (throttled).
fn progress_sender(jobs: &Jobs, gid: &str) -> impl FnMut(&str, u64, u64) + use<> {
    let (jobs, gid) = (jobs.clone(), gid.to_string());
    let mut last: Option<Instant> = None;
    move |label: &str, done: u64, total: u64| {
        if last.is_some_and(|l| l.elapsed() < Duration::from_millis(100)) {
            return;
        }
        last = Some(Instant::now());
        let (gid, busy) = (gid.clone(), format!("Downloading {label}…"));
        jobs.send(Box::new(move |app: &mut App| {
            if let Some(g) = app.game(&gid) {
                g.busy = Some(busy);
                g.progress = Some((done, total));
            }
        }));
    }
}

/// Download what `scans` can fetch and has nowhere else; returns the errors
/// ("label: reason"). `progress` gets (label, bytes so far, total).
fn fetch_missing(api: &Api, scans: &[DataScan], progress: &mut dyn FnMut(&str, u64, u64)) -> Vec<String> {
    let mut errors = Vec::new();
    for s in scans.iter().filter(|s| s.needs_fetch()) {
        let total = s.req.fetch.first().map(|f| f.size).unwrap_or(0);
        if let Err(e) = gamedata::fetch(api, &s.req, &mut |done| progress(&s.req.label, done, total)) {
            errors.push(format!("Could not download {}: {e}", s.req.label));
        }
    }
    errors
}

pub struct GameState {
    pub desc: GameDescriptor,
    pub release: Option<ReleaseInfo>,
    pub installed: Option<Installed>,
    pub changelog: Vec<ChangelogEntry>,
    pub data: Option<Vec<DataScan>>,
    pub busy: Option<String>,
    pub progress: Option<(u64, u64)>,
    pub error: Option<String>,
    pub tile: Option<TextureHandle>,
    pub running: bool,
}

impl GameState {
    fn new(desc: GameDescriptor) -> GameState {
        let installed = install::installed(&desc.id);
        GameState { desc, release: None, installed, changelog: Vec::new(), data: None, busy: None, progress: None, error: None, tile: None, running: false }
    }

    pub fn platform_asset(&self) -> bool {
        self.release.as_ref().is_some_and(|r| install::asset_for(r, install::platform()).is_some())
    }

    pub fn update_available(&self) -> bool {
        match (&self.installed, &self.release) {
            (Some(i), Some(r)) => i.version != r.version && install::asset_for(r, install::platform()).is_some(),
            _ => false,
        }
    }

    pub fn data_ready(&self) -> bool {
        self.data.as_ref().is_none_or(|d| d.iter().all(|s| s.ready() || s.req.optional))
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Tab {
    Library,
    Rooms,
    Ladder,
    Mods,
    Moderation,
    Settings,
}

pub enum Primary {
    Busy(String),
    Running,
    Install,
    Update,
    Play,
    FindData,
    Unavailable(String),
}

pub struct Queue {
    pub game: String,
    pub queue: String,
    pub client: warpzone_core::client::Client,
    pub since: Instant,
    pub status: String,
}

pub struct ShotPlan {
    pub dir: PathBuf,
    pub steps: Vec<(String, Tab, Option<String>, bool)>,
    pub idx: usize,
    pub at: Instant,
    pub waiting: bool,
}

pub struct App {
    pub jobs: Jobs,
    rx: Receiver<Apply>,
    pub api: Arc<Api>,
    pub settings: Settings,
    pub games: Vec<GameState>,
    pub tab: Tab,
    pub page: Option<String>,
    pub account: Option<Account>,
    pub roles: Vec<String>,
    pub online: Option<bool>,
    pub toasts: Vec<(String, Instant)>,
    pub rooms: Vec<RoomInfo>,
    pub rooms_game: Option<String>,
    pub code_input: String,
    pub join_status: Option<String>,
    pub standings: Option<StandingsResponse>,
    pub ladder_pick: Option<(String, String)>,
    pub queue: Option<Queue>,
    pub mods_game: Option<String>,
    pub mods: Vec<ModSummary>,
    pub mods_query: String,
    pub mod_detail: Option<ModDetail>,
    pub moderation: Option<ModerationQueue>,
    pub reject_reason: String,
    pub username_input: String,
    pub link_input: String,
    pub link_code: Option<String>,
    pub recovery: Option<String>,
    pub show_hidden: bool,
    pub first_run: bool,
    pub lobby: Option<Lobby>,
    pub host_entry: Entry,
    pub host_visibility: Visibility,
    pub data_folder_input: String,
    pub pad: crate::pad::Pad,
    pub shots: Option<ShotPlan>,
    pub args_rx: Option<Receiver<Vec<String>>>,
    pub last_bg: Instant,
    pub hidden: bool,
    pub play_after: Option<String>,
    pub exit_after_launch: bool,
    pub ctx: Option<egui::Context>,
    pub self_updated: Option<String>,
    pub confirm_delete: bool,
}

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("display".into(), Arc::new(egui::FontData::from_static(art::DISPLAY_FONT)));
    fonts.families.insert(egui::FontFamily::Name("display".into()), vec!["display".into(), "Ubuntu-Light".into()]);
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(18, 18, 31);
    v.faint_bg_color = Color32::from_rgb(20, 20, 34);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 52);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 30, 52);
    v.widgets.hovered.bg_fill = Color32::from_rgb(45, 40, 90);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(45, 40, 90);
    v.widgets.active.bg_fill = ACCENT;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT2;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::new(26.0, egui::FontFamily::Name("display".into())));
    });
}

pub fn display(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("display".into()))
}

fn mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}

impl App {
    pub fn new(cc: &eframe::CreationContext, args_rx: Option<Receiver<Vec<String>>>) -> App {
        style(&cc.egui_ctx);
        let settings = Settings::load();
        let identity = Arc::new(Identity::shared().unwrap_or_else(|_| Identity::generate()));
        let api = Arc::new(Api::new(&settings.server(), identity));
        let (jobs, rx) = Jobs::new(cc.egui_ctx.clone());
        let first_run = settings.background.is_none();
        let mut app = App {
            jobs,
            rx,
            api,
            settings,
            games: Vec::new(),
            tab: Tab::Library,
            page: None,
            account: None,
            roles: Vec::new(),
            online: None,
            toasts: Vec::new(),
            rooms: Vec::new(),
            rooms_game: None,
            code_input: String::new(),
            join_status: None,
            standings: None,
            ladder_pick: None,
            queue: None,
            mods_game: None,
            mods: Vec::new(),
            mods_query: String::new(),
            mod_detail: None,
            moderation: None,
            reject_reason: REJECT_REASONS[0].to_string(),
            username_input: String::new(),
            link_input: String::new(),
            link_code: None,
            recovery: None,
            show_hidden: false,
            first_run,
            lobby: None,
            host_entry: Entry::Open,
            host_visibility: Visibility::Public,
            data_folder_input: String::new(),
            pad: crate::pad::Pad::new(),
            shots: None,
            args_rx,
            last_bg: Instant::now(),
            hidden: false,
            play_after: None,
            exit_after_launch: false,
            ctx: Some(cc.egui_ctx.clone()),
            self_updated: None,
            confirm_delete: false,
        };
        app.login();
        app.refresh_catalog();
        app.check_self_update();
        app
    }

    /// Keep the launcher itself current (verified against GitHub like games).
    pub fn check_self_update(&mut self) {
        if !self.settings.auto_update || std::env::var_os("WARPZONE_NO_SELF_UPDATE").is_some() {
            return;
        }
        let api = self.api.clone();
        self.jobs.run(move || {
            let current = env!("CARGO_PKG_VERSION");
            let r = api.updates("launcher", current).and_then(|u| match (u.update_available, u.latest) {
                (true, Some(rel)) => match std::env::var_os("APPIMAGE").filter(|a| !a.is_empty()) {
                    // Running as an AppImage: replace the AppImage file itself.
                    Some(path) => install::self_update_appimage(&api, "jarvis-jhk/warpzone", &rel, std::path::Path::new(&path)).map(Some),
                    None => {
                        let bin = if cfg!(windows) { "warpzone.exe" } else { "warpzone" };
                        install::self_update(&api, "jarvis-jhk/warpzone", &rel, bin).map(Some)
                    }
                },
                _ => Ok(None),
            });
            Box::new(move |app: &mut App| match r {
                Ok(Some(v)) => {
                    app.self_updated = Some(v.clone());
                    app.toast(format!("Warpzone {v} is installed — it starts next time"));
                }
                Ok(None) => {}
                Err(e) => log::warn!("launcher self-update: {e}"),
            })
        });
    }

    pub fn toast(&mut self, m: impl Into<String>) {
        self.toasts.push((m.into(), Instant::now()));
    }

    pub fn game(&mut self, id: &str) -> Option<&mut GameState> {
        self.games.iter_mut().find(|g| g.desc.id == id)
    }

    fn login(&mut self) {
        let api = self.api.clone();
        self.jobs.run(move || {
            let r = api.login();
            Box::new(move |app: &mut App| match r {
                Ok(l) => {
                    app.online = Some(true);
                    app.account = l.account;
                    app.roles = l.roles;
                }
                Err(e) => {
                    app.online = Some(false);
                    log::warn!("login: {e}");
                }
            })
        });
    }

    pub fn refresh_catalog(&mut self) {
        let api = self.api.clone();
        self.jobs.run(move || {
            let games = api.games();
            let mut extra = Vec::new();
            if let Ok(g) = &games {
                for d in g {
                    extra.push((d.id.clone(), api.release(&d.id).ok(), api.changelog(&d.id).unwrap_or_default()));
                }
            }
            Box::new(move |app: &mut App| match games {
                Ok(list) => {
                    let mut next = Vec::new();
                    for d in list {
                        let mut g = app.games.iter().position(|x| x.desc.id == d.id).map(|i| app.games.remove(i)).unwrap_or_else(|| GameState::new(d.clone()));
                        g.desc = d;
                        if let Some((_, r, c)) = extra.iter().find(|(id, _, _)| *id == g.desc.id) {
                            g.release = r.clone();
                            g.changelog = c.clone();
                        }
                        next.push(g);
                    }
                    app.games = next;
                    app.online = Some(true);
                    let ids: Vec<String> = app.games.iter().map(|g| g.desc.id.clone()).collect();
                    for id in ids {
                        app.render_tile(&id);
                        app.scan_data(&id);
                    }
                    app.background_updates();
                }
                Err(e) => {
                    app.online = Some(false);
                    app.toast(format!("Offline: {e}"));
                }
            })
        });
    }

    fn render_tile(&mut self, id: &str) {
        let Some(g) = self.games.iter().find(|g| g.desc.id == id) else { return };
        if g.tile.is_some() {
            return;
        }
        let (gid, name, accent, motif) =
            (g.desc.id.clone(), g.desc.name.clone(), g.desc.accent.clone(), g.desc.motif.clone().unwrap_or_else(|| "waves".into()));
        self.jobs.run(move || {
            let (w, h, px) = art::render(&art::Spec { id: &gid, name: &name, accent: &accent, motif: &motif }, art::Kind::Tile);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            Box::new(move |app: &mut App| {
                let ctx = app.jobs_ctx();
                let tex = ctx.load_texture(format!("tile-{gid}"), img, TextureOptions::LINEAR);
                if let Some(g) = app.game(&gid) {
                    g.tile = Some(tex);
                }
            })
        });
    }

    fn jobs_ctx(&self) -> egui::Context {
        self.ctx.clone().unwrap()
    }

    pub fn scan_data(&mut self, id: &str) {
        let Some(g) = self.games.iter().find(|g| g.desc.id == id) else { return };
        if g.desc.data.is_empty() {
            return;
        }
        let reqs = g.desc.data.clone();
        let inst_dir = g.installed.as_ref().map(|i| i.dir.clone());
        let extra: Vec<PathBuf> = self.settings.data_folders.iter().map(PathBuf::from).collect();
        let gid = id.to_string();
        self.jobs.run(move || {
            let scans = scan_reqs(reqs, inst_dir.as_deref(), &extra);
            Box::new(move |app: &mut App| {
                if let Some(g) = app.game(&gid) {
                    g.data = Some(scans);
                }
            })
        });
    }

    /// Download the freeware data the launcher can fetch itself, then search
    /// again (and start the game when `play` is set).
    pub fn fetch_data(&mut self, id: &str, play: bool) {
        let Some(g) = self.games.iter_mut().find(|g| g.desc.id == id) else { return };
        let Some(reqs) = g.data.as_ref().map(|d| d.iter().map(|s| s.req.clone()).collect::<Vec<_>>()) else { return };
        g.busy = Some("Downloading the game files…".into());
        g.error = None;
        let inst_dir = g.installed.as_ref().map(|i| i.dir.clone());
        let extra: Vec<PathBuf> = self.settings.data_folders.iter().map(PathBuf::from).collect();
        let api = self.api.clone();
        let jobs = self.jobs.clone();
        let gid = id.to_string();
        self.jobs.run(move || {
            let before = scan_reqs(reqs.clone(), inst_dir.as_deref(), &extra);
            let errors = fetch_missing(&api, &before, &mut progress_sender(&jobs, &gid));
            let scans = scan_reqs(reqs, inst_dir.as_deref(), &extra);
            Box::new(move |app: &mut App| {
                let ready = scans.iter().all(|s| s.present || s.chosen.is_some() || s.req.optional);
                if let Some(g) = app.game(&gid) {
                    g.busy = None;
                    g.progress = None;
                    g.data = Some(scans);
                    if !errors.is_empty() {
                        g.error = Some(errors.join("\n"));
                    }
                }
                if play && ready {
                    app.play(&gid);
                }
            })
        });
    }

    /// Check installed games for updates and install them quietly.
    pub fn background_updates(&mut self) {
        if !self.settings.auto_update {
            return;
        }
        let ids: Vec<String> = self.games.iter().filter(|g| g.update_available() && g.busy.is_none() && !g.running).map(|g| g.desc.id.clone()).collect();
        for id in ids {
            self.install(&id, true);
        }
    }

    pub fn primary(&self, g: &GameState) -> Primary {
        if let Some(b) = &g.busy {
            return Primary::Busy(b.clone());
        }
        if g.running {
            return Primary::Running;
        }
        if g.installed.is_some() {
            return if g.update_available() { Primary::Update } else { Primary::Play };
        }
        if g.desc.install.is_none() {
            return Primary::Unavailable("No downloads".into());
        }
        if g.release.is_none() {
            return Primary::Unavailable("No release yet".into());
        }
        if !g.platform_asset() {
            return Primary::Unavailable(format!("Not available for {}", install::platform()));
        }
        if !g.data_ready() {
            return Primary::FindData;
        }
        Primary::Install
    }

    /// One click: download, verify, unpack, hand the game data over.
    pub fn install(&mut self, id: &str, quiet: bool) {
        let Some(g) = self.games.iter_mut().find(|g| g.desc.id == id) else { return };
        let Some(release) = g.release.clone() else { return };
        g.busy = Some(if g.installed.is_some() { "Updating…".into() } else { "Installing…".into() });
        g.progress = Some((0, install::asset_for(&release, install::platform()).map(|a| a.1.size).unwrap_or(0)));
        g.error = None;
        let desc = g.desc.clone();
        let mut choices: Vec<(DataRequirement, Option<PathBuf>, bool)> =
            g.data.as_ref().map(|d| d.iter().map(|s| (s.req.clone(), s.chosen.clone(), s.present)).collect()).unwrap_or_default();
        let needs_fetch = g.data.as_ref().is_some_and(|d| d.iter().any(DataScan::needs_fetch));
        let busy = g.busy.clone();
        let inst_dir = g.installed.as_ref().map(|i| i.dir.clone());
        let extra: Vec<PathBuf> = self.settings.data_folders.iter().map(PathBuf::from).collect();
        let api = self.api.clone();
        let jobs = self.jobs.clone();
        let gid = id.to_string();
        self.jobs.run(move || {
            // Freeware data the launcher downloads itself comes first: it is small,
            // and the game is of no use without it.
            let mut fetch_errors = Vec::new();
            let mut rescanned = None;
            if needs_fetch {
                let reqs: Vec<DataRequirement> = choices.iter().map(|c| c.0.clone()).collect();
                let before = scan_reqs(reqs.clone(), inst_dir.as_deref(), &extra);
                fetch_errors = fetch_missing(&api, &before, &mut progress_sender(&jobs, &gid));
                let scans = scan_reqs(reqs, inst_dir.as_deref(), &extra);
                for (c, s) in choices.iter_mut().zip(&scans) {
                    if c.1.is_none() {
                        c.1 = s.chosen.clone();
                    }
                }
                rescanned = Some(scans);
                let (gid, busy) = (gid.clone(), busy.clone());
                jobs.send(Box::new(move |app: &mut App| {
                    if let Some(g) = app.game(&gid) {
                        g.busy = busy;
                        g.progress = None;
                    }
                }));
            }
            let total = install::asset_for(&release, install::platform()).map(|a| a.1.size).unwrap_or(0);
            let mut last = Instant::now();
            let gid2 = gid.clone();
            let r = install::install(&api, &desc, &release, &mut |done| {
                if last.elapsed() > Duration::from_millis(100) {
                    last = Instant::now();
                    let gid3 = gid2.clone();
                    jobs.send(Box::new(move |app: &mut App| {
                        if let Some(g) = app.game(&gid3) {
                            g.progress = Some((done, total));
                        }
                    }));
                }
            });
            // Hand over found data (copy_to) right after installing.
            let mut handed = Vec::new();
            if let Ok(inst) = &r {
                for (req, chosen, present) in &choices {
                    if *present || req.copy_to.is_none() {
                        continue;
                    }
                    if let Some(p) = chosen {
                        handed.push((req.label.clone(), gamedata::hand_over(req, p, Some(&inst.dir)).map(|_| ()).map_err(|e| e.to_string())));
                    }
                }
            }
            Box::new(move |app: &mut App| {
                let play_after = app.play_after.as_deref() == Some(gid.as_str());
                let mut toast = None;
                if let Some(g) = app.game(&gid) {
                    g.busy = None;
                    g.progress = None;
                    match r {
                        Ok(i) => {
                            toast = Some(format!("{} {} is ready", g.desc.name, i.version));
                            g.installed = Some(i);
                        }
                        Err(e) => g.error = Some(e.to_string()),
                    }
                    if !fetch_errors.is_empty() {
                        let e = fetch_errors.join("\n");
                        g.error = Some(g.error.take().map(|x| format!("{x}\n{e}")).unwrap_or(e));
                    }
                    if rescanned.is_some() {
                        g.data = rescanned;
                    }
                }
                for (label, res) in handed {
                    if let Err(e) = res {
                        app.toast(format!("Could not copy {label}: {e}"));
                    }
                }
                if let Some(t) = toast {
                    if !quiet {
                        app.toast(t);
                    } else if app.hidden {
                        system::notify("Game updated", &t);
                    }
                }
                app.scan_data(&gid);
                if play_after {
                    app.play_after = None;
                    app.play(&gid);
                }
            })
        });
    }

    /// Start a game (updating it first when an update is waiting).
    pub fn play(&mut self, id: &str) {
        let Some(g) = self.games.iter().find(|g| g.desc.id == id) else { return };
        if g.update_available() && self.settings.auto_update && g.busy.is_none() {
            self.play_after = Some(id.to_string());
            self.install(id, false);
            return;
        }
        if g.busy.is_none() && g.data.as_ref().is_some_and(|d| d.iter().any(|s| s.needs_fetch() && !s.req.optional)) {
            self.fetch_data(id, true);
            return;
        }
        let mut args = Vec::new();
        if let Some(d) = &g.data {
            for s in d {
                if !s.present
                    && !s.req.import_args.is_empty()
                    && let Some(p) = &s.chosen
                    && let Ok(Some(a)) = gamedata::hand_over(&s.req, p, g.installed.as_ref().map(|i| i.dir.as_path()))
                {
                    args.extend(a);
                }
            }
        }
        self.launch_with(id, args);
    }

    pub fn launch_with(&mut self, id: &str, args: Vec<String>) {
        // The game always learns where the launcher found its files.
        let env: Vec<(String, String)> = self
            .games
            .iter()
            .find(|g| g.desc.id == id)
            .and_then(|g| g.data.as_ref())
            .map(|d| d.iter().filter_map(|s| s.chosen.as_ref().map(|p| (install::data_env_name(&s.req.id), p.display().to_string()))).collect())
            .unwrap_or_default();
        self.launch_full(id, args, env);
    }

    /// Start a game and follow it: the launcher steps aside while it runs,
    /// comes back when it ends, and explains a start that failed.
    pub fn launch_full(&mut self, id: &str, args: Vec<String>, env: Vec<(String, String)>) {
        let ctx = self.jobs_ctx();
        let Some(g) = self.games.iter_mut().find(|g| g.desc.id == id) else { return };
        let Some(inst) = g.installed.clone() else { return };
        g.error = None;
        match install::launch_env(&g.desc, &inst, &args, &env) {
            Ok(mut child) => {
                g.running = true;
                let name = g.desc.name.clone();
                let seen = inst.version.clone();
                let gid = id.to_string();
                self.settings.seen_versions.insert(gid.clone(), seen);
                self.settings.save();
                if self.exit_after_launch {
                    std::process::exit(0);
                }
                self.toast(format!("Starting {name}…"));
                // Step aside so the game's window (and its first-start dialogs) is in front.
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                let started = Instant::now();
                self.jobs.run(move || {
                    let status = child.wait();
                    let ran = started.elapsed();
                    Box::new(move |app: &mut App| {
                        let ctx = app.jobs_ctx();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        let failed = match &status {
                            Ok(s) if !s.success() => Some(s.code().map(|c| format!("exit code {c}")).unwrap_or_else(|| "it was killed".into())),
                            Err(e) => Some(e.to_string()),
                            _ => None,
                        };
                        if let Some(g) = app.game(&gid) {
                            g.running = false;
                            if let Some(why) = failed.filter(|_| ran < Duration::from_secs(30)) {
                                let tail = install::log_tail(&gid, 6);
                                g.error = Some(format!(
                                    "{name} stopped right after starting ({why}).{}",
                                    if tail.is_empty() { String::new() } else { format!("\n{tail}") }
                                ));
                            }
                        }
                        app.scan_data(&gid);
                    })
                });
            }
            Err(e) => {
                g.error = Some(format!("Could not start {}: {e}", g.desc.name));
            }
        }
    }

    /// Join `room` of `game`: install if needed, then hand over to the game
    /// (tier ≥ 1) or open the launcher lobby (tier 0, staged rooms).
    pub fn join(&mut self, game: &str, room: &str, code: Option<String>, secret: Option<String>) {
        let Some(g) = self.games.iter().find(|g| g.desc.id == game) else {
            self.toast("That game is not in the catalog");
            return;
        };
        if g.installed.is_none() {
            self.page = Some(game.to_string());
            self.tab = Tab::Library;
            self.toast(format!("Install {} first, then join again", g.desc.name));
            return;
        }
        if g.desc.tier == 0 {
            if g.desc.tier0.is_none() {
                self.toast(format!("{} cannot join online rooms yet", g.desc.name));
                return;
            }
            let mut cfg = warpzone_core::client::Config::new(game, &g.installed.as_ref().unwrap().version);
            cfg.server = self.settings.server();
            self.lobby =
                Some(Lobby::start(cfg, g.desc.clone(), g.installed.clone().unwrap(), lobby::Mode::Join { room: room.into(), code, secret }, self.jobs_ctx()));
            return;
        }
        // Tier ≥ 1: join with our shared identity, then the game joins as the same account.
        self.join_status = Some("Joining…".into());
        let api = self.api.clone();
        let (gid, rid) = (game.to_string(), room.to_string());
        self.jobs.run(move || {
            let req = JoinRequest { code, invite: secret, candidates: vec![] };
            let mut result = api.join_room(&rid, &req).map(|r| matches!(r, JoinResponse::Joined(_)));
            if let Ok(false) = result {
                // Knocking: wait for the host (up to five minutes).
                let t = Instant::now();
                let mut after = 0;
                result = Err(warpzone_core::Error::Other("the host did not answer".into()));
                while t.elapsed() < Duration::from_secs(300) {
                    match api.events(after, 25) {
                        Ok(r) => {
                            after = r.next;
                            if let Some(ok) = r.events.iter().find_map(|e| match &e.event {
                                ServerEvent::KnockResult { room, accepted, .. } if *room == rid => Some(*accepted),
                                _ => None,
                            }) {
                                result = if ok { Ok(true) } else { Err(warpzone_core::Error::Other("the host declined".into())) };
                                break;
                            }
                        }
                        Err(_) => std::thread::sleep(Duration::from_secs(2)),
                    }
                }
            }
            if result.is_ok() {
                let _ = api.leave_room(&rid);
            }
            Box::new(move |app: &mut App| {
                app.join_status = None;
                match result {
                    Ok(_) => {
                        let args = app
                            .games
                            .iter()
                            .find(|g| g.desc.id == gid)
                            .and_then(|g| g.desc.launch.as_ref())
                            .map(|l| l.join.iter().map(|a| a.replace("{room}", &rid)).collect())
                            .unwrap_or_default();
                        app.launch_with(&gid, args);
                    }
                    Err(e) => app.toast(format!("Could not join: {e}")),
                }
            })
        });
    }

    pub fn host(&mut self, game: &str) {
        let Some(g) = self.games.iter().find(|g| g.desc.id == game) else { return };
        let Some(inst) = g.installed.clone() else { return };
        if g.desc.tier >= 2 {
            let args = g.desc.launch.as_ref().map(|l| l.host.clone()).unwrap_or_default();
            self.launch_with(game, args);
        } else if g.desc.tier0.is_some() {
            let mut cfg = warpzone_core::client::Config::new(game, &inst.version);
            cfg.server = self.settings.server();
            let max = g.desc.rooms.as_ref().map(|r| r.max_players).unwrap_or(8);
            self.lobby = Some(Lobby::start(
                cfg,
                g.desc.clone(),
                inst,
                lobby::Mode::Host { visibility: self.host_visibility, entry: self.host_entry, audience: Audience::default(), max_players: max },
                self.jobs_ctx(),
            ));
        }
    }

    pub fn add_to_steam(&mut self, game: Option<String>) {
        let (name, id, accent, motif, launch, urls) = match &game {
            Some(gid) => {
                let Some(g) = self.games.iter().find(|g| &g.desc.id == gid) else { return };
                let s = g.desc.steam.clone().unwrap_or_default();
                (
                    s.name.clone().unwrap_or(g.desc.name.clone()),
                    gid.clone(),
                    g.desc.accent.clone(),
                    g.desc.motif.clone().unwrap_or("waves".into()),
                    format!("--play {gid}"),
                    s,
                )
            }
            None => ("Warpzone".to_string(), "warpzone".to_string(), "#7c5cff".to_string(), "waves".to_string(), String::new(), Default::default()),
        };
        let api = self.api.clone();
        self.jobs.run(move || {
            let r = (|| -> Result<usize, String> {
                if steam::steam_running() {
                    steam::close_steam();
                }
                let mut art_set = Vec::new();
                for (suffix, kind, url) in [
                    ("p", art::Kind::Capsule, urls.capsule.clone()),
                    ("", art::Kind::Tile, urls.wide.clone()),
                    ("_hero", art::Kind::Hero, urls.hero.clone()),
                    ("_logo", art::Kind::Logo, urls.logo.clone()),
                    ("_icon", art::Kind::Icon, urls.icon.clone()),
                ] {
                    let provided = url.and_then(|u| api.fetch(&u, 16 * 1024 * 1024).ok()).and_then(|b| image::load_from_memory(&b).ok());
                    let png = match provided {
                        Some(img) => {
                            let mut out = std::io::Cursor::new(Vec::new());
                            img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                            out.into_inner()
                        }
                        None => {
                            let (w, h, px) = art::render(&art::Spec { id: &id, name: &name, accent: &accent, motif: &motif }, kind);
                            let mut out = std::io::Cursor::new(Vec::new());
                            image::write_buffer_with_format(&mut out, &px, w, h, image::ColorType::Rgba8, image::ImageFormat::Png)
                                .map_err(|e| e.to_string())?;
                            out.into_inner()
                        }
                    };
                    art_set.push((suffix, png));
                }
                let exe = install::launcher_path().map_err(|e| e.to_string())?;
                let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                steam::add(&steam::Shortcut { name: &name, exe: &exe, start_dir: &dir, launch_options: &launch, art: art_set }).map_err(|e| e.to_string())
            })();
            Box::new(move |app: &mut App| match r {
                Ok(n) => app.toast(format!("Added \"{name}\" to Steam ({n} user{}). Start Steam to see it.", if n == 1 { "" } else { "s" })),
                Err(e) => app.toast(format!("Add to Steam: {e}")),
            })
        });
    }

    pub fn load_rooms(&mut self) {
        let api = self.api.clone();
        let game = self.rooms_game.clone();
        self.jobs.run(move || {
            let r = api.rooms(game.as_deref(), false);
            Box::new(move |app: &mut App| match r {
                Ok(v) => app.rooms = v,
                Err(e) => app.toast(format!("Rooms: {e}")),
            })
        });
    }

    pub fn join_code(&mut self) {
        let code = warpzone_protocol::names::normalize_code(&self.code_input);
        if code.is_empty() {
            return;
        }
        if let Some((room, secret)) = warpzone_core::launch::parse_invite(self.code_input.trim()) {
            self.open_invite(&room, secret);
            return;
        }
        // Find the room's game first: a code join decides the room on the server.
        let api = self.api.clone();
        self.join_status = Some("Looking up the code…".into());
        self.jobs.run(move || {
            let r = api.join_code(&code, vec![]);
            Box::new(move |app: &mut App| {
                app.join_status = None;
                match r {
                    Ok(JoinResponse::Joined(j)) => {
                        let _ = app.api.leave_room(&j.room.id);
                        app.join(&j.room.game.clone(), &j.room.id.clone(), Some(code.clone()), None);
                    }
                    Ok(JoinResponse::Knocking { room, .. }) => {
                        app.join(&room.game.clone(), &room.id.clone(), Some(code.clone()), None);
                    }
                    Err(e) => app.toast(format!("Code: {e}")),
                }
            })
        });
    }

    pub fn open_invite(&mut self, room: &str, secret: Option<String>) {
        let api = self.api.clone();
        let (rid, sec) = (room.to_string(), secret.clone());
        self.jobs.run(move || {
            let r = api.room(&rid, sec.as_deref());
            Box::new(move |app: &mut App| match r {
                Ok(info) => app.join(&info.game, &info.id, None, secret),
                Err(e) => app.toast(format!("Invite: {e}")),
            })
        });
    }

    pub fn handle_args(&mut self, args: &[String]) {
        self.hidden = false;
        let ctx = self.jobs_ctx();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--play" {
                if let Some(g) = it.next() {
                    self.page = Some(g.clone());
                    self.play_after = Some(g.clone());
                }
            } else if let Some((room, secret)) = warpzone_core::launch::parse_invite(a) {
                self.open_invite(&room, secret);
            }
        }
    }

    fn apply_pending(&mut self) {
        while let Ok(f) = self.rx.try_recv() {
            f(self);
        }
        if let Some(rx) = &self.args_rx {
            let v: Vec<Vec<String>> = rx.try_iter().collect();
            for a in v {
                self.handle_args(&a);
            }
        }
        // --play once the catalog knows the game.
        if let Some(g) = self.play_after.clone()
            && let Some(s) = self.games.iter().find(|x| x.desc.id == g)
            && s.busy.is_none()
            && s.release.is_some()
        {
            if s.installed.is_some() {
                self.play_after = None;
                self.play(&g);
            } else if s.data_ready() && s.data.is_some() {
                self.install(&g, false);
            }
        }
        // Matchmaking client.
        let mut launch: Option<(String, String)> = None;
        if let Some(q) = self.queue.as_mut() {
            q.client.poll();
            while let Some(e) = q.client.next_event() {
                match e {
                    warpzone_core::client::Event::MatchFound { room, .. } => launch = Some((q.game.clone(), room)),
                    warpzone_core::client::Event::Queued(s) => q.status = format!("{} waiting in this queue", s.waiting),
                    warpzone_core::client::Event::Error { message, .. } => q.status = message,
                    _ => {}
                }
            }
        }
        if let Some((game, room)) = launch {
            self.queue = None;
            let args = self
                .games
                .iter()
                .find(|g| g.desc.id == game)
                .and_then(|g| g.desc.launch.as_ref())
                .map(|l| l.join.iter().map(|a| a.replace("{room}", &room)).collect())
                .unwrap_or_default();
            system::notify("Match found", "Starting the game…");
            self.launch_with(&game, args);
        }
        self.toasts.retain(|(_, t)| t.elapsed() < Duration::from_secs(6));
    }
}

// Store the context for jobs that need it (textures).
impl App {
    pub fn set_ctx(&mut self, ctx: &egui::Context) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
    }
}

// ---------------------------------------------------------------- drawing

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).size(12.0).color(color)));
}

#[derive(Clone, Copy)]
enum Glyph {
    Pad,
    Globe,
    Puzzle,
    Trophy,
}

/// One capability icon (drawn, not a font glyph) with an optional number.
fn cap_icon(ui: &mut egui::Ui, glyph: Glyph, label: &str, active: bool, tip: &str) {
    let col = if active { Color32::from_rgb(226, 226, 244) } else { Color32::from_rgb(92, 92, 120) };
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
            let c = rect.center();
            let p = ui.painter();
            let v = |x: f32, y: f32| c + Vec2::new(x, y);
            let thin = Stroke::new(1.4, col);
            match glyph {
                Glyph::Pad => {
                    p.rect_filled(egui::Rect::from_center_size(v(0.0, -0.5), Vec2::new(15.0, 8.0)), CornerRadius::same(4), col);
                    p.circle_filled(v(-5.0, 2.5), 3.2, col);
                    p.circle_filled(v(5.0, 2.5), 3.2, col);
                    p.rect_filled(egui::Rect::from_center_size(v(-4.0, -0.5), Vec2::new(4.0, 1.4)), CornerRadius::ZERO, PANEL);
                    p.rect_filled(egui::Rect::from_center_size(v(-4.0, -0.5), Vec2::new(1.4, 4.0)), CornerRadius::ZERO, PANEL);
                    p.circle_filled(v(3.3, -1.6), 1.0, PANEL);
                    p.circle_filled(v(5.2, 0.3), 1.0, PANEL);
                }
                Glyph::Globe => {
                    p.circle_stroke(c, 7.0, thin);
                    let pts: Vec<egui::Pos2> = (0..=24)
                        .map(|i| {
                            let a = i as f32 / 24.0 * std::f32::consts::TAU;
                            v(3.2 * a.cos(), 7.0 * a.sin())
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, thin));
                    p.line_segment([v(-7.0, 0.0), v(7.0, 0.0)], thin);
                    p.line_segment([v(-5.8, -3.6), v(5.8, -3.6)], Stroke::new(1.0, col));
                    p.line_segment([v(-5.8, 3.6), v(5.8, 3.6)], Stroke::new(1.0, col));
                }
                Glyph::Puzzle => {
                    p.rect_filled(egui::Rect::from_center_size(v(-1.0, 1.5), Vec2::new(11.0, 11.0)), CornerRadius::same(2), col);
                    p.circle_filled(v(-1.0, -5.0), 2.7, col);
                    p.circle_filled(v(5.6, 1.5), 2.7, col);
                    p.circle_filled(v(-1.0, 8.0), 2.0, PANEL);
                }
                Glyph::Trophy => {
                    p.rect_filled(egui::Rect::from_min_max(v(-4.5, -7.0), v(4.5, 0.5)), CornerRadius { nw: 1, ne: 1, sw: 5, se: 5 }, col);
                    p.circle_stroke(v(-5.0, -4.2), 2.3, thin);
                    p.circle_stroke(v(5.0, -4.2), 2.3, thin);
                    p.rect_filled(egui::Rect::from_min_max(v(-1.0, 0.0), v(1.0, 4.0)), CornerRadius::ZERO, col);
                    p.rect_filled(egui::Rect::from_min_max(v(-4.5, 4.0), v(4.5, 6.5)), CornerRadius::same(1), col);
                }
            }
            if !label.is_empty() {
                ui.label(RichText::new(label).size(13.0).color(col).strong());
            }
        })
        .response;
    resp.on_hover_text(tip);
}

/// The icon row: controllers, online, mods, ladder — bright when the game can
/// do it today, dim when it is declared but still coming.
fn capability_row(ui: &mut egui::Ui, d: &GameDescriptor) {
    let c = d.capabilities();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if let Some(n) = c.local_players {
            let tip = if n == 1 { "Single player on one screen".to_string() } else { format!("Up to {n} players on one screen, each with a controller") };
            cap_icon(ui, Glyph::Pad, &n.to_string(), true, &tip);
        }
        if let Some(n) = c.online_players {
            let tip = if c.online_now { format!("Online with up to {n} players through Warpzone") } else { format!("Online play (up to {n}) is coming") };
            cap_icon(ui, Glyph::Globe, &n.to_string(), c.online_now, &tip);
        }
        if c.mods {
            cap_icon(ui, Glyph::Puzzle, "", c.mods_now, if c.mods_now { "Mods from the Warpzone archive" } else { "Mod support is coming" });
        }
        if c.ladder {
            cap_icon(ui, Glyph::Trophy, "", c.ladder_now, if c.ladder_now { "Ranked ladder with seasons" } else { "Ranked play is coming" });
        }
    });
}

fn primary_button(ui: &mut egui::Ui, text: &str, big: bool) -> egui::Response {
    let t = RichText::new(text).color(Color32::WHITE).strong().size(if big { 18.0 } else { 15.0 });
    let b = egui::Button::new(t).fill(ACCENT).corner_radius(CornerRadius::same(10)).min_size(if big { Vec2::new(180.0, 44.0) } else { Vec2::new(110.0, 32.0) });
    ui.add(b)
}

/// The compact primary button of a library tile.
fn tile_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let t = RichText::new(text).color(Color32::WHITE).strong().size(14.0);
    ui.add(egui::Button::new(t).fill(ACCENT).corner_radius(CornerRadius::same(8)).min_size(Vec2::new(76.0, 28.0)))
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("WARPZONE").font(display(22.0)).color(Color32::WHITE));
            ui.add_space(18.0);
            let mut tabs = vec![(Tab::Library, "Library"), (Tab::Rooms, "Rooms"), (Tab::Ladder, "Ladder"), (Tab::Mods, "Mods")];
            if !self.roles.is_empty() {
                tabs.push((Tab::Moderation, "Moderation"));
            }
            tabs.push((Tab::Settings, "Settings"));
            for (t, label) in tabs {
                let sel = self.tab == t;
                let txt = RichText::new(label).size(16.0).color(if sel { Color32::WHITE } else { MUTED });
                if ui.add(egui::Button::selectable(sel, txt).corner_radius(CornerRadius::same(8))).clicked() {
                    self.tab = t;
                    self.page = None;
                    match t {
                        Tab::Rooms => self.load_rooms(),
                        Tab::Moderation => self.load_moderation(),
                        _ => {}
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match self.online {
                    Some(true) => ui.label(RichText::new("online").color(OK).size(13.0)),
                    Some(false) => ui.label(RichText::new("offline").color(WARN).size(13.0)),
                    None => ui.label(RichText::new("connecting").color(MUTED).size(13.0)),
                };
                match &self.account {
                    Some(a) => {
                        ui.label(RichText::new(&a.username).strong());
                    }
                    None => {
                        if ui.link("Choose a username").clicked() {
                            self.tab = Tab::Settings;
                        }
                    }
                }
            });
        });
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            let hidden = self.games.iter().filter(|g| self.settings.hidden_games.contains(&g.desc.id)).count();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if hidden > 0 {
                    ui.checkbox(&mut self.show_hidden, format!("Show hidden games ({hidden})"));
                }
                if ui.button("Refresh").clicked() {
                    self.refresh_catalog();
                }
            });
        });
        ui.add_space(6.0);
        if self.games.is_empty() {
            ui.label(
                RichText::new(if self.online == Some(false) { "Cannot reach the Warpzone server. Installed games still work." } else { "Loading games…" })
                    .color(MUTED),
            );
        }
        let avail = ui.available_width();
        // Small tiles: five columns in the default 1280×800 window, so a
        // library of ten games fits without scrolling.
        let cols = ((avail + 16.0) / (220.0 + 16.0)).floor().max(1.0) as usize;
        let w = (avail - 16.0 * (cols as f32 - 1.0)) / cols as f32;
        let ids: Vec<String> = self
            .games
            .iter()
            .filter(|g| !g.desc.hidden && (self.show_hidden || !self.settings.hidden_games.contains(&g.desc.id)))
            .map(|g| g.desc.id.clone())
            .collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for row in ids.chunks(cols) {
                ui.horizontal_top(|ui| {
                    for id in row {
                        ui.allocate_ui(Vec2::new(w, 200.0), |ui| self.tile(ui, id, w));
                        ui.add_space(16.0 - ui.spacing().item_spacing.x);
                    }
                });
                ui.add_space(16.0);
            }
        });
    }

    fn tile(&mut self, ui: &mut egui::Ui, id: &str, w: f32) {
        let Some(gi) = self.games.iter().position(|g| g.desc.id == id) else { return };
        let primary = self.primary(&self.games[gi]);
        let hidden = self.settings.hidden_games.contains(&id.to_string());
        let mut open = false;
        let mut act: Option<&str> = None;
        Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(14)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(w);
                let g = &self.games[gi];
                let h = w * 430.0 / 920.0;
                let resp = match &g.tile {
                    Some(t) => ui.add(
                        egui::Image::new(t)
                            .fit_to_exact_size(Vec2::new(w, h))
                            .corner_radius(CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 })
                            .sense(egui::Sense::click()),
                    ),
                    None => {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::click());
                        ui.painter().rect_filled(r, CornerRadius::same(14), Color32::from_rgb(30, 30, 52));
                        resp
                    }
                };
                if resp.clicked() {
                    open = true;
                }
                Frame::new().inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
                    capability_row(ui, &g.desc);
                    ui.horizontal(|ui| {
                        let status = match (&g.installed, &primary) {
                            (_, Primary::Busy(b)) => b.clone(),
                            (_, Primary::Running) => "Playing".into(),
                            (Some(i), Primary::Update) => format!("{} - update ready", i.version),
                            (Some(i), _) => format!("Installed {}", i.version),
                            (None, Primary::FindData) => String::new(),
                            (None, Primary::Unavailable(s)) => s.clone(),
                            (None, _) => g
                                .release
                                .as_ref()
                                .map(|r| format!("{} · {}", r.version, install::asset_for(r, install::platform()).map(|a| mb(a.1.size)).unwrap_or_default()))
                                .unwrap_or_default(),
                        };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button(RichText::new("...").size(15.0), |ui| {
                                if ui.button("Open game page").clicked() {
                                    open = true;
                                    ui.close();
                                }
                                if ui.button(if hidden { "Show in library" } else { "Hide from library" }).clicked() {
                                    act = Some("hide");
                                    ui.close();
                                }
                            });
                            match &primary {
                                Primary::Play => {
                                    if tile_button(ui, "Play").clicked() {
                                        act = Some("play");
                                    }
                                }
                                Primary::Install => {
                                    if tile_button(ui, "Install").clicked() {
                                        act = Some("install");
                                    }
                                }
                                Primary::Update => {
                                    if tile_button(ui, "Update").clicked() {
                                        act = Some("install");
                                    }
                                }
                                Primary::FindData => {
                                    if ui.button("Find game files").clicked() {
                                        open = true;
                                    }
                                }
                                Primary::Busy(_) => {
                                    if let Some((d, t)) = g.progress
                                        && t > 0
                                    {
                                        ui.add(egui::ProgressBar::new(d as f32 / t as f32).desired_width(80.0));
                                    } else {
                                        ui.spinner();
                                    }
                                }
                                Primary::Unavailable(_) | Primary::Running => {}
                            }
                            // What is left of the row, truncated (full text on hover).
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(status).color(MUTED).size(13.0)).truncate());
                            });
                        });
                    });
                    if let Some(e) = &g.error {
                        ui.label(RichText::new(e).color(WARN).size(12.0));
                    }
                });
            })
        });
        if open {
            self.page = Some(id.to_string());
        }
        match act {
            Some("play") => self.play(id),
            Some("install") => self.install(id, false),
            Some("hide") => {
                if hidden {
                    self.settings.hidden_games.retain(|g| g != id);
                } else {
                    self.settings.hidden_games.push(id.to_string());
                    self.toast("Hidden — bring it back with \"Show hidden games\" or in Settings");
                }
                self.settings.save();
            }
            _ => {}
        }
    }

    /// Remember an extra folder to search for game files and search again.
    fn add_data_folder(&mut self, id: &str, f: String) {
        if !f.is_empty() && !self.settings.data_folders.contains(&f) {
            self.settings.data_folders.push(f);
            self.settings.save();
        }
        self.scan_data(id);
    }

    fn game_page(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(gi) = self.games.iter().position(|g| g.desc.id == id) else {
            self.page = None;
            return;
        };
        if ui.button("< Library").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.page = None;
            return;
        }
        let primary = self.primary(&self.games[gi]);
        let mut act: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let g = &self.games[gi];
            let w = ui.available_width();
            if let Some(t) = &g.tile {
                let h = (w * 430.0 / 920.0).min(300.0);
                ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::new(w, h)).maintain_aspect_ratio(false).corner_radius(CornerRadius::same(14)));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&g.desc.name).font(display(32.0)).color(Color32::WHITE));
                    ui.label(RichText::new(&g.desc.tagline).color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        if let Some(s) = &g.desc.status {
                            badge(ui, s, WARN);
                        }
                        if g.desc.tier >= 2 {
                            for f in g.desc.features() {
                                badge(ui, f, OK);
                            }
                        } else if g.desc.tier0.is_some() {
                            badge(ui, "launcher lobby", OK);
                        } else {
                            badge(ui, "Warpzone support coming", MUTED);
                        }
                    });
                    capability_row(ui, &g.desc);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &primary {
                    Primary::Play => {
                        if primary_button(ui, "Play", true).clicked() {
                            act = Some("play".into());
                        }
                    }
                    Primary::Install => {
                        let size = g.release.as_ref().and_then(|r| install::asset_for(r, install::platform())).map(|a| mb(a.1.size)).unwrap_or_default();
                        if primary_button(ui, &format!("Install  ({size})"), true).clicked() {
                            act = Some("install".into());
                        }
                    }
                    Primary::Update => {
                        if primary_button(ui, "Update", true).clicked() {
                            act = Some("install".into());
                        }
                    }
                    Primary::Busy(b) => {
                        ui.vertical(|ui| {
                            ui.label(b);
                            if let Some((d, t)) = g.progress
                                && t > 0
                            {
                                ui.add(egui::ProgressBar::new(d as f32 / t as f32).desired_width(220.0).show_percentage());
                            }
                        });
                    }
                    Primary::FindData => {
                        ui.label(RichText::new("Your game files are needed first (below)").color(WARN));
                    }
                    Primary::Running => {
                        ui.add_enabled(false, egui::Button::new(RichText::new("Running").size(18.0)).min_size(Vec2::new(180.0, 44.0)));
                    }
                    Primary::Unavailable(s) => {
                        ui.label(RichText::new(s).color(MUTED));
                    }
                });
            });
            if let Some(e) = &g.error {
                ui.label(RichText::new(e).color(WARN));
            }
            ui.add_space(8.0);
            // Game data.
            if let Some(scans) = &g.data {
                for (si, s) in scans.iter().enumerate() {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&s.req.label).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if s.present {
                                    ui.label(RichText::new("in place").color(OK));
                                } else if s.chosen.is_some() {
                                    ui.label(RichText::new("found").color(OK));
                                } else if s.needs_fetch() {
                                    ui.label(RichText::new("free download").color(OK));
                                } else if s.req.optional {
                                    ui.label(RichText::new("optional").color(MUTED));
                                } else {
                                    ui.label(RichText::new("needed").color(WARN));
                                }
                            });
                        });
                        if let Some(d) = &s.req.description {
                            ui.label(RichText::new(d).color(MUTED).size(13.5));
                        }
                        for (f, c) in &s.found {
                            ui.horizontal(|ui| {
                                let chosen = s.chosen.as_ref() == Some(&f.path);
                                let (txt, col) = match c {
                                    Check::Verified(l) => (format!("verified: {l}"), OK),
                                    Check::Unsupported(l) => (format!("not usable: {l}"), WARN),
                                    Check::Unknown => ("unknown version".to_string(), WARN),
                                    Check::Unchecked => (String::new(), MUTED),
                                };
                                if ui.radio(chosen, format!("{}  ({})", f.path.display(), f.source)).clicked() && !matches!(c, Check::Unsupported(_)) {
                                    act = Some(format!("choose:{si}:{}", f.path.display()));
                                }
                                if !txt.is_empty() {
                                    ui.label(RichText::new(txt).color(col).size(13.0));
                                }
                            });
                        }
                        if s.needs_fetch()
                            && let Some(f) = s.req.fetch.first()
                        {
                            ui.label(
                                RichText::new(format!(
                                    "Not found on this computer. The launcher downloads it by itself when you install or play ({}, from {}). {}",
                                    mb(f.size),
                                    f.label,
                                    f.license
                                ))
                                .color(MUTED)
                                .size(13.5),
                            );
                        } else if !s.present && s.found.is_empty() {
                            ui.label(
                                RichText::new("Not found in Steam, GOG, Heroic, Lutris, Wine, your Downloads folder or on discs.").color(MUTED).size(13.5),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Search again").clicked() {
                                act = Some("rescan".into());
                            }
                            if s.needs_fetch() && g.busy.is_none() && ui.button("Download now").clicked() {
                                act = Some("fetch".into());
                            }
                            for l in &s.req.free {
                                if ui.button(format!("Free: {}", l.label)).clicked() {
                                    system::open(&l.url);
                                }
                            }
                            for l in &s.req.buy {
                                if ui.button(format!("Buy: {}", l.label)).clicked() {
                                    system::open(&l.url);
                                }
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Files elsewhere?").color(MUTED).size(13.5));
                    if ui.button("Browse…").on_hover_text("Choose the folder with the game files; a .zip download in it is found too").clicked() {
                        act = Some("browse".into());
                    }
                    ui.label(RichText::new("or type it:").color(MUTED).size(13.5));
                    ui.add(egui::TextEdit::singleline(&mut self.data_folder_input).hint_text("/path/to/folder").desired_width(280.0));
                    if ui.button("Add & search").clicked() {
                        act = Some("addfolder".into());
                    }
                });
            }
            if !g.desc.description.is_empty() {
                ui.add_space(8.0);
                ui.label(&g.desc.description);
            }
            if let Some(r) = &g.desc.requires {
                ui.label(RichText::new(format!("Needs: {r}")).color(MUTED).size(13.5));
            }
            // What's new.
            if !g.changelog.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new("What's new").font(display(20.0)));
                let seen = self.settings.seen_versions.get(&g.desc.id).cloned();
                for e in g.changelog.iter().take(3) {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if e.name.is_empty() { &e.version } else { &e.name }).strong());
                            if e.prerelease {
                                badge(ui, "preview", WARN);
                            }
                            if seen.as_deref().is_some_and(|s| s != e.version) && g.installed.as_ref().is_some_and(|i| i.version == e.version) {
                                badge(ui, "new for you", ACCENT2);
                            }
                            ui.label(RichText::new(e.published.get(..10).unwrap_or("")).color(MUTED).size(12.5));
                        });
                        let notes: String = e.notes.lines().take(14).collect::<Vec<_>>().join("\n");
                        ui.label(RichText::new(notes.replace("**", "")).size(13.5));
                    });
                    ui.add_space(4.0);
                }
            }
            // Online.
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Online").font(display(20.0)));
                let can_host = g.installed.is_some() && (g.desc.tier >= 2 || g.desc.tier0.is_some());
                if can_host {
                    egui::ComboBox::from_id_salt("vis")
                        .selected_text(match self.host_visibility {
                            Visibility::Public => "Public",
                            Visibility::Unlisted => "Private (code/link)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.host_visibility, Visibility::Public, "Public");
                            ui.selectable_value(&mut self.host_visibility, Visibility::Unlisted, "Private (code/link)");
                        });
                    egui::ComboBox::from_id_salt("entry")
                        .selected_text(match self.host_entry {
                            Entry::Open => "Anyone joins",
                            Entry::Code => "Code required",
                            Entry::Approve => "I approve joiners",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.host_entry, Entry::Open, "Anyone joins");
                            ui.selectable_value(&mut self.host_entry, Entry::Code, "Code required");
                            ui.selectable_value(&mut self.host_entry, Entry::Approve, "I approve joiners");
                        });
                    if ui.button("Host a game").clicked() {
                        act = Some("host".into());
                    }
                }
            });
            let rooms: Vec<&RoomInfo> = self.rooms.iter().filter(|r| r.game == g.desc.id).collect();
            if rooms.is_empty() {
                ui.label(RichText::new("No open rooms for this game right now.").color(MUTED));
            }
            for r in rooms {
                ui.horizontal(|ui| {
                    ui.label(format!("{}  {}/{}", r.name, r.players, r.max_players));
                    if ui.button("Join").clicked() {
                        act = Some(format!("join:{}", r.id));
                    }
                });
            }
            // Manage.
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if steam::steam_installed() && g.installed.is_some() {
                    let name = g.desc.steam.as_ref().and_then(|s| s.name.clone()).unwrap_or(g.desc.name.clone());
                    let label = if steam::has(&name) { "In Steam (update shortcut)" } else { "Add to Steam" };
                    if ui.button(label).clicked() {
                        act = Some("steam".into());
                    }
                }
                if let Some(h) = &g.desc.homepage
                    && ui.button("Project page").clicked()
                {
                    system::open(h);
                }
                if let Some(i) = &g.installed {
                    if ui.button("Open install folder").clicked() {
                        system::open(&i.dir.display().to_string());
                    }
                    if ui.button("Previous version").clicked() {
                        act = Some("rollback".into());
                    }
                    if ui.button("Uninstall").clicked() {
                        act = Some("uninstall".into());
                    }
                }
            });
        });
        let Some(a) = act else { return };
        match a.as_str() {
            "play" => self.play(id),
            "install" => self.install(id, false),
            "host" => self.host(id),
            "rescan" => self.scan_data(id),
            "fetch" => self.fetch_data(id, false),
            "steam" => self.add_to_steam(Some(id.to_string())),
            "addfolder" => {
                let f = self.data_folder_input.trim().to_string();
                self.data_folder_input.clear();
                self.add_data_folder(id, f);
            }
            "browse" => {
                let gid = id.to_string();
                let title = format!("Folder with the files for {}", self.games[gi].desc.name);
                self.jobs.run(move || {
                    let r = system::pick_folder(&title);
                    Box::new(move |app: &mut App| match r {
                        Ok(Some(p)) => app.add_data_folder(&gid, p.display().to_string()),
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!("folder dialog: {e}");
                            app.toast("No folder dialog on this system (zenity or kdialog): type the path instead");
                        }
                    })
                });
            }
            "rollback" => match install::rollback(id) {
                Ok(i) => {
                    self.toast(format!("Back on {}", i.version));
                    if let Some(g) = self.game(id) {
                        g.installed = Some(i);
                    }
                }
                Err(e) => self.toast(e.to_string()),
            },
            "uninstall" => {
                let _ = install::uninstall(id);
                if let Some(g) = self.game(id) {
                    g.installed = None;
                }
                self.scan_data(id);
            }
            other => {
                if let Some(rest) = other.strip_prefix("join:") {
                    self.join(id, rest, None, None);
                } else if let Some(rest) = other.strip_prefix("choose:") {
                    let mut parts = rest.splitn(2, ':');
                    let si: usize = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let path = PathBuf::from(parts.next().unwrap_or(""));
                    if let Some(g) = self.game(id)
                        && let Some(d) = g.data.as_mut()
                        && let Some(s) = d.get_mut(si)
                    {
                        s.chosen = Some(path);
                    }
                }
            }
        }
    }

    fn rooms_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Open rooms");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.load_rooms();
                }
            });
        });
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Join by code or invite link:");
                let r = ui.add(egui::TextEdit::singleline(&mut self.code_input).hint_text("K7QM3X").desired_width(260.0));
                if (ui.button("Join").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.code_input.trim().is_empty() {
                    self.join_code();
                }
                if let Some(s) = &self.join_status {
                    ui.spinner();
                    ui.label(s);
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Game:");
            let current =
                self.rooms_game.clone().and_then(|g| self.games.iter().find(|x| x.desc.id == g).map(|x| x.desc.name.clone())).unwrap_or("All games".into());
            let mut changed = false;
            egui::ComboBox::from_id_salt("roomgame").selected_text(current).show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut self.rooms_game, None, "All games").clicked();
                for g in &self.games {
                    changed |= ui.selectable_value(&mut self.rooms_game, Some(g.desc.id.clone()), &g.desc.name).clicked();
                }
            });
            if changed {
                self.load_rooms();
            }
        });
        let mut join: Option<(String, String)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.rooms.is_empty() {
                ui.label(RichText::new("No public rooms right now. Host one from a game's page, or join a private game with its code.").color(MUTED));
            }
            for r in &self.rooms {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        let gname = self.games.iter().find(|g| g.desc.id == r.game).map(|g| g.desc.name.clone()).unwrap_or(r.game.clone());
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&r.name).strong().size(16.0));
                            ui.label(RichText::new(format!("{gname} · {}/{} players · v{}", r.players, r.max_players, r.game_version)).color(MUTED).size(13.0));
                            let langs = if r.audience.languages.is_empty() { "any language".into() } else { r.audience.languages.join(", ").to_uppercase() };
                            ui.horizontal(|ui| {
                                badge(ui, &langs, MUTED);
                                badge(ui, &format!("{:?}", r.audience.skill).to_lowercase(), MUTED);
                                badge(ui, &format!("{:?}", r.audience.style).to_lowercase(), MUTED);
                                if r.entry == Entry::Approve {
                                    badge(ui, "host approves", WARN);
                                }
                                if r.lockfile.is_some() {
                                    badge(ui, "modded", ACCENT2);
                                }
                            });
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if primary_button(ui, "Join", false).clicked() {
                                join = Some((r.game.clone(), r.id.clone()));
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });
        if let Some((g, r)) = join {
            self.join(&g, &r, None, None);
        }
    }

    fn ladder_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ladder");
        ui.label(
            RichText::new(
                "Ranked play is optional. Seasons follow the calendar (quarterly unless a game says otherwise); ratings carry over with a soft reset.",
            )
            .color(MUTED),
        );
        let mut load: Option<(String, String)> = None;
        ui.horizontal_wrapped(|ui| {
            for g in &self.games {
                for q in &g.desc.ladder {
                    let sel = self.ladder_pick.as_ref() == Some(&(g.desc.id.clone(), q.id.clone()));
                    if ui.selectable_label(sel, format!("{} — {}", g.desc.name, q.title)).clicked() {
                        load = Some((g.desc.id.clone(), q.id.clone()));
                    }
                }
            }
        });
        if let Some((g, q)) = load {
            self.ladder_pick = Some((g.clone(), q.clone()));
            let api = self.api.clone();
            self.jobs.run(move || {
                let r = api.standings(&g, &q, None);
                Box::new(move |app: &mut App| match r {
                    Ok(s) => app.standings = Some(s),
                    Err(e) => app.toast(format!("Ladder: {e}")),
                })
            });
        }
        let Some((gid, qid)) = self.ladder_pick.clone() else { return };
        let tier = self.games.iter().find(|g| g.desc.id == gid).map(|g| g.desc.tier).unwrap_or(0);
        let installed = self.games.iter().find(|g| g.desc.id == gid).and_then(|g| g.installed.clone());
        ui.add_space(6.0);
        ui.horizontal(|ui| match &self.queue {
            Some(q) => {
                ui.spinner();
                ui.label(format!("Looking for a {} match… {} ({}s)", q.queue, q.status, q.since.elapsed().as_secs()));
                if ui.button("Cancel").clicked() {
                    self.queue.as_ref().unwrap().client.leave_queue();
                    self.queue = None;
                }
            }
            None => {
                if tier >= 2
                    && let Some(inst) = &installed
                {
                    if primary_button(ui, "Find a match", false).clicked() {
                        let mut cfg = warpzone_core::client::Config::new(&gid, &inst.version);
                        cfg.server = self.settings.server();
                        cfg.auto_join_matches = false;
                        if let Ok(c) = warpzone_core::client::Client::new(cfg) {
                            c.queue(&qid, "stock");
                            self.queue = Some(Queue { game: gid.clone(), queue: qid.clone(), client: c, since: Instant::now(), status: String::new() });
                        }
                    }
                } else {
                    ui.label(RichText::new("Ranked matches start once this game supports Warpzone online play.").color(MUTED));
                }
            }
        });
        if let Some(s) = &self.standings {
            card(ui, |ui| {
                ui.label(
                    RichText::new(format!("Season {}{}", s.season, if s.placing > 0 { format!(" · {} still placing", s.placing) } else { String::new() }))
                        .color(MUTED),
                );
                if s.standings.is_empty() {
                    ui.label("Nobody has finished their placement matches this season yet.");
                }
                egui::Grid::new("standings").striped(true).num_columns(4).show(ui, |ui| {
                    ui.label(RichText::new("#").color(MUTED));
                    ui.label(RichText::new("Player").color(MUTED));
                    ui.label(RichText::new("Rating").color(MUTED));
                    ui.label(RichText::new("Games").color(MUTED));
                    ui.end_row();
                    for r in &s.standings {
                        ui.label(r.rank.to_string());
                        ui.label(&r.username);
                        ui.label(r.rating.to_string());
                        ui.label(format!("{} ({} won)", r.games, r.wins));
                        ui.end_row();
                    }
                });
            });
        }
    }

    fn mods_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mods");
        ui.label(RichText::new("Every version is checked by a moderator before it appears. Dependencies install automatically; joining a modded room installs exactly its set.").color(MUTED));
        let mut load = false;
        if self.mods_game.is_none()
            && let Some(g) = self.games.iter().find(|g| g.desc.mods.is_some())
        {
            self.mods_game = Some(g.desc.id.clone());
            load = true;
        }
        ui.horizontal(|ui| {
            let current =
                self.mods_game.clone().and_then(|g| self.games.iter().find(|x| x.desc.id == g).map(|x| x.desc.name.clone())).unwrap_or("Choose a game".into());
            egui::ComboBox::from_id_salt("modgame").selected_text(current).show_ui(ui, |ui| {
                for g in self.games.iter().filter(|g| g.desc.mods.is_some()) {
                    load |= ui.selectable_value(&mut self.mods_game, Some(g.desc.id.clone()), &g.desc.name).clicked();
                }
            });
            let r = ui.add(egui::TextEdit::singleline(&mut self.mods_query).hint_text("search").desired_width(220.0));
            load |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            load |= ui.button("Search").clicked();
        });
        let Some(gid) = self.mods_game.clone() else { return };
        if load {
            let api = self.api.clone();
            let q = self.mods_query.trim().to_string();
            let g = gid.clone();
            self.jobs.run(move || {
                let r = api.mods(&g, None, if q.is_empty() { None } else { Some(&q) });
                Box::new(move |app: &mut App| match r {
                    Ok(v) => app.mods = v,
                    Err(e) => app.toast(format!("Mods: {e}")),
                })
            });
        }
        let store = warpzone_core::modstore::ModStore::new(&gid, false);
        let active = store.active();
        let mut act: Option<(String, bool)> = None;
        let mut detail: Option<String> = None;
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical().id_salt("modlist").show(&mut cols[0], |ui| {
                if self.mods.is_empty() {
                    ui.label(RichText::new("No published mods for this game yet.").color(MUTED));
                }
                for m in &self.mods {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                if ui.link(RichText::new(&m.title).strong()).clicked() {
                                    detail = Some(m.name.clone());
                                }
                                ui.label(
                                    RichText::new(format!("{} · {} · by {} · {} downloads", m.latest, m.category, m.author, m.downloads))
                                        .color(MUTED)
                                        .size(12.5),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let on = active.as_ref().is_some_and(|l| l.mods.iter().any(|e| e.name == m.name));
                                if on {
                                    if ui.button("Remove").clicked() {
                                        act = Some((m.name.clone(), false));
                                    }
                                } else if primary_button(ui, "Add", false).clicked() {
                                    act = Some((m.name.clone(), true));
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            let ui = &mut cols[1];
            card(ui, |ui| {
                ui.label(RichText::new("Your active set").strong());
                match &active {
                    Some(l) if !l.mods.is_empty() => {
                        for name in &l.order {
                            let v = l.mods.iter().find(|m| &m.name == name).map(|m| m.version.clone()).unwrap_or_default();
                            ui.label(format!("{name} {v}"));
                        }
                    }
                    _ => {
                        ui.label(RichText::new("No mods active — the game runs stock.").color(MUTED));
                    }
                }
            });
            if let Some(d) = &self.mod_detail {
                card(ui, |ui| {
                    ui.label(RichText::new(&d.summary.title).font(display(18.0)));
                    ui.label(RichText::new(format!("by {} · {}", d.summary.author, d.license)).color(MUTED));
                    if d.contains_code {
                        ui.label(RichText::new("Mods for this game can contain code. Only install mods from authors you trust.").color(WARN));
                    }
                    ui.label(&d.summary.description);
                    for v in d.versions.iter().take(5) {
                        ui.label(
                            RichText::new(format!(
                                "{} — game {} — {}",
                                v.version,
                                v.game_version,
                                if v.dependencies.is_empty() { "no dependencies".into() } else { v.dependencies.join(", ") }
                            ))
                            .size(13.0),
                        );
                    }
                });
            }
        });
        if let Some(name) = detail {
            let api = self.api.clone();
            let g = gid.clone();
            self.jobs.run(move || {
                let r = api.mod_detail(&g, &name);
                Box::new(move |app: &mut App| {
                    if let Ok(d) = r {
                        app.mod_detail = Some(d);
                    }
                })
            });
        }
        if let Some((name, add)) = act {
            let mut want: Vec<String> = active.map(|l| l.mods.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
            if add {
                want.push(name);
            } else {
                want.retain(|n| *n != name);
            }
            let version = self
                .games
                .iter()
                .find(|g| g.desc.id == gid)
                .and_then(|g| g.installed.as_ref().map(|i| i.version.clone()).or(g.release.as_ref().map(|r| r.version.clone())))
                .unwrap_or("0.0.0".into());
            let extract = self.games.iter().find(|g| g.desc.id == gid).and_then(|g| g.desc.mods.as_ref()).is_some_and(|m| m.install == "extract");
            let api = self.api.clone();
            let g = gid.clone();
            self.toast("Updating your mod set…");
            self.jobs.run(move || {
                let store = warpzone_core::modstore::ModStore::new(&g, extract);
                let r = if want.is_empty() {
                    store.set_active(None).map(|_| 0usize)
                } else {
                    api.resolve(&g, &version, want).and_then(|l| store.sync(&api, &l, &mut |_, _| {}).map(|p| p.len()))
                };
                Box::new(move |app: &mut App| match r {
                    Ok(n) => app.toast(format!("Mod set ready ({n} mods)")),
                    Err(e) => app.toast(format!("Mods: {e}")),
                })
            });
        }
    }

    pub fn load_moderation(&mut self) {
        let api = self.api.clone();
        self.jobs.run(move || {
            let r = api.moderation_queue();
            Box::new(move |app: &mut App| match r {
                Ok(q) => app.moderation = Some(q),
                Err(e) => app.toast(format!("Moderation: {e}")),
            })
        });
    }

    fn moderation_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Moderation");
            if ui.button("Refresh").clicked() {
                self.load_moderation();
            }
        });
        let is_mod = self.roles.iter().any(|r| r == "moderator");
        let mut decide: Option<(String, String, String, String)> = None;
        let mut download: Option<PendingMod> = None;
        ui.horizontal(|ui| {
            ui.label("Reason for rejections:");
            egui::ComboBox::from_id_salt("reason").selected_text(self.reject_reason.clone()).show_ui(ui, |ui| {
                for r in REJECT_REASONS {
                    ui.selectable_value(&mut self.reject_reason, r.to_string(), *r);
                }
            });
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let Some(q) = &self.moderation else {
                ui.label("Loading…");
                return;
            };
            if q.pending.is_empty() && q.reports.is_empty() {
                ui.label(RichText::new("Nothing waiting.").color(OK));
            }
            for p in &q.pending {
                card(ui, |ui| {
                    ui.label(RichText::new(format!("{} — {} {}", p.title, p.name, p.version)).strong());
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · by {} · {} KiB{}",
                            p.game,
                            p.category,
                            p.author,
                            p.size / 1024,
                            if p.contains_code { " · may contain code" } else { "" }
                        ))
                        .color(MUTED),
                    );
                    for n in &p.notes {
                        ui.label(RichText::new(format!("{}: {}", n.role, n.text)).size(13.0));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Download to inspect").clicked() {
                            download = Some(p.clone());
                        }
                        if is_mod {
                            if primary_button(ui, "Approve", false).clicked() {
                                decide = Some((p.game.clone(), p.name.clone(), p.version.clone(), "approve".into()));
                            }
                            if ui.button(format!("Reject ({})", self.reject_reason)).clicked() {
                                decide = Some((p.game.clone(), p.name.clone(), p.version.clone(), "reject".into()));
                            }
                        }
                    });
                });
                ui.add_space(6.0);
            }
            for r in &q.reports {
                card(ui, |ui| {
                    ui.label(format!("Report #{}: {} {} {} — {}", r.id, r.target, r.game.as_deref().unwrap_or(""), r.name, r.reason));
                    if let Some(d) = &r.detail {
                        ui.label(RichText::new(d).color(MUTED));
                    }
                });
            }
        });
        if let Some((g, n, v, d)) = decide {
            let api = self.api.clone();
            let reason = (d == "reject").then(|| self.reject_reason.clone());
            self.jobs.run(move || {
                let r = api.decide(&g, &n, &v, &d, reason.as_deref());
                Box::new(move |app: &mut App| {
                    match r {
                        Ok(_) => app.toast(format!("{n} {v}: {d}d")),
                        Err(e) => app.toast(e.to_string()),
                    }
                    app.load_moderation();
                })
            });
        }
        if let Some(p) = download {
            let api = self.api.clone();
            self.jobs.run(move || {
                let dir = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default().join("Downloads");
                let dest = dir.join(format!("{}_{}.review.zip", p.name, p.version));
                let r = api.download_mod(&p.game, &p.name, &p.version, &p.sha256, &dest);
                Box::new(move |app: &mut App| match r {
                    Ok(_) => {
                        app.toast(format!("Saved {}", dest.display()));
                        system::open(&dir.display().to_string());
                    }
                    Err(e) => app.toast(e.to_string()),
                })
            });
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading("Settings");
            card(ui, |ui| {
                ui.label(RichText::new("Running in the background").font(display(18.0)));
                ui.label(RichText::new("When on, closing the window keeps Warpzone running: it updates your games and tells you when a match is found. It also starts with your desktop. When off, Warpzone runs only while its window is open.").color(MUTED));
                let mut on = self.settings.background.unwrap_or(false);
                if ui.checkbox(&mut on, RichText::new("Keep running in the background").size(16.0)).changed() {
                    self.settings.background = Some(on);
                    let _ = system::set_autostart(on);
                    self.settings.save();
                }
                if on && !system::autostart_enabled() {
                    ui.label(RichText::new("Autostart entry missing — toggle off and on to recreate it.").color(WARN).size(12.5));
                }
                if ui.checkbox(&mut self.settings.auto_update, "Update games automatically").changed() {
                    self.settings.save();
                }
            });
            ui.add_space(8.0);
            card(ui, |ui| {
                ui.label(RichText::new("Your identity").font(display(18.0)));
                ui.label(RichText::new("No password and no e-mail: this device's key is your login. Use a link code to play as the same player on another device.").color(MUTED));
                match self.account.clone() {
                    Some(a) => {
                        ui.label(format!("Playing as {}", a.username));
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.username_input).hint_text("username (3–16 letters, digits, _ -)").desired_width(260.0));
                            if primary_button(ui, "Claim", false).clicked() {
                                let api = self.api.clone();
                                let name = self.username_input.trim().to_string();
                                self.jobs.run(move || {
                                    let r = api.register(&name);
                                    Box::new(move |app: &mut App| match r {
                                        Ok(a) => {
                                            app.toast(format!("Welcome, {}!", a.username));
                                            app.account = Some(a);
                                        }
                                        Err(e) => app.toast(e.to_string()),
                                    })
                                });
                            }
                        });
                    }
                }
                ui.horizontal(|ui| {
                    if self.account.is_some() && ui.button("Get a link code").clicked() {
                        let api = self.api.clone();
                        self.jobs.run(move || {
                            let r = api.link_code();
                            Box::new(move |app: &mut App| match r {
                                Ok(c) => app.link_code = Some(c.code),
                                Err(e) => app.toast(e.to_string()),
                            })
                        });
                    }
                    if let Some(c) = &self.link_code {
                        ui.label(RichText::new(c).font(display(20.0)).color(ACCENT2));
                        ui.label(RichText::new("valid 10 minutes").color(MUTED));
                    }
                });
                if self.account.is_none() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.link_input).hint_text("link code from your other device").desired_width(260.0));
                        if ui.button("Link this device").clicked() {
                            let api = self.api.clone();
                            let code = self.link_input.trim().to_string();
                            self.jobs.run(move || {
                                let r = api.link(&code);
                                Box::new(move |app: &mut App| match r {
                                    Ok(a) => app.account = Some(a),
                                    Err(e) => app.toast(e.to_string()),
                                })
                            });
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Show recovery code").clicked() {
                        self.recovery = Some(self.api.identity().recovery_code());
                    }
                    if let Some(r) = &self.recovery {
                        ui.label(RichText::new(r).monospace().size(12.0));
                    }
                });
                if self.recovery.is_some() {
                    ui.label(RichText::new("Anyone with this code can play as you. Write it down and keep it private.").color(WARN).size(12.5));
                }
                if self.account.is_some() {
                    ui.horizontal(|ui| {
                        if !self.confirm_delete {
                            if ui.button("Delete account…").clicked() {
                                self.confirm_delete = true;
                            }
                        } else {
                            ui.label(RichText::new("Delete your account, ratings and scores for good?").color(WARN));
                            if ui.button("Yes, delete").clicked() {
                                self.confirm_delete = false;
                                let api = self.api.clone();
                                self.jobs.run(move || {
                                    let r = api.delete_account();
                                    Box::new(move |app: &mut App| match r {
                                        Ok(_) => {
                                            app.account = None;
                                            app.toast("Account deleted");
                                        }
                                        Err(e) => app.toast(e.to_string()),
                                    })
                                });
                            }
                            if ui.button("Cancel").clicked() {
                                self.confirm_delete = false;
                            }
                        }
                    });
                }
            });
            ui.add_space(8.0);
            card(ui, |ui| {
                ui.label(RichText::new("Library").font(display(18.0)));
                if self.settings.hidden_games.is_empty() {
                    ui.label(RichText::new("No hidden games.").color(MUTED));
                }
                let mut unhide = None;
                for h in &self.settings.hidden_games {
                    ui.horizontal(|ui| {
                        let name = self.games.iter().find(|g| &g.desc.id == h).map(|g| g.desc.name.clone()).unwrap_or(h.clone());
                        ui.label(name);
                        if ui.button("Show again").clicked() {
                            unhide = Some(h.clone());
                        }
                    });
                }
                if let Some(h) = unhide {
                    self.settings.hidden_games.retain(|x| *x != h);
                    self.settings.save();
                }
                ui.label(RichText::new(format!("Extra folders searched for game files: {}", if self.settings.data_folders.is_empty() { "none".into() } else { self.settings.data_folders.join(", ") })).color(MUTED).size(13.0));
            });
            ui.add_space(8.0);
            card(ui, |ui| {
                ui.label(RichText::new("Desktop & Steam").font(display(18.0)));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open warpzone:// links with Warpzone").clicked() {
                        match system::register_uri_handler() {
                            Ok(()) => self.toast("Invite links now open in Warpzone"),
                            Err(e) => self.toast(e.to_string()),
                        }
                    }
                    if steam::steam_installed() && ui.button("Add Warpzone to Steam").clicked() {
                        self.add_to_steam(None);
                    }
                });
                if steam::steam_installed() {
                    ui.label(RichText::new("Adding to Steam closes Steam first if it is running (Steam would undo the change), then adds the shortcut with full artwork.").color(MUTED).size(12.5));
                }
            });
            ui.add_space(8.0);
            card(ui, |ui| {
                ui.label(RichText::new("About").font(display(18.0)));
                ui.label(format!("Warpzone launcher {} · server {}", env!("CARGO_PKG_VERSION"), self.settings.server()));
                ui.label(RichText::new(format!("Data folder: {}", warpzone_core::paths::home().display())).color(MUTED).size(13.0));
                ui.horizontal(|ui| {
                    ui.label("Server:");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.server).hint_text(warpzone_core::DEFAULT_SERVER).desired_width(320.0));
                    if ui.button("Save").clicked() {
                        self.settings.save();
                        self.toast("Saved — restart the launcher to use the new server");
                    }
                });
                if ui.link("Source, documentation and self-hosting").clicked() {
                    system::open("https://github.com/jarvis-jhk/warpzone");
                }
            });
        });
    }

    fn first_run_modal(&mut self, ctx: &egui::Context) {
        egui::Modal::new(egui::Id::unique("first-run")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Welcome to Warpzone").font(display(26.0)));
            ui.add_space(6.0);
            ui.label("Should Warpzone keep running in the background when you close its window?");
            ui.label(RichText::new("In the background it keeps your games updated and tells you when a match is found, and it starts with your desktop. You can change this any time in Settings.").color(MUTED));
            ui.add_space(10.0);
            ui.columns(2, |c| {
                if c[0].add_sized([250.0, 44.0], egui::Button::new(RichText::new("Run in the background").size(16.0))).clicked() {
                    self.settings.background = Some(true);
                    let _ = system::set_autostart(true);
                    self.settings.save();
                    self.first_run = false;
                }
                if c[1].add_sized([250.0, 44.0], egui::Button::new(RichText::new("Only while open").size(16.0))).clicked() {
                    self.settings.background = Some(false);
                    let _ = system::set_autostart(false);
                    self.settings.save();
                    self.first_run = false;
                }
            });
        });
    }

    fn lobby_view(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.lobby else { return };
        let v = l.snapshot();
        let game = l.game.clone();
        let desc = self.games.iter().find(|g| g.desc.id == game).map(|g| g.desc.clone());
        let mut leave = false;
        let mut cmds = Vec::new();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} lobby", desc.as_ref().map(|d| d.name.as_str()).unwrap_or(&game))).font(display(22.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(if v.running { "Leave after the game" } else { "Leave" }).clicked() {
                        leave = true;
                    }
                });
            });
            ui.label(RichText::new(&v.status).color(MUTED));
            if let Some(e) = &v.error {
                ui.label(RichText::new(e).color(WARN));
            }
            if let Some(c) = &v.code {
                ui.horizontal(|ui| {
                    ui.label("Code:");
                    ui.label(RichText::new(c).font(display(26.0)).color(ACCENT2));
                    if let Some(i) = &v.invite
                        && ui.button("Copy invite link").clicked()
                    {
                        ui.ctx().copy_text(i.clone());
                    }
                });
            }
            ui.label(RichText::new("Players").strong());
            for p in &v.players {
                let conn = v.connections.iter().find(|(n, _)| *n == p.name).map(|(_, r)| if *r { " · relayed" } else { " · direct" }).unwrap_or("");
                ui.label(format!("Seat {} — {}{}", p.seat + 1, p.name, conn));
            }
            if let Some(t0) = desc.as_ref().and_then(|d| d.tier0.as_ref()) {
                ui.add_space(6.0);
                for s in &t0.settings {
                    let cur = v.settings.get(&s.key).cloned().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(&s.label);
                        if v.host && !v.running {
                            egui::ComboBox::from_id_salt(format!("t0-{}", s.key)).selected_text(cur.clone()).show_ui(ui, |ui| {
                                for o in &s.options {
                                    if ui.selectable_label(*o == cur, o).clicked() {
                                        cmds.push(lobby::Cmd::Setting(s.key.clone(), o.clone()));
                                    }
                                }
                            });
                        } else {
                            ui.label(RichText::new(cur).strong());
                        }
                    });
                }
            }
            if v.host && !v.running && v.players.len() >= 2 && primary_button(ui, "Start", true).clicked() {
                cmds.push(lobby::Cmd::Start);
            }
        });
        if let Some(l) = &self.lobby {
            for c in cmds {
                l.send(c);
            }
            if leave {
                l.send(lobby::Cmd::Leave);
            }
        }
        if leave || v.ended {
            if v.ended && v.error.is_some() {
                self.toast(v.error.clone().unwrap());
            }
            self.lobby = None;
        }
    }

    fn screenshots(&mut self, ctx: &egui::Context) {
        let Some(plan) = self.shots.as_mut() else { return };
        if plan.idx >= plan.steps.len() {
            std::process::exit(0);
        }
        let (name, tab, page, modal) = plan.steps[plan.idx].clone();
        self.tab = tab;
        self.page = page;
        self.first_run = modal;
        let plan = self.shots.as_mut().unwrap();
        if !plan.waiting && plan.at.elapsed() > Duration::from_millis(2500) {
            plan.waiting = true;
            let path = plan.dir.join(format!("{:02}-{name}.png", plan.idx));
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                let px: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                let _ = image::save_buffer(&path, &px, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8);
                ctx2.request_repaint();
            });
            let jobs = self.jobs.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1200));
                jobs.send(Box::new(|app: &mut App| {
                    if let Some(p) = app.shots.as_mut() {
                        p.idx += 1;
                        p.at = Instant::now();
                        p.waiting = false;
                    }
                }));
            });
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.set_ctx(ctx);
        self.apply_pending();
        // Background work every 30 minutes, visible or not.
        if self.last_bg.elapsed() > Duration::from_secs(1800) {
            self.last_bg = Instant::now();
            self.refresh_catalog();
        }
        if self.hidden || self.queue.is_some() || self.games.iter().any(|g| g.busy.is_some()) || self.lobby.is_some() {
            ctx.request_repaint_after(Duration::from_millis(if self.hidden { 30_000 } else { 100 }));
        }
        // Background mode: the window's X hides instead of quitting.
        if ctx.input(|i| i.viewport().close_requested()) && self.settings.background == Some(true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
            if !self.settings.hinted_background {
                self.settings.hinted_background = true;
                self.settings.save();
                system::notify("Warpzone keeps running", "Updates continue in the background. Turn this off in Settings → Running in the background.");
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.set_ctx(&ctx);
        self.screenshots(&ctx);
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::PageDown)) {
            self.tab = match self.tab {
                Tab::Library => Tab::Rooms,
                Tab::Rooms => Tab::Ladder,
                Tab::Ladder => Tab::Mods,
                Tab::Mods => Tab::Settings,
                t => t,
            };
        }
        egui::Panel::top("top").frame(Frame::new().fill(Color32::from_rgb(14, 14, 25)).inner_margin(Margin::symmetric(14, 10))).show(ui, |ui| self.top_bar(ui));
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(18))).show(ui, |ui| {
            if self.lobby.is_some() {
                self.lobby_view(ui);
                ui.add_space(10.0);
            }
            match (self.tab, self.page.clone()) {
                (Tab::Library, Some(id)) => self.game_page(ui, &id),
                (Tab::Library, None) => self.library(ui),
                (Tab::Rooms, _) => self.rooms_tab(ui),
                (Tab::Ladder, _) => self.ladder_tab(ui),
                (Tab::Mods, _) => self.mods_tab(ui),
                (Tab::Moderation, _) => self.moderation_tab(ui),
                (Tab::Settings, _) => self.settings_tab(ui),
            }
        });
        if !self.toasts.is_empty() {
            egui::Area::new(egui::Id::unique("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-18.0, -18.0)).show(&ctx, |ui| {
                for (t, _) in &self.toasts {
                    Frame::new()
                        .fill(Color32::from_rgb(36, 32, 70))
                        .stroke(Stroke::new(1.0, ACCENT))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.label(RichText::new(t).color(Color32::WHITE));
                        });
                    ui.add_space(6.0);
                }
            });
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if self.first_run {
            self.first_run_modal(&ctx);
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.pad.feed(raw);
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [11.0 / 255.0, 11.0 / 255.0, 20.0 / 255.0, 1.0]
    }
}
