//! `warpzone` — the Warpzone launcher.
//!
//! ```text
//! warpzone                          open the launcher
//! warpzone warpzone://join/<room>#… open an invite link
//! warpzone --play <game>            update if needed, then start the game (Steam shortcuts use this)
//! warpzone --background             start hidden (background mode autostart)
//! warpzone --screenshot-dir <dir>   render every screen to PNGs and exit (tests, docs)
//! warpzone --render-art <dir>       write every game's generated artwork as PNGs and exit
//! ```
//!
//! A second start hands its arguments to the running launcher and exits.

mod app;
mod art;
mod jobs;
mod lobby;
mod pad;
mod settings;
mod steam;
mod system;

use std::time::Instant;

fn render_art(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let api = warpzone_core::api::Api::new(&settings::Settings::load().server(), std::sync::Arc::new(warpzone_core::identity::Identity::generate()));
    let games = api.games().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    for d in games {
        for (k, n) in [(art::Kind::Tile, "tile"), (art::Kind::Capsule, "capsule"), (art::Kind::Hero, "hero"), (art::Kind::Logo, "logo"), (art::Kind::Icon, "icon")] {
            let name = d.steam.as_ref().and_then(|s| s.name.clone()).unwrap_or(d.name.clone());
            let (w, h, px) = art::render(&art::Spec { id: &d.id, name: &name, accent: &d.accent, motif: d.motif.as_deref().unwrap_or("waves") }, k);
            art::save_png(&dir.join(format!("{}-{n}.png", d.id)), w, h, &px).unwrap();
        }
        println!("{}", d.id);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--render-art") {
        render_art(std::path::Path::new(args.get(i + 1).map(String::as_str).unwrap_or("art")));
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
                        ("game-page".into(), app::Tab::Library, first.or(Some("super-mario-land-port".into())), false),
                        ("rooms".into(), app::Tab::Rooms, None, false),
                        ("ladder".into(), app::Tab::Ladder, None, false),
                        ("mods".into(), app::Tab::Mods, None, false),
                        ("settings".into(), app::Tab::Settings, None, false),
                    ],
                    idx: 0,
                    at: Instant::now(),
                    waiting: false,
                });
            }
            Ok(Box::new(a))
        }),
    );
    if let Err(e) = result {
        eprintln!("warpzone: {e}");
        std::process::exit(1);
    }
}
