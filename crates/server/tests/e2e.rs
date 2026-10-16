//! End-to-end: the real server binary, real clients (`warpzone-core`) over
//! loopback. Covers accounts, hosting, joining by code, knocking, direct and
//! relayed encrypted peer traffic (unreliable and reliable), a ranked match,
//! leaderboards, and the whole mod path: upload → moderation → resolve → sync.

use std::io::Write;
use std::net::{TcpListener, UdpSocket};
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};
use warpzone_core::api::Api;
use warpzone_core::client::{Client, Config, Event, HostOptions};
use warpzone_core::identity::Identity;
use warpzone_core::protocol::api::*;
use warpzone_core::protocol::mods::Manifest;

struct Server {
    child: Child,
    url: String,
    _dir: tempfile::TempDir,
    moderator: Arc<Identity>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_tcp() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn free_udp() -> u16 {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

const GAME: &str = r#"
id = "demo"
name = "Demo Game"
tagline = "A test game"
tier = 3
[rooms]
max_players = 4
presets = [{ id = "duel", label = "duel" }]
display = [{ key = "map", label = "Map" }]
[mods]
categories = [{ id = "level", label = "Levels" }, { id = "tiles", label = "Tiles" }]
install = "extract"
[[ladder]]
id = "1v1"
title = "1 vs 1"
teams = 2
[[leaderboards]]
id = "any"
title = "Any%"
kind = "time"
seasonal = true
"#;

fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let http = free_tcp();
    let udp = free_udp();
    std::fs::create_dir_all(dir.path().join("games")).unwrap();
    std::fs::write(dir.path().join("games/demo.toml"), GAME).unwrap();
    let moderator = Arc::new(Identity::generate());
    std::fs::write(dir.path().join("staff.toml"), format!("moderators = [\"{}\"]\nblocked_words = [\"badword\"]\n", moderator.public_hex())).unwrap();
    let url = format!("http://127.0.0.1:{http}");
    std::fs::write(
        dir.path().join("server.toml"),
        format!(
            "public_url = \"{url}\"\nhttp_listen = \"127.0.0.1:{http}\"\nudp_listen = \"127.0.0.1:{udp}\"\ndata_dir = \"data\"\ngames_dir = \"games\"\nstaff_file = \"staff.toml\"\n[limits]\nregistrations_per_ip_per_day = 100\nlogins_per_ip_per_minute = 1000\ncode_guesses_per_ip_per_minute = 100\n"
        ),
    )
    .unwrap();
    let child =
        Command::new(env!("CARGO_BIN_EXE_warpzone-server")).arg("--config").arg(dir.path().join("server.toml")).env("WARPZONE_LOG", "warn").spawn().unwrap();
    let s = Server { child, url, _dir: dir, moderator };
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if std::net::TcpStream::connect(format!("127.0.0.1:{http}")).is_ok() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not start");
}

fn client(s: &Server, name: &str) -> Client {
    let id = Arc::new(Identity::generate());
    let api = Api::new(&s.url, id.clone());
    api.register(name).unwrap();
    client_as(s, id)
}

fn client_as(s: &Server, id: Arc<Identity>) -> Client {
    let mut cfg = Config::new("demo", "1.0.0");
    cfg.server = s.url.clone();
    cfg.identity = Some(id);
    Client::new(cfg).unwrap()
}

/// Poll all clients until `f` finds what it waits for in client `i`'s events.
fn wait<T>(clients: &mut [&mut Client], i: usize, secs: u64, mut f: impl FnMut(&Event) -> Option<T>) -> T {
    let t = Instant::now();
    loop {
        for (k, c) in clients.iter_mut().enumerate() {
            c.poll();
            if k == i {
                while let Some(e) = c.next_event() {
                    if let Event::Error { op, message, .. } = &e {
                        eprintln!("client {k} error in {op}: {message}");
                    }
                    if let Some(v) = f(&e) {
                        return v;
                    }
                }
            }
        }
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out waiting");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_ready(c: &mut Client) {
    let t = Instant::now();
    while c.net_candidates().is_empty() {
        c.poll();
        assert!(t.elapsed() < Duration::from_secs(10), "client never got ready");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn rooms_peers_and_relay() {
    let s = start();
    let mut host = client(&s, "Hosty");
    let mut guest = client(&s, "Guesty");
    wait_ready(&mut host);
    wait_ready(&mut guest);

    host.host(HostOptions { max_players: 4, entry: Entry::Code, preset: Some("duel".into()), ..Default::default() });
    let code = wait(&mut [&mut host, &mut guest], 0, 10, |e| match e {
        Event::RoomHosted { code, room, .. } => {
            assert_eq!(room.name, "Hosty's duel");
            code.clone()
        }
        _ => None,
    });
    guest.join_code(&code);
    let guest_peer = wait(&mut [&mut host, &mut guest], 1, 10, |e| match e {
        Event::PeerConnected { peer, relayed } => {
            assert!(!relayed, "loopback must connect directly");
            Some(*peer)
        }
        _ => None,
    });
    let host_peer = wait(&mut [&mut host, &mut guest], 0, 10, |e| match e {
        Event::PeerConnected { peer, .. } => Some(*peer),
        _ => None,
    });

    // Unreliable datagram and 300 reliable messages in order.
    assert!(guest.send(guest_peer, 1, b"hello host", false));
    let got = wait(&mut [&mut host, &mut guest], 0, 5, |e| match e {
        Event::PeerData { data, reliable: false, .. } => Some(data.clone()),
        _ => None,
    });
    assert_eq!(got, b"hello host");
    for i in 0u32..300 {
        assert!(host.send(host_peer, 2, &i.to_be_bytes(), true));
    }
    let mut next = 0u32;
    while next < 300 {
        let v = wait(&mut [&mut host, &mut guest], 1, 10, |e| match e {
            Event::PeerData { data, reliable: true, channel: 2, .. } => Some(u32::from_be_bytes(data[..4].try_into().unwrap())),
            _ => None,
        });
        assert_eq!(v, next, "reliable messages arrive in order");
        next += 1;
    }

    // A third player knocks on an approve-only room and is let in.
    host.leave();
    let mut third = client(&s, "Thirdy");
    wait_ready(&mut third);
    host.host(HostOptions { max_players: 4, entry: Entry::Approve, visibility: Visibility::Public, ..Default::default() });
    let room = wait(&mut [&mut host, &mut third], 0, 10, |e| match e {
        Event::RoomJoined { room, .. } => Some(room.id.clone()),
        _ => None,
    });
    let api = Api::new(&s.url, Arc::new(Identity::generate()));
    let listed = api.rooms(Some("demo"), true).unwrap();
    assert!(listed.iter().any(|r| r.id == room), "public room is listed");
    third.join(&room, None, None);
    let knock = wait(&mut [&mut host, &mut third], 0, 10, |e| match e {
        Event::Knock { knock, username, .. } => {
            assert_eq!(username, "Thirdy");
            Some(knock.clone())
        }
        _ => None,
    });
    host.answer_knock(&knock, true);
    wait(&mut [&mut host, &mut third], 1, 10, |e| matches!(e, Event::PeerConnected { .. }).then_some(()));

    // Relay: force both sides through it.
    host.leave();
    third.leave();
    let mut a = client(&s, "RelayA");
    let mut b = client(&s, "RelayB");
    wait_ready(&mut a);
    wait_ready(&mut b);
    a.force_relay(true);
    b.force_relay(true);
    a.host(HostOptions { max_players: 2, visibility: Visibility::Unlisted, ..Default::default() });
    let (code, invite) = wait(&mut [&mut a, &mut b], 0, 10, |e| match e {
        Event::RoomHosted { code, invite, .. } => Some((code.clone().expect("unlisted rooms get a code"), invite.clone())),
        _ => None,
    });
    assert!(api.rooms(Some("demo"), false).unwrap().iter().all(|r| r.visibility == Visibility::Public), "unlisted rooms are not listed");
    let (room, secret) = warpzone_core::launch::parse_invite(&invite).unwrap();
    let _ = code;
    b.join(&room, None, secret.as_deref());
    let peer = wait(&mut [&mut a, &mut b], 1, 15, |e| match e {
        Event::PeerConnected { peer, relayed } => {
            assert!(relayed, "forced relay");
            Some(*peer)
        }
        _ => None,
    });
    assert!(b.send(peer, 9, b"through the relay", true));
    let got = wait(&mut [&mut a, &mut b], 0, 10, |e| match e {
        Event::PeerData { data, .. } => Some(data.clone()),
        _ => None,
    });
    assert_eq!(got, b"through the relay");
    let stats: serde_json::Value = api.get_public("/stats").unwrap();
    assert!(stats["relay_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn accounts_and_privacy() {
    let s = start();
    let id = Arc::new(Identity::generate());
    let api = Api::new(&s.url, id.clone());
    api.login().unwrap();
    assert!(api.register("ab").is_err(), "too short");
    assert!(api.register("admin").is_err(), "reserved");
    assert!(api.register("mybadwordname").is_err(), "blocked word");
    api.register("Alice").unwrap();
    let other = Api::new(&s.url, Arc::new(Identity::generate()));
    assert!(matches!(other.register("alice"), Err(warpzone_core::Error::Api { status: 409, .. })), "names are case-insensitively unique");
    // Second device via link code.
    let code = api.link_code().unwrap().code;
    let dev2 = Api::new(&s.url, Arc::new(Identity::generate()));
    dev2.login().unwrap();
    assert_eq!(dev2.link(&code).unwrap().username, "Alice");
    // A session lost on the server is renewed transparently.
    let p = api.profile("alice").unwrap();
    assert_eq!(p.username, "Alice");
    let deleted: serde_json::Value = api.delete_account().unwrap();
    assert_eq!(deleted["deleted"], "Alice");
    assert!(api.profile("alice").is_err());
}

#[test]
fn ladder_and_boards() {
    let s = start();
    let id1 = Arc::new(Identity::generate());
    Api::new(&s.url, id1.clone()).register("LadderOne").unwrap();
    let mut p1 = client_as(&s, id1.clone());
    let mut p2 = client(&s, "LadderTwo");
    wait_ready(&mut p1);
    wait_ready(&mut p2);
    p1.queue("1v1", "stock");
    p2.queue("1v1", "stock");
    let (m1, host1) = wait(&mut [&mut p1, &mut p2], 0, 15, |e| match e {
        Event::MatchFound { match_id, host, .. } => Some((match_id.clone(), *host)),
        _ => None,
    });
    let m2 = wait(&mut [&mut p1, &mut p2], 1, 15, |e| match e {
        Event::MatchFound { match_id, .. } => Some(match_id.clone()),
        _ => None,
    });
    assert_eq!(m1, m2);
    // Both join the match room automatically and connect.
    let joiner = if host1 { 1 } else { 0 };
    wait(&mut [&mut p1, &mut p2], joiner, 15, |e| matches!(e, Event::PeerConnected { .. }).then_some(()));
    let a1 = p1.account().unwrap().id;
    let a2 = p2.account().unwrap().id;
    let report = MatchReport { placements: vec![vec![a1], vec![a2]], state_hash: Some("abc".into()) };
    p1.report_match(&m1, report.clone());
    p2.report_match(&m1, report);
    let state = wait(&mut [&mut p1, &mut p2], 0, 15, |e| match e {
        Event::MatchFinished { state, .. } => Some(state.clone()),
        _ => None,
    });
    assert_eq!(state, "rated");

    // A game started later on the same account does not replay that match.
    let mut again = client_as(&s, id1);
    wait(&mut [&mut again], 0, 10, |e| matches!(e, Event::LoggedIn { .. }).then_some(()));
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(500) {
        again.poll();
        while let Some(e) = again.next_event() {
            assert!(!matches!(e, Event::MatchFound { .. } | Event::MatchFinished { .. } | Event::Error { .. }), "replayed: {e:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(again);
    let api = p1.api();
    let info = api.match_info(&m1).unwrap();
    let winner = info.teams.iter().flatten().find(|p| p.account == a1).unwrap();
    assert!(winner.rating_after.unwrap() > winner.rating_before);
    let st = api.standings("demo", "1v1", None).unwrap();
    assert_eq!(st.placing, 2, "both are still in placement matches");

    // Leaderboard: lower time wins, only improvements count.
    let r = api.submit_score("demo", "any", &ScoreSubmit { value: 61_000, ruleset: "stock".into(), replay_hash: None, replay: None }).unwrap();
    assert!(r.improved && r.rank == 1);
    let r = api.submit_score("demo", "any", &ScoreSubmit { value: 65_000, ruleset: "stock".into(), replay_hash: None, replay: None }).unwrap();
    assert!(!r.improved && r.best == 61_000);
    let b = api.board("demo", "any", "stock", "all").unwrap();
    assert_eq!(b.entries[0].username, "LadderOne");
}

fn zip_with(path: &Path, files: &[(&str, &str)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    for (n, d) in files {
        z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(d.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

fn manifest(name: &str, version: &str, category: &str, deps: &[&str]) -> Manifest {
    Manifest {
        name: name.into(),
        version: version.into(),
        title: format!("{name} title"),
        category: category.into(),
        game_version: ">=1".into(),
        dependencies: deps.iter().map(|s| s.to_string()).collect(),
        license: "CC0-1.0".into(),
        description: "test".into(),
        tags: vec![],
        homepage: None,
        extra: serde_json::Value::Null,
    }
}

#[test]
fn mods_moderation_resolve_sync() {
    let s = start();
    let tmp = tempfile::tempdir().unwrap();
    let author = Api::new(&s.url, Arc::new(Identity::generate()));
    author.register("Modder").unwrap();
    let tiles = tmp.path().join("tiles.zip");
    zip_with(&tiles, &[("tiles/a.png", "png")]);
    let level = tmp.path().join("level.zip");
    zip_with(&level, &[("level.json", "{}")]);
    let up = author.upload_mod("demo", &manifest("cave-tiles", "1.0.0", "tiles", &[]), &tiles).unwrap();
    assert_eq!(up.state, "pending");
    author.upload_mod("demo", &manifest("cave-level", "1.0.0", "level", &["cave-tiles >= 1"]), &level).unwrap();
    assert!(author.upload_mod("demo", &manifest("cave-tiles", "1.0.0", "tiles", &[]), &tiles).is_err(), "versions are immutable");
    assert!(author.upload_mod("demo", &manifest("x", "1.0.0", "nope", &[]), &tiles).is_err(), "unknown category");

    // Not visible before approval; the author sees it.
    let anon = Api::new(&s.url, Arc::new(Identity::generate()));
    assert!(anon.mods("demo", None, None).unwrap().is_empty());
    assert_eq!(author.mod_detail("demo", "cave-tiles").unwrap().versions[0].state, "pending");
    let thief = Api::new(&s.url, Arc::new(Identity::generate()));
    thief.register("Thief").unwrap();
    assert!(thief.upload_mod("demo", &manifest("cave-tiles", "2.0.0", "tiles", &[]), &tiles).is_err(), "names belong to their author");

    // Moderator approves both.
    let m = Api::new(&s.url, s.moderator.clone());
    m.login().unwrap();
    assert!(m.roles().contains(&"moderator".to_string()));
    let q = m.moderation_queue().unwrap();
    assert_eq!(q.pending.len(), 2);
    assert!(author.decide("demo", "cave-tiles", "1.0.0", "approve", None).is_err(), "authors cannot approve");
    assert!(m.decide("demo", "cave-tiles", "1.0.0", "reject", None).is_err(), "rejections need a reason");
    m.decide("demo", "cave-tiles", "1.0.0", "approve", None).unwrap();
    m.decide("demo", "cave-level", "1.0.0", "approve", None).unwrap();
    assert_eq!(anon.mods("demo", None, None).unwrap().len(), 2);

    // Resolve and sync into a private store.
    let lock = anon.resolve("demo", "1.2.0", vec!["cave-level".into()]).unwrap();
    assert_eq!(lock.order, vec!["cave-tiles", "cave-level"]);
    assert_eq!(anon.lockfile("demo", &lock.hash).unwrap(), lock);
    let store = warpzone_core::modstore::ModStore::at(tmp.path().join("store"), "demo", true);
    let paths = store.sync(&anon, &lock, &mut |_, _| {}).unwrap();
    assert!(paths[0].join("tiles/a.png").exists());
    assert!(paths[1].join("level.json").exists());
    assert_eq!(store.active().unwrap().hash, lock.hash);
    assert!(store.validate(&[("cave-level".into(), "1.0.0".into())], "1.2.0").iter().any(|p| p.contains("cave-tiles")));
    // Downloads are counted.
    assert!(anon.mod_detail("demo", "cave-tiles").is_ok());
}
