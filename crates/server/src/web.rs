//! The website: start page with the game list and downloads, a page per game
//! (rooms, ladder, leaderboards, mods) and per mod, and the landing page of
//! invite links. Server-rendered HTML; the only script is on the invite page.

use crate::error::ApiError;
use crate::rooms::{RoomQuery, list_public};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use std::fmt::Write;
use warpzone_protocol::api::{ReleaseInfo, RoomInfo, Skill, Style};
use warpzone_protocol::descriptor::GameDescriptor;

const CSS: &str = include_str!("../../../site/style.css");
const LOGO: &str = include_str!("../../../site/logo.svg");

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

const PLATFORMS: &[(&str, &str)] =
    &[("appimage", "Linux AppImage"), ("linux", "Linux"), ("windows", "Windows"), ("deck", "Steam Deck"), ("portmaster", "PortMaster")];

fn platform_label(p: &str) -> &str {
    PLATFORMS.iter().find(|(k, _)| *k == p).map(|(_, v)| *v).unwrap_or(p)
}

fn mb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 { format!("{:.1} MB", bytes as f64 / 1_048_576.0) } else { format!("{} KB", bytes.div_ceil(1024)) }
}

fn date(iso_or_unix: &str) -> String {
    iso_or_unix.get(..10).unwrap_or(iso_or_unix).to_string()
}

fn unix_date(t: i64) -> String {
    let days = t.div_euclid(86_400);
    // Same civil conversion as the ladder seasons.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y}-{m:02}-{d:02}")
}

fn page(state: &AppState, title: &str, description: &str, body: &str) -> Html<String> {
    let cfg = &state.cfg;
    let legal = cfg.legal_url.as_ref().map(|u| format!(r#"<a href="{}">Legal &amp; contact</a>"#, esc(u))).unwrap_or_default();
    let src = esc(&cfg.source_url);
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:title" content="{title}"><meta property="og:description" content="{description}">
<link rel="icon" href="/static/logo.svg" type="image/svg+xml">
<link rel="stylesheet" href="/static/style.css">
</head><body>
<div class="wrap">
<header class="top">
  <a class="brand" href="/"><img src="/static/logo.svg" alt="">{name}</a>
  <nav><a href="/#games">Games</a><a href="/#rooms">Open rooms</a><a href="/#launcher">Launcher</a><a href="{src}/blob/main/docs/README.md">Docs</a><a href="{src}/blob/main/docs/self-hosting.md">Host your own</a><a href="{src}">Source</a></nav>
</header>
{body}
</div>
<footer><div class="wrap">
  <span>{name} is open source (MIT or Apache-2.0).</span>
  <a href="{src}">Source</a><a href="{src}/blob/main/docs/api.md">API</a><a href="{src}/blob/main/docs/integration.md">Add your game</a><a href="{src}/blob/main/docs/self-hosting.md">Host your own ecosystem</a>
  <span class="right">{legal}</span>
</div></footer>
</body></html>"#,
        title = esc(title),
        description = esc(description),
        name = esc(&cfg.name),
    ))
}

/// Generated tile artwork next to the descriptors (`games/art/<id>.jpg`,
/// written by `warpzone --render-art`), if there is no `art` URL.
fn art_url(state: &AppState, g: &GameDescriptor) -> Option<String> {
    g.art.clone().or_else(|| state.cfg.games_dir.join("art").join(format!("{}.jpg", g.id)).is_file().then(|| format!("/art/{}.jpg", g.id)))
}

fn banner(state: &AppState, g: &GameDescriptor) -> String {
    let initials: String = g.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    let art = art_url(state, g);
    let img = art.as_ref().map(|a| format!(r#"<img src="{}" alt="">"#, esc(a))).unwrap_or_default();
    format!(
        r#"<div class="banner" style="background:linear-gradient(135deg,{acc}cc,{acc}22 70%),#151526">{img}<span class="initials">{ini}</span></div>"#,
        acc = esc(&g.accent),
        img = img,
        ini = if art.is_some() { String::new() } else { esc(&initials) }
    )
}

fn badges(g: &GameDescriptor, rooms: usize) -> String {
    let mut b = String::new();
    if let Some(s) = &g.status {
        let _ = write!(b, r#"<span class="badge warn">{}</span>"#, esc(s));
    }
    // Only what a game can do today: tier 2+ games use rooms, ladder, boards
    // and mods themselves; tier 0 games only through the launcher.
    if g.tier >= 2 {
        for f in g.features() {
            let _ = write!(b, r#"<span class="badge on">{f}</span>"#);
        }
    } else if g.tier0.is_some() {
        b.push_str(r#"<span class="badge on">launcher lobby</span>"#);
    } else {
        b.push_str(r#"<span class="badge">Warpzone support coming</span>"#);
    }
    if g.third_party {
        b.push_str(r#"<span class="badge">third-party</span>"#);
    }
    if rooms > 0 {
        let _ = write!(b, r#"<span class="badge live">{rooms} open room{}</span>"#, if rooms == 1 { "" } else { "s" });
    }
    format!(r#"<div class="badges">{b}</div>"#)
}

fn downloads(rel: Option<&ReleaseInfo>, homepage: Option<&str>) -> String {
    let Some(r) = rel.filter(|r| !r.assets.is_empty()) else {
        return match homepage {
            Some(h) => format!(r#"<div class="dl"><a class="btn small" href="{}">No release yet · project page</a></div>"#, esc(h)),
            None => r#"<div class="dl"><span class="btn small disabled">No release yet</span></div>"#.into(),
        };
    };
    let mut s = String::from(r#"<div class="dl">"#);
    for (p, _) in PLATFORMS {
        if let Some(a) = r.assets.get(*p) {
            let _ = write!(
                s,
                r#"<a class="btn small" href="{}" title="{}">{} <span class="sub">{}</span></a>"#,
                esc(&a.url),
                esc(&a.name),
                platform_label(p),
                mb(a.size)
            );
        }
    }
    let _ = write!(
        s,
        r#"</div><div class="ver">{} {}{} · <a href="{}">release notes</a></div>"#,
        esc(&r.version),
        if r.prerelease { "preview · " } else { "· " },
        date(&r.published),
        esc(&r.page)
    );
    s
}

fn audience(r: &RoomInfo) -> String {
    let langs = if r.audience.languages.is_empty() { "any language".to_string() } else { r.audience.languages.join(", ").to_uppercase() };
    let skill = match r.audience.skill {
        Skill::Beginners => "beginners welcome",
        Skill::Mixed => "mixed skill",
        Skill::Experienced => "experienced",
    };
    let style = match r.audience.style {
        Style::Casual => "casual",
        Style::Serious => "serious",
        Style::Learning => "learning",
    };
    format!("{langs} · {skill} · {style}")
}

fn rooms_table(state: &AppState, rooms: &[RoomInfo], show_game: bool) -> String {
    if rooms.is_empty() {
        return r#"<p class="muted">No open rooms right now. Host one from a game or the launcher.</p>"#.into();
    }
    let games = state.games.read().unwrap();
    let mut s = String::from("<div class=\"panel\" style=\"padding:4px 8px\"><table><tr>");
    if show_game {
        s.push_str("<th>Game</th>");
    }
    s.push_str("<th>Room</th><th>Players</th><th>For</th><th>Details</th><th></th></tr>");
    for r in rooms {
        let g = games.by_id.get(&r.game);
        let details: Vec<String> = g
            .and_then(|g| g.rooms.as_ref())
            .map(|rc| rc.display.iter().filter_map(|f| r.metadata.get(&f.key).map(|v| format!("{}: {}", esc(&f.label), esc(v)))).collect())
            .unwrap_or_default();
        s.push_str("<tr>");
        if show_game {
            let _ = write!(s, r#"<td><a href="/games/{0}">{1}</a></td>"#, esc(&r.game), esc(g.map(|g| g.name.as_str()).unwrap_or(&r.game)));
        }
        let _ = write!(
            s,
            r#"<td>{}{}</td><td>{}/{}</td><td class="muted">{}</td><td class="muted">{}</td><td><a class="btn small" href="/join/{}">Join</a></td></tr>"#,
            esc(&r.name),
            if r.entry == warpzone_protocol::api::Entry::Approve {
                r#" <span class="badge">host approves</span>"#
            } else if r.entry == warpzone_protocol::api::Entry::Code {
                r#" <span class="badge">code</span>"#
            } else {
                ""
            },
            r.players,
            r.max_players,
            audience(r),
            details.join(" · "),
            esc(&r.id)
        );
    }
    s.push_str("</table></div>");
    s
}

pub async fn index(State(state): State<AppState>) -> Html<String> {
    state.reload_games();
    let games: Vec<GameDescriptor> = state.games.read().unwrap().by_id.values().filter(|g| !g.hidden).cloned().collect();
    let rooms = list_public(&state, &RoomQuery::default());
    let releases = state.releases.read().unwrap().clone();
    let players: u32 = state.rooms.lock().unwrap().by_id.values().map(|r| r.members.len() as u32).sum();
    let mods: i64 =
        state.db().query_row("SELECT COUNT(DISTINCT game || '/' || name) FROM mod_versions WHERE state='approved' AND yanked=0", [], |r| r.get(0)).unwrap_or(0);
    let src = esc(&state.cfg.source_url);

    let mut cards = String::new();
    for g in &games {
        let n = rooms.iter().filter(|r| r.game == g.id).count();
        let req = g.requires.as_ref().map(|r| format!(r#"<p class="ver">Needs: {}</p>"#, esc(r))).unwrap_or_default();
        let _ = write!(
            cards,
            r#"<article class="card">{banner}<div class="body"><h3><a href="/games/{id}">{name}</a></h3><p class="tagline">{tag}</p>{badges}{req}{dl}</div></article>"#,
            banner = banner(&state, g),
            id = esc(&g.id),
            name = esc(&g.name),
            tag = esc(&g.tagline),
            badges = badges(g, n),
            req = req,
            dl = downloads(releases.get(&g.id), g.homepage.as_deref()),
        );
    }

    let launcher = match releases.get(crate::releases::LAUNCHER) {
        Some(r) if !r.assets.is_empty() => downloads(Some(r), None),
        _ => format!(
            r#"<div class="dl"><a class="btn" href="{src}/releases">Launcher builds on GitHub</a></div><p class="ver">The first launcher release is on its way; until then, games work on their own.</p>"#
        ),
    };

    let body = format!(
        r##"<div class="hero">
  <div>
    <h1>Play together.<br><span class="grad">Across games.</span></h1>
    <p class="lead">Warpzone connects players of independent games directly — through NAT hole-punching, with a relay when that fails — and adds rooms, private games, ranked ladders with seasons, leaderboards and a moderated mod archive. Games integrate a small library; the launcher shows everything in one place. You never need it to play.</p>
    <div class="btns"><a class="btn primary" href="#games">Browse games</a><a class="btn" href="#launcher">Get the launcher</a><a class="btn" href="{src}/blob/main/docs/self-hosting.md">Host your own</a></div>
    <div class="stats"><div><b>{ng}</b>games</div><div><b>{nr}</b>open rooms</div><div><b>{np}</b>players in rooms</div><div><b>{nm}</b>mods</div></div>
  </div>
  <div class="art"><img src="/static/logo.svg" alt=""></div>
</div>

<section id="games"><h2>Games</h2><p class="sub">Every game is free and open source. Downloads come straight from each project's GitHub releases.</p>
<div class="grid">{cards}</div></section>

<section id="rooms"><h2>Open rooms</h2><p class="sub">Public rooms right now. Private games use a join code or an invite link and are never listed.</p>
{rooms}</section>

<section id="how"><h2>How it works</h2><p class="sub">No accounts with passwords, no e-mail, no chat — just play.</p>
<div class="steps">
<div class="panel"><h3>Get a game</h3><p>Download it here or install it with the launcher, which keeps it updated and shows what changed.</p></div>
<div class="panel"><h3>Find or host a room</h3><p>Browse open rooms by language, skill and style, or host a private one and share its 6-letter code or invite link.</p></div>
<div class="panel"><h3>Connect directly</h3><p>Players connect peer to peer (IPv6 first, then IPv4 hole-punching). If that fails, the relay carries the traffic — encrypted end to end.</p></div>
<div class="panel"><h3>Compete and create</h3><p>Optional ranked queues with quarterly seasons and leaderboards. Mods sync automatically when you join a modded room.</p></div>
</div></section>

<section id="launcher"><h2>Warpzone launcher</h2><p class="sub">One small app for every Warpzone game: install and update games, browse rooms of all games without launching them, join with one click, manage mods. Linux, Windows and Steam Deck.</p>
<div class="panel">{launcher}</div></section>

<section id="selfhost"><h2>Run your own ecosystem</h2><p class="sub">Everything here — server, library, launcher — is open source. Run your own instance for your own games with one container.</p>
<div class="two"><div class="panel"><pre>git clone {src}
cd warpzone/deploy
cp config/server.example.toml config/server.toml
docker compose up -d</pre></div>
<div class="panel"><p>A single, small Rust daemon serves rooms, STUN, the relay, the ladder, leaderboards, the mod archive and this website. <a href="{src}/blob/main/docs/self-hosting.md">Self-hosting guide</a> · <a href="{src}/blob/main/docs/integration.md">Integrate a game</a> · <a href="{src}/blob/main/docs/descriptors.md">Game descriptors</a></p></div></div>
</section>"##,
        ng = games.len(),
        nr = rooms.len(),
        np = players,
        nm = mods,
        cards = cards,
        rooms = rooms_table(&state, &rooms, true),
        launcher = launcher,
    );
    page(&state, &state.cfg.name, "Play independent games together: rooms, hole-punching with relay fallback, ladders, leaderboards and mods.", &body)
}

pub async fn game_page(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Ok(g) = state.game(&id) else { return not_found(&state) };
    let rooms = list_public(&state, &RoomQuery { game: Some(id.clone()), ..Default::default() });
    let rel = state.releases.read().unwrap().get(&id).cloned();
    let hero = art_url(&state, &g)
        .map(|a| format!(r#"<img src="{}" alt="" style="width:100%;max-height:300px;object-fit:cover;border-radius:14px;margin-top:20px">"#, esc(&a)))
        .unwrap_or_default();
    let mut body = format!(
        r#"{hero}<div class="game-head"><div><h1>{name}</h1><p class="lead muted" style="margin:0">{tag}</p>{badges}</div>
<div>{home}</div></div>"#,
        name = esc(&g.name),
        tag = esc(&g.tagline),
        badges = badges(&g, rooms.len()),
        home = g.homepage.as_ref().map(|h| format!(r#"<a class="btn" href="{}">Project page</a>"#, esc(h))).unwrap_or_default(),
        hero = hero,
    );
    let _ = write!(body, r#"<section><div class="panel">{}</div>"#, downloads(rel.as_ref(), g.homepage.as_deref()));
    if !g.description.is_empty() {
        let _ = write!(body, r#"<p>{}</p>"#, esc(&g.description).replace("\n\n", "</p><p>"));
    }
    if let Some(r) = &g.requires {
        let _ = write!(body, r#"<p class="notice">You need: {}</p>"#, esc(r));
    }
    body.push_str("</section>");
    body.push_str(&changelog_section(&state, &id));
    if g.rooms.is_some() {
        let _ = write!(body, r#"<section><h2>Open rooms</h2>{}</section>"#, rooms_table(&state, &rooms, false));
    }
    if !g.ladder.is_empty() {
        body.push_str(r#"<section><h2>Ladder</h2><p class="sub">Ranked play is optional. Seasons follow the calendar; ratings carry over with a soft reset.</p><div class="two">"#);
        for q in &g.ladder {
            let s = crate::ladder::standings_inner(&state, &g, &q.id, None, 10);
            let mut t = format!(r#"<div class="panel"><h3 style="margin-top:0">{}</h3>"#, esc(&q.title));
            match s {
                Ok(s) if !s.standings.is_empty() => {
                    let _ = write!(
                        t,
                        r#"<p class="muted">Season {}{}</p><table><tr><th>#</th><th>Player</th><th>Rating</th><th>Games</th></tr>"#,
                        esc(&s.season),
                        if s.placing > 0 { format!(" · {} placing", s.placing) } else { String::new() }
                    );
                    for r in s.standings {
                        let _ = write!(t, "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", r.rank, esc(&r.username), r.rating, r.games);
                    }
                    t.push_str("</table>");
                }
                Ok(s) => {
                    let _ = write!(
                        t,
                        r#"<p class="muted">Season {} has no ranked players yet{}.</p>"#,
                        esc(&s.season),
                        if s.placing > 0 { format!(" ({} placing)", s.placing) } else { String::new() }
                    );
                }
                Err(_) => t.push_str(r#"<p class="muted">Unavailable.</p>"#),
            }
            t.push_str("</div>");
            body.push_str(&t);
        }
        body.push_str("</div></section>");
    }
    if !g.leaderboards.is_empty() {
        body.push_str(r#"<section><h2>Leaderboards</h2><div class="two">"#);
        for b in &g.leaderboards {
            let mut t = format!(r#"<div class="panel"><h3 style="margin-top:0">{}</h3>"#, esc(&b.title));
            match crate::boards::top(&state, &g, &b.id, "stock", "all", 10) {
                Ok(r) if !r.entries.is_empty() => {
                    t.push_str("<table><tr><th>#</th><th>Player</th><th>Record</th></tr>");
                    for e in r.entries {
                        let v = if b.kind == "time" { format_ms(e.value) } else { e.value.to_string() };
                        let _ = write!(t, "<tr><td>{}</td><td>{}</td><td>{}</td></tr>", e.rank, esc(&e.username), v);
                    }
                    t.push_str("</table>");
                }
                _ => t.push_str(r#"<p class="muted">No records yet.</p>"#),
            }
            t.push_str("</div>");
            body.push_str(&t);
        }
        body.push_str("</div></section>");
    }
    if let Some(m) = &g.mods {
        let list = crate::modstore::list_inner(&state, &id, &Default::default()).unwrap_or_default();
        let cats: Vec<String> = m.categories.iter().map(|c| esc(&c.label)).collect();
        let _ = write!(
            body,
            r#"<section><h2>Mods</h2><p class="sub">Categories: {}. Every mod version is reviewed by a moderator before it appears.{}</p>"#,
            cats.join(", "),
            if m.contains_code { " Mods for this game can contain code." } else { "" }
        );
        if list.is_empty() {
            body.push_str(r#"<p class="muted">No mods published yet.</p>"#);
        } else {
            body.push_str(
                r#"<div class="panel" style="padding:4px 8px"><table><tr><th>Mod</th><th>Category</th><th>Author</th><th>Version</th><th>Downloads</th></tr>"#,
            );
            for x in list.iter().take(50) {
                let label = m.categories.iter().find(|c| c.id == x.category).map(|c| c.label.as_str()).unwrap_or(&x.category);
                let _ = write!(
                    body,
                    r#"<tr><td><a href="/games/{g}/mods/{n}">{t}</a><div class="muted" style="font-size:13px">{d}</div></td><td>{c}</td><td>{a}</td><td>{v}</td><td>{dl}</td></tr>"#,
                    g = esc(&id),
                    n = esc(&x.name),
                    t = esc(&x.title),
                    d = esc(&x.description.chars().take(140).collect::<String>()),
                    c = esc(label),
                    a = esc(&x.author),
                    v = esc(&x.latest),
                    dl = x.downloads
                );
            }
            body.push_str("</table></div>");
        }
        body.push_str("</section>");
    }
    page(&state, &format!("{} · {}", g.name, state.cfg.name), &g.tagline, &body).into_response()
}

/// Minimal, safe Markdown for release notes: headings, bullet lists,
/// paragraphs, `code`, **bold** and https links. Everything is escaped first.
pub fn md(src: &str) -> String {
    fn inline(t: &str) -> String {
        let e = esc(t);
        let mut out = String::new();
        let mut rest = e.as_str();
        // `code`
        while let Some(i) = rest.find('`') {
            let Some(j) = rest[i + 1..].find('`') else { break };
            out.push_str(&bold(&rest[..i]));
            out.push_str("<code>");
            out.push_str(&rest[i + 1..i + 1 + j]);
            out.push_str("</code>");
            rest = &rest[i + 2 + j..];
        }
        out.push_str(&bold(rest));
        out
    }
    fn bold(t: &str) -> String {
        let mut out = String::new();
        let parts: Vec<&str> = t.split("**").collect();
        for (i, p) in parts.iter().enumerate() {
            if i % 2 == 1 && i + 1 < parts.len() {
                out.push_str("<b>");
                out.push_str(&links(p));
                out.push_str("</b>");
            } else {
                if i > 0 && i % 2 == 1 {
                    out.push_str("**");
                }
                out.push_str(&links(p));
            }
        }
        out
    }
    fn links(t: &str) -> String {
        let mut out = String::new();
        let mut rest = t;
        while let Some(i) = rest.find('[') {
            let Some(j) = rest[i..].find("](https://") else { break };
            let Some(k) = rest[i + j..].find(')') else { break };
            let text = &rest[i + 1..i + j];
            let url = &rest[i + j + 2..i + j + k];
            out.push_str(&rest[..i]);
            out.push_str(&format!(r#"<a href="{url}">{text}</a>"#));
            rest = &rest[i + j + k + 1..];
        }
        out.push_str(rest);
        out
    }
    let mut html = String::new();
    let mut in_list = false;
    let mut para: Vec<String> = Vec::new();
    let flush = |html: &mut String, para: &mut Vec<String>| {
        if !para.is_empty() {
            html.push_str(&format!("<p>{}</p>", para.join(" ")));
            para.clear();
        }
    };
    for line in src.lines() {
        let l = line.trim_end();
        let t = l.trim_start();
        if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush(&mut html, &mut para);
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>", inline(item)));
            continue;
        }
        if in_list && !t.is_empty() && l.starts_with("  ") {
            if let Some(last) = html.rfind("</li>") {
                html.insert_str(last, &format!(" {}", inline(t)));
            }
            continue;
        }
        if in_list {
            html.push_str("</ul>");
            in_list = false;
        }
        if t.is_empty() {
            flush(&mut html, &mut para);
        } else if let Some(h) = t.strip_prefix('#') {
            flush(&mut html, &mut para);
            html.push_str(&format!("<h4>{}</h4>", inline(h.trim_start_matches('#').trim())));
        } else {
            para.push(inline(t));
        }
    }
    if in_list {
        html.push_str("</ul>");
    }
    flush(&mut html, &mut para);
    html
}

fn changelog_section(state: &AppState, key: &str) -> String {
    let logs = state.changelogs.read().unwrap();
    let Some(h) = logs.get(key).filter(|h| !h.is_empty()) else { return String::new() };
    let mut s = String::from(
        r#"<section><h2>What's new</h2><p class="sub">From the project's release notes. Games and the launcher show these after every update.</p>"#,
    );
    for e in h.iter().take(3) {
        let _ = write!(
            s,
            r#"<div class="panel" style="margin-bottom:12px"><h3 style="margin:0">{}{}</h3><p class="muted" style="margin:2px 0 8px">{} · <a href="{}">on GitHub</a></p>{}</div>"#,
            esc(if e.name.is_empty() { &e.version } else { &e.name }),
            if e.prerelease { r#" <span class="badge warn">preview</span>"# } else { "" },
            date(&e.published),
            esc(&e.page),
            md(&e.notes)
        );
    }
    s.push_str("</section>");
    s
}

fn format_ms(ms: i64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}.{:03}", total / 60, total % 60, ms % 1000)
}

pub async fn mod_page(State(state): State<AppState>, Path((game, name)): Path<(String, String)>) -> Response {
    let (Ok(g), Ok(d)) = (state.game(&game), crate::modstore::detail_inner(&state, &game, &name, None)) else { return not_found(&state) };
    let cat =
        g.mods.as_ref().and_then(|m| m.categories.iter().find(|c| c.id == d.summary.category)).map(|c| c.label.clone()).unwrap_or(d.summary.category.clone());
    let mut body = format!(
        r#"<div class="game-head"><div><p class="muted" style="margin:0"><a href="/games/{gid}">{gname}</a> · {cat}</p><h1>{title}</h1><p class="muted" style="margin:0">by {author} · {license} · {dl} downloads</p></div><div>{home}</div></div><section>"#,
        gid = esc(&game),
        gname = esc(&g.name),
        cat = esc(&cat),
        title = esc(&d.summary.title),
        author = esc(&d.summary.author),
        license = esc(&d.license),
        dl = d.summary.downloads,
        home =
            d.homepage.as_ref().filter(|h| h.starts_with("https://")).map(|h| format!(r#"<a class="btn" href="{}">Homepage</a>"#, esc(h))).unwrap_or_default(),
    );
    if d.contains_code {
        body.push_str(r#"<p class="notice">Mods for this game can contain code. Only install mods from authors you trust.</p>"#);
    }
    let _ = write!(body, "<p>{}</p>", esc(&d.summary.description).replace('\n', "<br>"));
    body.push_str(r#"<div class="panel" style="padding:4px 8px"><table><tr><th>Version</th><th>Game</th><th>Dependencies</th><th>Size</th><th>Published</th><th></th></tr>"#);
    for v in &d.versions {
        let _ = write!(
            body,
            r#"<tr><td>{ver}{y}</td><td>{gv}</td><td class="muted">{deps}</td><td>{size}</td><td>{date}</td><td><a class="btn small" href="/api/v1/games/{g}/mods/{n}/{ver}/download">Download</a></td></tr>"#,
            ver = esc(&v.version),
            y = if v.yanked { r#" <span class="badge warn">yanked</span>"# } else { "" },
            gv = esc(&v.game_version),
            deps = esc(&v.dependencies.join(", ")),
            size = mb(v.size),
            date = unix_date(v.published),
            g = esc(&game),
            n = esc(&name)
        );
    }
    body.push_str(
        "</table></div><p class=\"muted\">The launcher and games that use Warpzone install mods with their dependencies automatically.</p></section>",
    );
    page(&state, &format!("{} · {}", d.summary.title, g.name), &d.summary.description.chars().take(150).collect::<String>(), &body).into_response()
}

pub async fn join_page(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let id_ok = id.len() <= 16 && id.bytes().all(|b| b.is_ascii_alphanumeric());
    if !id_ok {
        return not_found(&state);
    }
    let src = esc(&state.cfg.source_url);
    let body = format!(
        r##"<section style="max-width:720px"><h1>Join a game</h1>
<div class="panel" id="room"><p class="muted">Looking up the room…</p></div>
<p style="margin-top:18px" class="btns"><a class="btn primary" id="open" href="warpzone://join/{id}">Open in the Warpzone launcher</a><a class="btn" href="/#launcher">Get the launcher</a></p>
<p class="muted">Games with Warpzone built in can also join from their own online menu with the room's code. <a href="{src}/blob/main/docs/players.md">How joining works</a></p>
</section>
<script>
(function () {{
  var id = {id_js};
  var secret = location.hash ? location.hash.slice(1) : "";
  var link = "warpzone://join/" + id + (secret ? "#" + secret : "");
  document.getElementById("open").href = link;
  fetch("/api/v1/rooms/" + id + (secret ? "?s=" + encodeURIComponent(secret) : "")).then(function (r) {{ return r.ok ? r.json() : null; }}).then(function (r) {{
    var el = document.getElementById("room");
    if (!r) {{ el.innerHTML = "<p>This room is private or no longer open. If you have an invite link, open it in the launcher.</p>"; return; }}
    var t = document.createElement("div");
    var h = document.createElement("h2"); h.textContent = r.name; h.style.marginTop = "0"; t.appendChild(h);
    var p = document.createElement("p"); p.className = "muted"; p.textContent = r.game + " · " + r.players + "/" + r.max_players + " players · version " + r.game_version; t.appendChild(p);
    el.innerHTML = ""; el.appendChild(t);
  }});
}})();
</script>"##,
        id = esc(&id),
        id_js = serde_json::to_string(&id).unwrap(),
    );
    page(&state, &format!("Join a game · {}", state.cfg.name), "Join a Warpzone room", &body).into_response()
}

pub fn not_found(state: &AppState) -> Response {
    let body = r#"<section><h1>Not found</h1><p class="muted">That page does not exist. <a href="/">Back to the start page</a>.</p></section>"#;
    (StatusCode::NOT_FOUND, page(state, "Not found", "", body)).into_response()
}

pub async fn fallback(State(state): State<AppState>) -> Response {
    not_found(&state)
}

/// `/art/<id>.jpg` from `games_dir/art/`.
pub async fn art_file(State(state): State<AppState>, Path(file): Path<String>) -> Result<Response, ApiError> {
    let ok = file.ends_with(".jpg") && file.len() < 64 && file.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') && !file.contains("..");
    if !ok {
        return Err(ApiError::not_found("no such file"));
    }
    let bytes = tokio::fs::read(state.cfg.games_dir.join("art").join(&file)).await.map_err(|_| ApiError::not_found("no such file"))?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "public, max-age=86400")], bytes).into_response())
}

pub async fn static_file(Path(file): Path<String>) -> Result<Response, ApiError> {
    let (ct, body) = match file.as_str() {
        "style.css" => ("text/css; charset=utf-8", CSS),
        "logo.svg" => ("image/svg+xml", LOGO),
        _ => return Err(ApiError::not_found("no such file")),
    };
    Ok(([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=3600")], body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(esc(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }

    #[test]
    fn markdown() {
        let h = md("## Changes\n- **New:** `x` <b>\n- see [docs](https://e.org)\n\nDone.");
        assert_eq!(h, "<h4>Changes</h4><ul><li><b>New:</b> <code>x</code> &lt;b&gt;</li><li>see <a href=\"https://e.org\">docs</a></li></ul><p>Done.</p>");
        assert!(!md("[x](javascript:alert(1))").contains("<a"));
    }

    #[test]
    fn dates() {
        assert_eq!(unix_date(1_792_152_000), "2026-10-16");
        assert_eq!(format_ms(83_456), "1:23.456");
    }
}
