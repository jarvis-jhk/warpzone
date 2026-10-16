//! Finding the original game data a player already owns.
//!
//! Many Warpzone games are engines for classic games and need the player's own
//! files (a GOG installer, `DIABDAT.MPQ`, a ROM). The descriptor's `[[data]]`
//! entries say what identifies them; this module looks in the places such
//! files usually are:
//!
//! * Steam libraries (every `libraryfolders.vdf` entry, native, Flatpak and
//!   Snap Steam, and Proton prefixes),
//! * GOG: `~/GOG Games`, GOG Galaxy and Heroic installs,
//! * Lutris (`~/Games`) and Wine prefixes,
//! * the Downloads folder (XDG `user-dirs` aware) and the Desktop,
//! * mounted discs and images (`/media`, `/run/media`, other Windows drives).
//!
//! Patterns: a plain name matches a file; one with a `/` matches the
//! directory containing that path (`DATA/IO/IO.DAT`); `./name` matches the
//! directory directly containing `name` (for games that want a folder).
//!
//! Searches are bounded (depth and number of entries) and read only names, so
//! they finish in well under a second on a normal home directory.

use std::path::{Path, PathBuf};
use warpzone_protocol::descriptor::{DataRequirement, GameDescriptor, glob_match};

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The matching file, or the directory that contains the pattern's path.
    pub path: PathBuf,
    /// Where it was found, for display ("Steam", "Downloads", ...).
    pub source: String,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Steam roots on this machine.
fn steam_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home() {
        for p in [".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam", "snap/steam/common/.local/share/Steam"] {
            v.push(h.join(p));
        }
    }
    if cfg!(windows) {
        v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        v.push(PathBuf::from(r"C:\Program Files\Steam"));
    }
    v.retain(|p| p.is_dir());
    v
}

/// Library folders listed in `steamapps/libraryfolders.vdf`.
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in steam_roots() {
        libs.push(root.clone());
        if let Ok(text) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for line in text.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("\"path\"") {
                    let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
                    libs.push(PathBuf::from(p));
                }
            }
        }
    }
    let mut seen = Vec::new();
    for l in libs {
        let c = std::fs::canonicalize(&l).unwrap_or(l);
        if c.is_dir() && !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen
}

fn downloads_dir() -> Option<PathBuf> {
    let h = home()?;
    if let Ok(t) = std::fs::read_to_string(h.join(".config/user-dirs.dirs")) {
        for line in t.lines() {
            if let Some(v) = line.strip_prefix("XDG_DOWNLOAD_DIR=") {
                let v = v.trim_matches('"').replace("$HOME", &h.to_string_lossy());
                return Some(PathBuf::from(v));
            }
        }
    }
    Some(h.join("Downloads"))
}

/// (root, label, depth) to search for one requirement.
fn roots_for(req: &DataRequirement) -> Vec<(PathBuf, String, usize)> {
    let mut roots = Vec::new();
    for lib in steam_libraries() {
        for dir in &req.steam {
            roots.push((lib.join("steamapps/common").join(dir), "Steam".to_string(), 5));
        }
        // Proton prefixes (games installed inside Windows installers run via Steam).
        if !req.gog.is_empty()
            && let Ok(rd) = std::fs::read_dir(lib.join("steamapps/compatdata"))
        {
            for e in rd.flatten() {
                for g in &req.gog {
                    for base in ["pfx/drive_c/GOG Games", "pfx/drive_c/Program Files (x86)/GOG Galaxy/Games", "pfx/drive_c/Program Files (x86)"] {
                        roots.push((e.path().join(base).join(g), "Proton".to_string(), 4));
                    }
                }
            }
        }
    }
    if let Some(h) = home() {
        for g in &req.gog {
            roots.push((h.join("GOG Games").join(g), "GOG".into(), 4));
            roots.push((h.join("Games/Heroic").join(g), "Heroic".into(), 4));
            roots.push((h.join("Games").join(g), "Lutris".into(), 5));
            roots.push((h.join(".wine/drive_c/GOG Games").join(g), "Wine".into(), 4));
            roots.push((h.join(".wine/drive_c/Program Files (x86)").join(g), "Wine".into(), 4));
        }
        for s in &req.steam {
            roots.push((h.join("Games").join(s), "Lutris".into(), 5));
        }
        // Heroic's record of GOG installs.
        if let Ok(t) = std::fs::read_to_string(h.join(".config/heroic/gog_store/installed.json"))
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
        {
            for i in v["installed"].as_array().into_iter().flatten() {
                if let Some(p) = i["install_path"].as_str() {
                    let name = Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if req.gog.iter().any(|g| g.eq_ignore_ascii_case(&name)) {
                        roots.push((PathBuf::from(p), "Heroic".into(), 4));
                    }
                }
            }
        }
        if req.downloads {
            if let Some(d) = downloads_dir() {
                roots.push((d, "Downloads".into(), 2));
            }
            roots.push((h.join("Desktop"), "Desktop".into(), 1));
        }
    }
    if cfg!(windows) {
        for g in &req.gog {
            roots.push((PathBuf::from(r"C:\GOG Games").join(g), "GOG".into(), 4));
            roots.push((PathBuf::from(r"C:\Program Files (x86)\GOG Galaxy\Games").join(g), "GOG".into(), 4));
            roots.push((PathBuf::from(r"C:\Program Files (x86)").join(g), "Program Files".into(), 4));
            roots.push((PathBuf::from(r"C:\Program Files").join(g), "Program Files".into(), 4));
        }
        if req.discs {
            for d in b'D'..=b'Z' {
                let p = PathBuf::from(format!("{}:\\", d as char));
                if p.is_dir() {
                    roots.push((p, "Disc/drive".into(), 2));
                }
            }
        }
    } else if req.discs {
        let user = std::env::var("USER").unwrap_or_default();
        for base in [format!("/media/{user}"), format!("/run/media/{user}"), "/media".to_string(), "/mnt".to_string()] {
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    roots.push((e.path(), "Disc".into(), 3));
                }
            }
        }
    }
    roots.retain(|(p, _, _)| p.is_dir());
    roots
}

fn matches_ci(pattern: &str, name: &str) -> bool {
    glob_match(&pattern.to_lowercase(), &name.to_lowercase())
}

/// Does `dir` contain `rel` (a `/`-separated path, case-insensitive)?
fn contains_ci(dir: &Path, rel: &str) -> bool {
    let mut cur = dir.to_path_buf();
    for seg in rel.split('/') {
        let Ok(rd) = std::fs::read_dir(&cur) else { return false };
        match rd.flatten().find(|e| matches_ci(seg, &e.file_name().to_string_lossy())) {
            Some(e) => cur = e.path(),
            None => return false,
        }
    }
    true
}

fn walk(dir: &Path, depth: usize, budget: &mut usize, req: &DataRequirement, source: &str, out: &mut Vec<Found>) {
    if *budget == 0 {
        return;
    }
    for pat in req.files.iter().filter(|p| p.contains('/')) {
        if contains_ci(dir, pat.trim_start_matches("./")) && !out.iter().any(|f| f.path == dir) {
            out.push(Found { path: dir.to_path_buf(), source: source.into() });
        }
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_file() {
            if req.files.iter().filter(|p| !p.contains('/')).any(|p| matches_ci(p, &name)) {
                out.push(Found { path: e.path(), source: source.into() });
            }
        } else if ft.is_dir() && depth > 0 && !name.starts_with('.') {
            walk(&e.path(), depth - 1, budget, req, source, out);
        }
    }
}

/// Search the usual places for `req`. `extra` adds folders the player chose.
pub fn find(req: &DataRequirement, extra: &[PathBuf]) -> Vec<Found> {
    let mut out = Vec::new();
    let mut roots = roots_for(req);
    for e in extra {
        roots.push((e.clone(), "Chosen folder".into(), 4));
    }
    for (root, source, depth) in roots {
        let mut budget = 20_000;
        walk(&root, depth, &mut budget, req, &source, &mut out);
    }
    out.dedup_by(|a, b| a.path == b.path);
    out
}

/// Result of checking a found source against the descriptor's fingerprints.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// Matches a supported known-good file.
    Verified(String),
    /// Matches a known file the game does not support (e.g. another version).
    Unsupported(String),
    /// The descriptor lists fingerprints and none matches.
    Unknown,
    /// The descriptor lists no fingerprints; nothing to check.
    Unchecked,
}

fn hash_file(p: &Path) -> std::io::Result<(String, String)> {
    use sha1::Digest as _;
    use std::io::Read;
    let mut f = std::fs::File::open(p)?;
    let mut s1 = sha1::Sha1::new();
    let mut s256 = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        s1.update(&buf[..n]);
        sha2::Digest::update(&mut s256, &buf[..n]);
    }
    Ok((hex::encode(s1.finalize()), hex::encode(sha2::Digest::finalize(s256))))
}

/// Check `found` against `req.known` (no engine download needed: the
/// fingerprints come from the descriptor).
pub fn check(req: &DataRequirement, found: &Path) -> Check {
    if req.known.is_empty() {
        return Check::Unchecked;
    }
    for k in &req.known {
        let target = if k.file.is_empty() { found.to_path_buf() } else { found.join(&k.file) };
        let Ok(meta) = std::fs::metadata(&target) else { continue };
        if !meta.is_file() || meta.len() != k.size {
            continue;
        }
        let Ok((s1, s256)) = hash_file(&target) else { continue };
        let ok1 = k.sha1.as_ref().is_none_or(|h| h.eq_ignore_ascii_case(&s1));
        let ok256 = k.sha256.as_ref().is_none_or(|h| h.eq_ignore_ascii_case(&s256));
        if ok1 && ok256 && (k.sha1.is_some() || k.sha256.is_some()) {
            return if k.supported { Check::Verified(k.label.clone()) } else { Check::Unsupported(k.label.clone()) };
        }
    }
    Check::Unknown
}

/// Expand `{install}` and `~` in a descriptor path.
pub fn expand(template: &str, install_dir: Option<&Path>) -> PathBuf {
    let mut s = template.to_string();
    if let Some(i) = install_dir {
        s = s.replace("{install}", &i.to_string_lossy());
    }
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(h) = home()
    {
        return h.join(rest);
    }
    PathBuf::from(s)
}

/// Whether the requirement is already satisfied (one of its `present` paths exists).
pub fn present(req: &DataRequirement, install_dir: Option<&Path>) -> bool {
    req.present.iter().any(|p| expand(p, install_dir).exists())
}

/// Hand the data to the game: copy it into `copy_to`, or return the import
/// arguments to start the game with.
pub fn hand_over(req: &DataRequirement, found: &Path, install_dir: Option<&Path>) -> std::io::Result<Option<Vec<String>>> {
    if !req.import_args.is_empty() {
        return Ok(Some(req.import_args.iter().map(|a| a.replace("{path}", &found.to_string_lossy())).collect()));
    }
    if let Some(dest) = &req.copy_to {
        let dest = expand(dest, install_dir);
        if found.is_dir() {
            copy_dir(found, &dest)?;
        } else {
            let target = if dest.extension().is_some() { dest.clone() } else { dest.join(found.file_name().unwrap_or_default()) };
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(found, target)?;
        }
        return Ok(None);
    }
    Err(std::io::Error::other("the descriptor says neither import_args nor copy_to"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let t = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &t)?;
        } else {
            std::fs::copy(e.path(), t)?;
        }
    }
    Ok(())
}

/// Requirements of `desc` that are not yet satisfied for this install.
pub fn missing<'a>(desc: &'a GameDescriptor, install_dir: Option<&Path>) -> Vec<&'a DataRequirement> {
    desc.data.iter().filter(|r| !present(r, install_dir)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(files: &[&str]) -> DataRequirement {
        DataRequirement {
            id: "x".into(),
            label: "x".into(),
            description: None,
            files: files.iter().map(|s| s.to_string()).collect(),
            steam: vec![],
            gog: vec![],
            downloads: false,
            discs: false,
            buy: vec![],
            free: vec![],
            import_args: vec![],
            copy_to: None,
            present: vec![],
            known: vec![],
            optional: false,
        }
    }

    #[test]
    fn fingerprints() {
        use sha2::Digest;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("rom.gb");
        std::fs::write(&f, b"abc").unwrap();
        let mut r = req(&["*.gb"]);
        assert_eq!(check(&r, &f), Check::Unchecked);
        r.known.push(warpzone_protocol::descriptor::KnownFile {
            label: "good".into(),
            file: String::new(),
            size: 3,
            sha1: None,
            sha256: Some(hex::encode(sha2::Sha256::digest(b"abc"))),
            supported: true,
        });
        assert_eq!(check(&r, &f), Check::Verified("good".into()));
        std::fs::write(&f, b"abd").unwrap();
        assert_eq!(check(&r, &f), Check::Unknown);
    }

    #[test]
    fn finds_files_and_dirs_case_insensitively() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("game/Data/IO")).unwrap();
        std::fs::write(d.path().join("game/Data/IO/io.dat"), b"").unwrap();
        std::fs::write(d.path().join("game/DIABDAT.MPQ"), b"").unwrap();
        let f = find(&req(&["diabdat.mpq"]), &[d.path().to_path_buf()]);
        assert_eq!(f.len(), 1);
        assert!(f[0].path.ends_with("DIABDAT.MPQ"));
        let f = find(&req(&["DATA/IO/IO.DAT"]), &[d.path().to_path_buf()]);
        assert_eq!(f[0].path, d.path().join("game"));
    }

    #[test]
    fn hand_over_copies_or_builds_args() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("rom.gb");
        std::fs::write(&src, b"rom").unwrap();
        let mut r = req(&["*.gb"]);
        r.import_args = vec!["--import".into(), "{path}".into()];
        assert_eq!(hand_over(&r, &src, None).unwrap().unwrap(), vec!["--import".to_string(), src.to_string_lossy().to_string()]);
        let mut r = req(&["*.gb"]);
        r.copy_to = Some(format!("{}/rom/sml.gb", "{install}"));
        hand_over(&r, &src, Some(d.path())).unwrap();
        assert_eq!(std::fs::read(d.path().join("rom/sml.gb")).unwrap(), b"rom");
        r.present = vec!["{install}/rom/sml.gb".into()];
        assert!(present(&r, Some(d.path())));
    }
}
