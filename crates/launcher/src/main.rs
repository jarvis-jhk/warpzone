//! `warpzone` — the Warpzone launcher.
//!
//! ```text
//! warpzone                          open the launcher
//! warpzone warpzone://join/<room>#… open an invite link
//! warpzone --play <game>            update if needed, then start the game (Steam shortcuts use this)
//! warpzone --background             start hidden (background mode autostart)
//! warpzone --screenshot-dir <dir>   render every screen to PNGs and exit (tests, docs)
//! warpzone --render-art <dir>       write every game's generated artwork as PNGs and exit
//! warpzone --render-icon <file>     write the launcher icon (256×256 PNG) and exit
//! warpzone --add-to-steam           menu entry, warpzone:// links and a Steam shortcut, then exit
//! ```
//!
//! A second start hands its arguments to the running launcher and exits.

mod app;
mod art;
mod jobs;
mod lobby;
mod nav;
mod pad;
mod pixel;
mod settings;
mod steam;
mod system;
mod tileart;

use std::time::Instant;

fn render_art(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let api = warpzone_core::api::Api::new(&settings::Settings::load().server(), std::sync::Arc::new(warpzone_core::identity::Identity::generate()));
    let games = api.games().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    for d in games {
        for (k, n) in
            [(art::Kind::Tile, "tile"), (art::Kind::Capsule, "capsule"), (art::Kind::Hero, "hero"), (art::Kind::Logo, "logo"), (art::Kind::Icon, "icon")]
        {
            let name = d.steam.as_ref().and_then(|s| s.name.clone()).unwrap_or(d.name.clone());
            let (w, h, px) = art::render(&art::Spec { id: &d.id, name: &name, accent: &d.accent, motif: d.motif.as_deref().unwrap_or("waves") }, k);
            art::save_png(&dir.join(format!("{}-{n}.png", d.id)), w, h, &px).unwrap();
        }
        println!("{}", d.id);
    }
}

/// The launcher's own crashes go to the server as feedback for Warpzone
/// (unless the player turned crash reports off).
fn report_own_crashes() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let settings = settings::Settings::load();
        if !settings.crash_reports {
            return;
        }
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let trace = std::backtrace::Backtrace::force_capture();
        report_own(&settings, &format!("launcher panic{at}"), &format!("{msg}\n\n{trace}"));
    }));
}

fn report_own(settings: &settings::Settings, title: &str, detail: &str) {
    let api = warpzone_core::api::Api::new(&settings.server(), std::sync::Arc::new(warpzone_core::identity::Identity::generate()));
    let client = warpzone_core::feedback::client_name("warpzone-launcher", env!("CARGO_PKG_VERSION"));
    let req = warpzone_core::feedback::request("warpzone", "crash", title, detail, Some(env!("CARGO_PKG_VERSION")), &client);
    let _ = warpzone_core::feedback::send(&api, &req);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    report_own_crashes();
    if let Some(i) = args.iter().position(|a| a == "--render-art") {
        render_art(std::path::Path::new(args.get(i + 1).map(String::as_str).unwrap_or("art")));
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--render-icon") {
        // Packaging: the launcher's own icon (AppImage, .desktop files).
        let (w, h, px) = art::render(&art::Spec { id: "warpzone", name: "Warpzone", accent: "#7c5cff", motif: "waves" }, art::Kind::Icon);
        let out = args.get(i + 1).map(String::as_str).unwrap_or("warpzone.png");
        if let Err(e) = art::save_png(std::path::Path::new(out), w, h, &px) {
            eprintln!("{out}: {e}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--add-to-steam") {
        // The Steam Deck installer: menu entry, warpzone:// links and a Steam
        // shortcut with artwork (closing Steam first when it runs).
        let settings = settings::Settings::load();
        let identity = warpzone_core::identity::Identity::shared().unwrap_or_else(|_| warpzone_core::identity::Identity::generate());
        let api = std::sync::Arc::new(warpzone_core::api::Api::new(&settings.server(), std::sync::Arc::new(identity)));
        if let Err(e) = system::register_uri_handler() {
            eprintln!("desktop entry: {e}");
        }
        match app::SteamJob::warpzone(api).run() {
            Ok(n) => println!("Warpzone added to Steam ({n} user{})", if n == 1 { "" } else { "s" }),
            Err(e) => {
                eprintln!("Add to Steam: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("warpzone {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let shots = args.iter().position(|a| a == "--screenshot-dir").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    if shots.is_none() && system::forward_to_running(&args) {
        return;
    }
    let background = args.iter().any(|a| a == "--background");
    let play = args.iter().position(|a| a == "--play").and_then(|i| args.get(i + 1)).cloned();
    let (tx, rx) = std::sync::mpsc::channel();
    let icon = {
        let (w, h, px) = art::render(&art::Spec { id: "warpzone", name: "Warpzone", accent: "#7c5cff", motif: "waves" }, art::Kind::Icon);
        eframe::egui::IconData { rgba: px, width: w, height: h }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Warpzone")
            .with_app_id("warpzone")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 520.0])
            .with_icon(std::sync::Arc::new(icon))
            .with_visible(!background),
        ..Default::default()
    };
    let initial = args.clone();
    let result = eframe::run_native(
        "Warpzone",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            if shots.is_none() {
                system::listen(tx, move || ctx.request_repaint());
            }
            let mut a = app::App::new(cc, Some(rx));
            a.hidden = background;
            if let Some(g) = play {
                a.page = Some(g.clone());
                a.play_after = Some(g);
                a.exit_after_launch = true;
            }
            for x in &initial {
                if let Some((room, secret)) = warpzone_core::launch::parse_invite(x) {
                    a.open_invite(&room, secret);
                }
            }
            if let Some(dir) = shots {
                std::fs::create_dir_all(&dir).ok();
                let first = a.games.first().map(|g| g.desc.id.clone());
                a.shots = Some(app::ShotPlan {
                    dir,
                    steps: vec![
                        ("first-run".into(), app::Tab::Library, None, true),
                        ("library".into(), app::Tab::Library, None, false),
                        // The same, moved through with a controller's D-pad.
                        ("library-pad".into(), app::Tab::Library, None, false),
                        // Down to the row below the screen: it scrolls into view.
                        ("library-down".into(), app::Tab::Library, None, false),
                        // Up from the first row: the focus stays in the grid (not on the filter).
                        ("library-up".into(), app::Tab::Library, None, false),
                        ("game-page".into(), app::Tab::Library, first.or(Some("funny-furries".into())), false),
                        ("rooms".into(), app::Tab::Rooms, None, false),
                        // Rooms moved through with the D-pad (it stays on the page).
                        ("rooms-pad".into(), app::Tab::Rooms, None, false),
                        ("ladder".into(), app::Tab::Ladder, None, false),
                        ("mods".into(), app::Tab::Mods, None, false),
                        ("settings".into(), app::Tab::Settings, None, false),
                        // Controller mode only (else the feedback bar at the bottom).
                        ("feedback".into(), app::Tab::Feedback, None, false),
                        // View on the controller.
                        ("player-name".into(), app::Tab::Library, None, false),
                        // Opens "Change artwork" for the first game with SteamGridDB art.
                        ("art-picker".into(), app::Tab::Library, None, false),
                        // RT on the controller.
                        ("quit".into(), app::Tab::Library, None, false),
                    ],
                    idx: 0,
                    at: Instant::now(),
                    waiting: false,
                    scripted: None,
                });
            }
            Ok(Box::new(a))
        }),
    );
    if let Err(e) = result {
        eprintln!("warpzone: {e}");
        let settings = settings::Settings::load();
        if settings.crash_reports {
            report_own(&settings, "launcher could not open its window", &e.to_string());
        }
        std::process::exit(1);
    }
}
