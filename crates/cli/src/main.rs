//! `wz` — the Warpzone command-line tool. `wz help` lists every command;
//! `docs/cli.md` explains them with examples.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use warpzone_core::api::Api;
use warpzone_core::client::{Client, Config, Event, HostOptions};
use warpzone_core::identity::Identity;
use warpzone_core::modstore::ModStore;
use warpzone_core::{install, paths};
use warpzone_protocol::api::*;
use warpzone_protocol::mods::Manifest;

const HELP: &str = "wz — Warpzone command-line tool

USAGE: wz [--server URL] <command> [args]

Account (one key per device, no password):
  whoami                         show this device's key, account and roles
  register <username>            claim a username for this device
  recovery-code                  print the code that restores this identity (keep it secret!)
  restore <code>                 replace this device's identity with a recovered one
  link-code                      get a code to add another device to your account
  link <code>                    add this device to the account that made the code
  delete-account                 delete the account, ratings and scores

Games:
  games                          list games on the server
  rooms [game]                   list public rooms
  updates <game> <version>       is there something newer, and what changed
  changelog <game>               release notes
  install <game>                 install or update a game (verified against GitHub)
  installed                      games installed here
  launch <game> [args...]        start an installed game
  find-data <game> [folder...]   look for the original game data this game needs
  rollback <game>                go back to the previously installed version
  uninstall <game>               remove an installed game

Ladder & leaderboards:
  standings <game> <queue> [season]
  board <game> <board> [ruleset] [season]

Mods:
  mods <game> [search]           list published mods
  mod <game> <name>              details and versions
  mod-new <dir>                  write a warpzone-mod.json template into <dir>
  mod-upload <game> <manifest.json> <archive>
                                 submit a version (a moderator reviews it)
  resolve <game> <game-version> <mod>...
                                 compute a lockfile (`mod` or `\"mod >= 1.2\"`)
  mod-sync <game> <lockfile-hash>
                                 install and activate a mod set
  mod-installed <game>           mods installed here

Moderation (moderator/reviewer keys):
  queue                          pending versions and open reports
  approve <game> <name> <version>
  reject <game> <name> <version> <reason>
  hide|unhide <game> <name> <version>
  note <game> <name> <version> <text>
  close-report <id>
  rename-user <old> <new>

Network diagnostics:
  netcheck                       show this machine's addresses as the server sees them
  host <game> [--relay]          host a test room, print its code, echo peers' messages
  join <game> <code> [--relay]   join a test room and measure the connection
";

fn api(server: &str) -> Api {
    let id = Identity::shared().unwrap_or_else(|e| die(&format!("identity: {e}")));
    Api::new(server, Arc::new(id))
}

fn die(m: &str) -> ! {
    eprintln!("wz: {m}");
    std::process::exit(1)
}

fn ok<T>(r: warpzone_core::Result<T>) -> T {
    r.unwrap_or_else(|e| die(&e.to_string()))
}

fn print<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

fn arg<'a>(a: &'a [String], i: usize, what: &str) -> &'a str {
    a.get(i).map(String::as_str).unwrap_or_else(|| die(&format!("missing {what}; see `wz help`")))
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut server = warpzone_core::server_url();
    if let Some(i) = args.iter().position(|a| a == "--server") {
        server = args.get(i + 1).cloned().unwrap_or_else(|| die("--server needs a URL"));
        args.drain(i..i + 2);
    }
    let cmd = args.first().cloned().unwrap_or_else(|| "help".into());
    let a = &args;
    match cmd.as_str() {
        "help" | "-h" | "--help" => print!("{HELP}"),
        "--version" | "version" => println!("wz {}", env!("CARGO_PKG_VERSION")),
        "whoami" => {
            let api = api(&server);
            let l = ok(api.login());
            println!("server   {server}");
            println!("home     {}", paths::home().display());
            println!("key      {}", api.identity().public_hex());
            match l.account {
                Some(acc) => println!("account  {} (id {})", acc.username, acc.id),
                None => println!("account  none yet — `wz register <username>`"),
            }
            if !l.roles.is_empty() {
                println!("roles    {}", l.roles.join(", "));
            }
        }
        "register" => print(&ok(api(&server).register(arg(a, 1, "username")))),
        "recovery-code" => {
            let id = ok(Identity::shared().map_err(Into::into));
            eprintln!("Anyone with this code can play as you. Store it like a password.");
            println!("{}", id.recovery_code());
        }
        "restore" => {
            let code = a[1..].join(" ");
            let id = Identity::from_recovery_code(&code).unwrap_or_else(|| die("that is not a valid recovery code"));
            let p = paths::identity_file();
            if p.exists() {
                let backup = p.with_extension(format!("replaced-{}", warpzone_protocol::now()));
                ok(std::fs::rename(&p, &backup).map_err(Into::into));
                eprintln!("previous identity kept as {}", backup.display());
            }
            ok(id.save(&p).map_err(Into::into));
            println!("restored identity {}", id.public_hex());
        }
        "link-code" => {
            let c = ok(api(&server).link_code());
            println!("{}  (valid {} minutes; run `wz link {}` on the other device)", c.code, c.expires_in / 60, c.code);
        }
        "link" => print(&ok(api(&server).link(arg(a, 1, "code")))),
        "delete-account" => print(&ok(api(&server).delete_account())),
        "games" => {
            for g in ok(api(&server).games()) {
                println!("{:<24} tier {}  {}", g.id, g.tier, g.tagline);
            }
        }
        "rooms" => {
            for r in ok(api(&server).rooms(a.get(1).map(String::as_str), false)) {
                println!("{}  {:<18} {:<28} {}/{}  {:?}/{:?}", r.id, r.game, r.name, r.players, r.max_players, r.entry, r.audience.skill);
            }
        }
        "updates" => print(&ok(api(&server).updates(arg(a, 1, "game"), arg(a, 2, "version")))),
        "changelog" => {
            for e in ok(api(&server).changelog(arg(a, 1, "game"))) {
                println!("## {} {} ({})\n{}\n", e.version, if e.prerelease { "[preview]" } else { "" }, &e.published.get(..10).unwrap_or(""), e.notes.trim());
            }
        }
        "install" => {
            let api = api(&server);
            let game = arg(a, 1, "game");
            let desc = ok(api.game(game));
            let rel = ok(api.release(game));
            if let Some(r) = &desc.requires {
                eprintln!("note: {} needs {r}", desc.name);
            }
            let started = Instant::now();
            let i = ok(install::install(&api, &desc, &rel, &mut |n| {
                if started.elapsed().as_millis() % 500 < 5 {
                    eprint!("\r{:.1} MB", n as f64 / 1_048_576.0);
                }
            }));
            eprintln!();
            println!("installed {} {} ({}) in {}, verified by {}", desc.name, i.version, i.platform, i.dir.display(), i.verified_by);
        }
        "installed" => {
            if let Ok(rd) = std::fs::read_dir(paths::games_dir()) {
                for e in rd.flatten() {
                    if let Some(i) = install::installed(&e.file_name().to_string_lossy()) {
                        println!("{:<24} {:<10} {}", i.game, i.version, i.dir.display());
                    }
                }
            }
        }
        "launch" => {
            let api = api(&server);
            let game = arg(a, 1, "game");
            let desc = ok(api.game(game));
            let i = install::installed(game).unwrap_or_else(|| die("not installed here; `wz install` first"));
            let mut child = ok(install::launch(&desc, &i, &a[2..]));
            let _ = child.wait();
        }
        "find-data" => {
            let desc = ok(api(&server).game(arg(a, 1, "game")));
            let inst = install::installed(&desc.id);
            if desc.data.is_empty() {
                println!("{} needs no extra game data", desc.name);
            }
            let extra: Vec<std::path::PathBuf> = a[2..].iter().map(std::path::PathBuf::from).collect();
            for r in &desc.data {
                let present = warpzone_core::gamedata::present(r, inst.as_ref().map(|i| i.dir.as_path()));
                println!("{} — {}", r.label, if present { "already in place" } else { "needed" });
                let found = warpzone_core::gamedata::find(r, &extra);
                for f in &found {
                    let c = warpzone_core::gamedata::check(r, &f.path);
                    println!("  found ({}) {}  [{:?}]", f.source, f.path.display(), c);
                }
                if found.is_empty() && !present {
                    println!("  not found. Looked in Steam, GOG/Heroic/Lutris/Wine, Downloads and discs.");
                    for l in r.free.iter() {
                        println!("  free: {} {}", l.label, l.url);
                    }
                    for l in r.buy.iter() {
                        println!("  buy:  {} {}", l.label, l.url);
                    }
                }
            }
        }
        "rollback" => {
            let i = ok(install::rollback(arg(a, 1, "game")));
            println!("now on {} {}", i.game, i.version);
        }
        "uninstall" => {
            ok(install::uninstall(arg(a, 1, "game")));
            println!("removed");
        }
        "standings" => {
            let s = ok(api(&server).standings(arg(a, 1, "game"), arg(a, 2, "queue"), a.get(3).map(String::as_str)));
            println!("season {} (current {}), {} placing", s.season, s.current_season, s.placing);
            for r in s.standings {
                println!("{:>4}. {:<16} {:>5}  {} games, {} wins", r.rank, r.username, r.rating, r.games, r.wins);
            }
        }
        "board" => {
            let b = ok(api(&server).board(
                arg(a, 1, "game"),
                arg(a, 2, "board"),
                a.get(3).map(String::as_str).unwrap_or("stock"),
                a.get(4).map(String::as_str).unwrap_or("all"),
            ));
            println!("{} ({}, {})", b.title, b.ruleset, b.season);
            for e in b.entries {
                println!("{:>4}. {:<16} {}", e.rank, e.username, e.value);
            }
        }
        "mods" => {
            for m in ok(api(&server).mods(arg(a, 1, "game"), None, a.get(2).map(String::as_str))) {
                println!("{:<24} {:<10} {:<12} {:>6} dl  by {}", m.name, m.latest, m.category, m.downloads, m.author);
            }
        }
        "mod" => print(&ok(api(&server).mod_detail(arg(a, 1, "game"), arg(a, 2, "name")))),
        "mod-new" => {
            let dir = Path::new(arg(a, 1, "directory"));
            let m = Manifest {
                name: "my-mod".into(),
                version: "1.0.0".into(),
                title: "My mod".into(),
                category: "CHANGE-ME".into(),
                game_version: "*".into(),
                dependencies: vec![],
                license: "CC-BY-4.0".into(),
                description: "What it does.".into(),
                tags: vec![],
                homepage: None,
                extra: serde_json::Value::Null,
            };
            let p = dir.join("warpzone-mod.json");
            ok(std::fs::write(&p, serde_json::to_vec_pretty(&m).unwrap()).map_err(Into::into));
            println!("wrote {} — set the category to one of the game's (`wz game`), then `wz mod-upload`", p.display());
        }
        "mod-upload" => {
            let game = arg(a, 1, "game");
            let m: Manifest = serde_json::from_slice(&ok(std::fs::read(arg(a, 2, "manifest")).map_err(Into::into)))
                .unwrap_or_else(|e| die(&format!("manifest: {e}")));
            if let Err(e) = m.check() {
                die(&e.to_string());
            }
            let r = ok(api(&server).upload_mod(game, &m, Path::new(arg(a, 3, "archive"))));
            println!("{} {} uploaded ({}): a moderator reviews it before it is public", r.name, r.version, r.state);
        }
        "resolve" => print(&ok(api(&server).resolve(arg(a, 1, "game"), arg(a, 2, "game version"), a[3..].to_vec()))),
        "mod-sync" => {
            let api = api(&server);
            let game = arg(a, 1, "game");
            let lock = ok(api.lockfile(game, arg(a, 2, "lockfile hash")));
            let extract = api.game(game).ok().and_then(|g| g.mods).is_some_and(|m| m.install == "extract");
            let paths = ok(ModStore::new(game, extract).sync(&api, &lock, &mut |d, t| eprint!("\r{d}/{t} bytes")));
            eprintln!();
            for p in paths {
                println!("{}", p.display());
            }
        }
        "mod-installed" => {
            let s = ModStore::new(arg(a, 1, "game"), false);
            let active = s.active();
            for m in s.installed() {
                let on = active.as_ref().is_some_and(|l| l.mods.iter().any(|e| e.name == m.name && e.version == m.version));
                println!("{} {:<24} {}", if on { "*" } else { " " }, m.name, m.version);
            }
        }
        "queue" => {
            let q = ok(api(&server).moderation_queue());
            for p in &q.pending {
                println!(
                    "PENDING {}/{} {}  \"{}\" [{}] by {}, {} KiB{}",
                    p.game,
                    p.name,
                    p.version,
                    p.title,
                    p.category,
                    p.author,
                    p.size / 1024,
                    if p.contains_code { ", may contain code" } else { "" }
                );
                for n in &p.notes {
                    println!("    note ({}): {}", n.role, n.text.lines().next().unwrap_or(""));
                }
                println!("    download: {server}{}", p.download);
            }
            for r in &q.reports {
                println!("REPORT #{} {} {} {} — {} by {}{}", r.id, r.target, r.game.as_deref().unwrap_or(""), r.name, r.reason, r.reporter, r.detail.as_ref().map(|d| format!(": {d}")).unwrap_or_default());
            }
            if q.pending.is_empty() && q.reports.is_empty() {
                println!("nothing waiting");
            }
        }
        "approve" | "hide" | "unhide" => {
            print(&ok(api(&server).decide(arg(a, 1, "game"), arg(a, 2, "name"), arg(a, 3, "version"), &cmd, None)));
        }
        "reject" => {
            let reason = arg(a, 4, &format!("reason ({})", REJECT_REASONS.join(", ")));
            print(&ok(api(&server).decide(arg(a, 1, "game"), arg(a, 2, "name"), arg(a, 3, "version"), "reject", Some(reason))));
        }
        "note" => print(&ok(api(&server).note(arg(a, 1, "game"), arg(a, 2, "name"), arg(a, 3, "version"), &a[4..].join(" ")))),
        "close-report" => print(&ok(api(&server).close_report(arg(a, 1, "id").parse().unwrap_or_else(|_| die("id is a number"))))),
        "rename-user" => {
            let v: serde_json::Value = ok(api(&server).post(&format!("/moderation/accounts/{}/rename", arg(a, 1, "old")), &serde_json::json!({ "username": arg(a, 2, "new") })));
            print(&v);
        }
        "netcheck" => netcheck(&server),
        "host" => net_test(&server, arg(a, 1, "game"), None, a.iter().any(|x| x == "--relay")),
        "join" => net_test(&server, arg(a, 1, "game"), Some(arg(a, 2, "code").to_string()), a.iter().any(|x| x == "--relay")),
        other => die(&format!("unknown command {other}; see `wz help`")),
    }
}

fn netcheck(server: &str) {
    let api = api(server);
    let info = ok(api.info());
    println!("server {} ({}), UDP {}", info.name, info.version, info.udp.join(", "));
    let mut net = warpzone_core::net::Net::new(Arc::new(ok(Identity::shared().map_err(Into::into))), &info.udp, &info.server_key)
        .unwrap_or_else(|e| die(&format!("udp: {e}")));
    let t = Instant::now();
    while !net.stun_done() && t.elapsed() < Duration::from_secs(4) {
        net.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("local UDP port {}", net.local_port());
    let c = net.candidates();
    if c.is_empty() {
        println!("no candidates: UDP to the server seems blocked — games will not connect");
    }
    for a in c {
        println!("candidate {a}");
    }
    if !net.stun_done() {
        println!("(not every address family answered; IPv6 may be unavailable here)");
    }
}

fn net_test(server: &str, game: &str, code: Option<String>, relay: bool) {
    let mut cfg = Config::new(game, "0.0.0-wz-test");
    cfg.server = server.into();
    let mut c = ok(Client::new(cfg));
    let t = Instant::now();
    while c.net_candidates().is_empty() && t.elapsed() < Duration::from_secs(10) {
        c.poll();
        while let Some(e) = c.next_event() {
            if let Event::Error { op, message, .. } = e {
                die(&format!("{op}: {message}"));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if c.account().is_none() {
        die("this device has no username yet: `wz register <name>`");
    }
    c.force_relay(relay);
    match &code {
        None => c.host(HostOptions { max_players: 8, visibility: Visibility::Unlisted, ..Default::default() }),
        Some(code) => c.join_code(code),
    }
    let mut last_ping = Instant::now();
    let mut seq = 0u32;
    loop {
        c.poll();
        while let Some(e) = c.next_event() {
            match e {
                Event::RoomHosted { code, invite, .. } => println!("hosting — join with: wz join {game} {}\ninvite: {invite}", code.unwrap_or_default()),
                Event::RoomJoined { room, .. } => println!("in room {} ({})", room.id, room.name),
                Event::PeerJoined { username, .. } => println!("{username} joined, connecting…"),
                Event::PeerConnected { peer, relayed } => {
                    println!("connected to {} — {}", c.peer_name(peer).unwrap_or_default(), if relayed { "via relay" } else { "direct" })
                }
                Event::PeerPathChanged { peer, relayed } => println!("{}: now {}", c.peer_name(peer).unwrap_or_default(), if relayed { "relayed" } else { "direct" }),
                Event::PeerData { peer, data, .. } => {
                    if code.is_none() {
                        c.send(peer, 0, &data, false); // echo
                    } else if data.len() >= 4 {
                        let rtt = c.peer_rtt(peer).map(|d| d.as_millis()).unwrap_or(0);
                        println!("echo #{} from host, rtt ≈ {rtt} ms, {}", u32::from_be_bytes(data[..4].try_into().unwrap()), if c.peer_relayed(peer) == Some(true) { "relayed" } else { "direct" });
                    }
                }
                Event::PeerLeft { account, .. } => println!("peer {account} left"),
                Event::RoomClosed { .. } => die("room closed"),
                Event::Error { op, message, .. } => die(&format!("{op}: {message}")),
                _ => {}
            }
        }
        if code.is_some() && last_ping.elapsed() > Duration::from_secs(1) {
            last_ping = Instant::now();
            seq += 1;
            c.broadcast(0, &seq.to_be_bytes(), false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
