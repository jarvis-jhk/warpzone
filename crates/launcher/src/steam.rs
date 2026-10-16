//! "Add to Steam": a non-Steam shortcut with the game's name and a complete
//! artwork set (portrait and wide capsule, hero, logo, icon).
//!
//! Steam keeps shortcuts in `userdata/<user>/config/shortcuts.vdf`, a binary
//! KeyValues file, and artwork in `userdata/<user>/config/grid/<id>*.png`.
//! Steam rewrites the file when it exits, so changes are only made while
//! Steam is closed (the launcher offers to close it). An existing shortcut
//! with the same name is updated instead of duplicated.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Map(Vec<(String, Node)>),
    Str(String),
    Int(i32),
    /// Types we do not interpret (float, pointer, colour, u64): kept verbatim.
    Raw(u8, Vec<u8>),
}

impl Node {
    fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(v) => v.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, n)| n),
            _ => None,
        }
    }
    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Node::Str(s)) => Some(s),
            _ => None,
        }
    }
}

fn read_cstr(b: &[u8], i: &mut usize) -> Option<String> {
    let end = b[*i..].iter().position(|c| *c == 0)? + *i;
    let s = String::from_utf8_lossy(&b[*i..end]).to_string();
    *i = end + 1;
    Some(s)
}

fn parse_map(b: &[u8], i: &mut usize) -> Option<Vec<(String, Node)>> {
    let mut out = Vec::new();
    loop {
        let t = *b.get(*i)?;
        *i += 1;
        if t == 0x08 {
            return Some(out);
        }
        let key = read_cstr(b, i)?;
        let node = match t {
            0x00 => Node::Map(parse_map(b, i)?),
            0x01 => Node::Str(read_cstr(b, i)?),
            0x02 => {
                let v = i32::from_le_bytes(b.get(*i..*i + 4)?.try_into().ok()?);
                *i += 4;
                Node::Int(v)
            }
            0x03 | 0x04 | 0x06 => {
                let v = b.get(*i..*i + 4)?.to_vec();
                *i += 4;
                Node::Raw(t, v)
            }
            0x07 => {
                let v = b.get(*i..*i + 8)?.to_vec();
                *i += 8;
                Node::Raw(t, v)
            }
            _ => return None,
        };
        out.push((key, node));
    }
}

pub fn parse(b: &[u8]) -> Option<Node> {
    let mut i = 0;
    let root = parse_map(b, &mut i)?;
    Some(Node::Map(root))
}

fn write_map(out: &mut Vec<u8>, m: &[(String, Node)]) {
    for (k, n) in m {
        let t = match n {
            Node::Map(_) => 0x00,
            Node::Str(_) => 0x01,
            Node::Int(_) => 0x02,
            Node::Raw(t, _) => *t,
        };
        out.push(t);
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        match n {
            Node::Map(v) => write_map(out, v),
            Node::Str(s) => {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            Node::Int(v) => out.extend_from_slice(&v.to_le_bytes()),
            Node::Raw(_, raw) => out.extend_from_slice(raw),
        }
    }
    out.push(0x08);
}

pub fn write(root: &Node) -> Vec<u8> {
    let mut out = Vec::new();
    if let Node::Map(m) = root {
        write_map(&mut out, m);
    }
    out
}

/// Steam's id for a non-Steam shortcut (also the artwork file prefix).
pub fn shortcut_id(exe: &str, name: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// Steam roots that have user data.
pub fn steam_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from) {
        for p in [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            v.push(h.join(p));
        }
    }
    if cfg!(windows) {
        v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for p in v {
        if p.join("userdata").is_dir() {
            let c = std::fs::canonicalize(&p).unwrap_or(p);
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// `userdata/<id>/config` directories (one per Steam account on this machine).
pub fn user_configs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for root in steam_roots() {
        if let Ok(rd) = std::fs::read_dir(root.join("userdata")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.chars().all(|c| c.is_ascii_digit()) && n != "0" {
                    v.push(e.path().join("config"));
                }
            }
        }
    }
    v
}

pub fn steam_installed() -> bool {
    !steam_roots().is_empty()
}

/// Is the Steam client running? (It would overwrite our change on exit.)
pub fn steam_running() -> bool {
    if cfg!(windows) {
        return std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("steam.exe"));
    }
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| {
                    let c = c.trim();
                    c == "steam" || c == "steamwebhelper"
                })
            })
        })
        .unwrap_or(false)
}

/// Ask Steam to exit; returns once it has (or after 20 s).
pub fn close_steam() {
    let _ = std::process::Command::new(if cfg!(windows) { r"C:\Program Files (x86)\Steam\steam.exe" } else { "steam" }).arg("-shutdown").spawn();
    let t = std::time::Instant::now();
    while steam_running() && t.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub struct Shortcut<'a> {
    pub name: &'a str,
    pub exe: &'a Path,
    pub start_dir: &'a Path,
    pub launch_options: &'a str,
    /// (suffix, png bytes): "p" capsule, "" wide, "_hero", "_logo", "_icon".
    pub art: Vec<(&'static str, Vec<u8>)>,
}

/// Add or update the shortcut for every Steam user here. Returns how many
/// users got it.
pub fn add(s: &Shortcut) -> std::io::Result<usize> {
    if steam_running() {
        return Err(std::io::Error::other("Steam is running; close it first (Steam would undo the change)"));
    }
    let exe_q = format!("\"{}\"", s.exe.display());
    let dir_q = format!("\"{}\"", s.start_dir.display());
    let id = shortcut_id(&exe_q, s.name);
    let mut n = 0;
    for cfg in user_configs() {
        std::fs::create_dir_all(&cfg)?;
        let file = cfg.join("shortcuts.vdf");
        let mut root = std::fs::read(&file).ok().and_then(|b| parse(&b)).unwrap_or(Node::Map(vec![("shortcuts".into(), Node::Map(vec![]))]));
        let Node::Map(top) = &mut root else { continue };
        if !top.iter().any(|(k, _)| k.eq_ignore_ascii_case("shortcuts")) {
            top.push(("shortcuts".into(), Node::Map(vec![])));
        }
        let list = top.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("shortcuts")).map(|(_, n)| n).unwrap();
        let Node::Map(entries) = list else { continue };
        // Icon file next to the grid images.
        let grid = cfg.join("grid");
        std::fs::create_dir_all(&grid)?;
        for (suffix, png) in &s.art {
            std::fs::write(grid.join(format!("{id}{suffix}.png")), png)?;
        }
        let icon = grid.join(format!("{id}_icon.png"));
        let entry = Node::Map(vec![
            ("appid".into(), Node::Int(id as i32)),
            ("AppName".into(), Node::Str(s.name.into())),
            ("Exe".into(), Node::Str(exe_q.clone())),
            ("StartDir".into(), Node::Str(dir_q.clone())),
            ("icon".into(), Node::Str(if icon.exists() { icon.display().to_string() } else { String::new() })),
            ("ShortcutPath".into(), Node::Str(String::new())),
            ("LaunchOptions".into(), Node::Str(s.launch_options.into())),
            ("IsHidden".into(), Node::Int(0)),
            ("AllowDesktopConfig".into(), Node::Int(1)),
            ("AllowOverlay".into(), Node::Int(1)),
            ("OpenVR".into(), Node::Int(0)),
            ("Devkit".into(), Node::Int(0)),
            ("DevkitGameID".into(), Node::Str(String::new())),
            ("DevkitOverrideAppID".into(), Node::Int(0)),
            ("LastPlayTime".into(), Node::Int(0)),
            ("FlatpakAppID".into(), Node::Str(String::new())),
            ("tags".into(), Node::Map(vec![])),
        ]);
        match entries.iter_mut().find(|(_, e)| e.str("AppName").is_some_and(|n| n == s.name)) {
            Some((_, e)) => *e = entry,
            None => {
                let next = entries.iter().filter_map(|(k, _)| k.parse::<u32>().ok()).max().map(|m| m + 1).unwrap_or(0);
                entries.push((next.to_string(), entry));
            }
        }
        if file.exists() {
            let _ = std::fs::copy(&file, cfg.join("shortcuts.vdf.warpzone-backup"));
        }
        warpzone_core::paths::write_atomic(&file, &write(&root))?;
        n += 1;
    }
    if n == 0 {
        return Err(std::io::Error::other("no Steam user found on this machine"));
    }
    Ok(n)
}

/// Is there a shortcut with this name for any user?
pub fn has(name: &str) -> bool {
    user_configs().iter().any(|cfg| {
        std::fs::read(cfg.join("shortcuts.vdf"))
            .ok()
            .and_then(|b| parse(&b))
            .and_then(|r| r.get("shortcuts").cloned())
            .is_some_and(|s| matches!(s, Node::Map(v) if v.iter().any(|(_, e)| e.str("AppName") == Some(name))))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_roundtrip() {
        let root = Node::Map(vec![(
            "shortcuts".into(),
            Node::Map(vec![(
                "0".into(),
                Node::Map(vec![
                    ("appid".into(), Node::Int(-123)),
                    ("AppName".into(), Node::Str("Game".into())),
                    ("x".into(), Node::Raw(0x07, vec![1, 2, 3, 4, 5, 6, 7, 8])),
                    ("tags".into(), Node::Map(vec![("0".into(), Node::Str("fav".into()))])),
                ]),
            )]),
        )]);
        let b = write(&root);
        assert_eq!(parse(&b).unwrap(), root);
    }

    #[test]
    fn ids_have_high_bit() {
        assert!(shortcut_id("\"/x\"", "Game") & 0x8000_0000 != 0);
    }
}
