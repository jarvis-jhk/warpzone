//! Local mod store: `<home>/mods/<game>/`.
//!
//! Installing a lockfile downloads every missing version into a staging
//! directory, verifies its SHA-256, optionally unpacks it (with path, symlink
//! and size checks), moves it into place and only then switches the active
//! lockfile atomically. An interrupted sync leaves the previous set usable.

use crate::api::Api;
use crate::{Error, Result};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use warpzone_protocol::mods::{Dependency, Installed, LockEntry, Lockfile, validate_set};

/// What we keep next to each installed version.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstalledMod {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub size: u64,
    pub category: String,
    pub game_version: String,
    pub dependencies: Vec<String>,
    /// File name of the archive (archive mode) — the path games load.
    pub file: String,
    pub extracted: bool,
}

pub struct ModStore {
    pub game: String,
    pub root: PathBuf,
    pub extract: bool,
}

const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

impl ModStore {
    pub fn new(game: &str, extract: bool) -> ModStore {
        ModStore { game: game.into(), root: crate::paths::mods_dir(game), extract }
    }

    pub fn at(root: PathBuf, game: &str, extract: bool) -> ModStore {
        ModStore { game: game.into(), root, extract }
    }

    pub fn version_dir(&self, name: &str, version: &str) -> PathBuf {
        self.root.join(name).join(version)
    }

    fn meta_path(&self, name: &str, version: &str) -> PathBuf {
        self.version_dir(name, version).join("warpzone-installed.json")
    }

    pub fn get(&self, name: &str, version: &str) -> Option<InstalledMod> {
        let t = std::fs::read_to_string(self.meta_path(name, version)).ok()?;
        serde_json::from_str(&t).ok()
    }

    pub fn is_installed(&self, e: &LockEntry) -> bool {
        self.get(&e.name, &e.version).is_some_and(|m| m.sha256 == e.sha256)
    }

    /// Every installed version.
    pub fn installed(&self) -> Vec<InstalledMod> {
        let mut out = Vec::new();
        let Ok(names) = std::fs::read_dir(&self.root) else { return out };
        for n in names.flatten().filter(|e| e.path().is_dir()) {
            let Ok(vers) = std::fs::read_dir(n.path()) else { continue };
            for v in vers.flatten() {
                if let Ok(t) = std::fs::read_to_string(v.path().join("warpzone-installed.json"))
                    && let Ok(m) = serde_json::from_str::<InstalledMod>(&t)
                {
                    out.push(m);
                }
            }
        }
        out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        out
    }

    /// Path a game loads for an installed version: the unpacked directory or
    /// the archive file.
    pub fn content_path(&self, m: &InstalledMod) -> PathBuf {
        let d = self.version_dir(&m.name, &m.version);
        if m.extracted { d.join("files") } else { d.join(&m.file) }
    }

    pub fn active(&self) -> Option<Lockfile> {
        let t = std::fs::read_to_string(self.root.join("active.lock.json")).ok()?;
        serde_json::from_str::<Lockfile>(&t).ok().filter(|l| l.verify_hash())
    }

    pub fn set_active(&self, lock: Option<&Lockfile>) -> Result<()> {
        let p = self.root.join("active.lock.json");
        match lock {
            Some(l) => crate::paths::write_atomic(&p, &serde_json::to_vec_pretty(l).unwrap())?,
            None => {
                let _ = std::fs::remove_file(p);
            }
        }
        Ok(())
    }

    /// Content paths of a lockfile, in load order. Fails if anything is missing.
    pub fn paths(&self, lock: &Lockfile) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for name in &lock.order {
            let e = lock.mods.iter().find(|m| &m.name == name).ok_or_else(|| Error::Other(format!("{name} missing from lockfile")))?;
            let m = self.get(&e.name, &e.version).filter(|m| m.sha256 == e.sha256).ok_or_else(|| Error::Other(format!("{} {} is not installed", e.name, e.version)))?;
            out.push(self.content_path(&m));
        }
        Ok(out)
    }

    /// Download what is missing, then activate `lock`. `progress(done, total)` in bytes.
    pub fn sync(&self, api: &Api, lock: &Lockfile, progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<PathBuf>> {
        if lock.game != self.game || !lock.verify_hash() {
            return Err(Error::Other("lockfile does not verify".into()));
        }
        let missing: Vec<&LockEntry> = lock.mods.iter().filter(|e| !self.is_installed(e)).collect();
        let total: u64 = missing.iter().map(|e| e.size).sum();
        let mut done = 0u64;
        progress(done, total);
        for e in missing {
            let detail = api.mod_detail(&self.game, &e.name)?;
            let v = detail
                .versions
                .iter()
                .find(|v| v.version == e.version && v.sha256 == e.sha256)
                .ok_or_else(|| Error::Other(format!("{} {} is not available", e.name, e.version)))?;
            let staging = self.root.join(".staging").join(format!("{}-{}-{}", e.name, e.version, std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging)?;
            let file = format!("{}_{}.zip", e.name, e.version);
            let archive = staging.join(&file);
            api.download_mod(&self.game, &e.name, &e.version, &e.sha256, &archive)?;
            if self.extract {
                unzip_safe(&archive, &staging.join("files"))?;
                std::fs::remove_file(&archive)?;
            }
            let meta = InstalledMod {
                name: e.name.clone(),
                version: e.version.clone(),
                sha256: e.sha256.clone(),
                size: e.size,
                category: e.category.clone(),
                game_version: v.game_version.clone(),
                dependencies: v.dependencies.clone(),
                file,
                extracted: self.extract,
            };
            std::fs::write(staging.join("warpzone-installed.json"), serde_json::to_vec_pretty(&meta).unwrap())?;
            let dest = self.version_dir(&e.name, &e.version);
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::create_dir_all(dest.parent().unwrap())?;
            std::fs::rename(&staging, &dest)?;
            done += e.size;
            progress(done, total);
        }
        let paths = self.paths(lock)?;
        self.set_active(Some(lock))?;
        Ok(paths)
    }

    /// Offline check of a set of installed versions.
    pub fn validate(&self, enabled: &[(String, String)], game_version: &str) -> Vec<String> {
        let gv = warpzone_protocol::mods::parse_version(game_version).unwrap_or(Version::new(0, 0, 0));
        let mut set = Vec::new();
        let mut problems = Vec::new();
        for (n, v) in enabled {
            match self.get(n, v) {
                Some(m) => set.push(Installed {
                    name: m.name.clone(),
                    version: Version::parse(&m.version).unwrap_or(Version::new(0, 0, 0)),
                    deps: m.dependencies.iter().filter_map(|d| Dependency::parse(d).ok()).collect(),
                    game_req: VersionReq::parse(&m.game_version).unwrap_or(VersionReq::STAR),
                }),
                None => problems.push(format!("{n} {v} is not installed")),
            }
        }
        problems.extend(validate_set(&set, &gv));
        problems
    }

    /// Remove versions no lockfile in `keep` references.
    pub fn gc(&self, keep: &[Lockfile]) -> usize {
        let wanted: HashSet<(String, String)> = keep.iter().flat_map(|l| l.mods.iter().map(|m| (m.name.clone(), m.version.clone()))).collect();
        let mut n = 0;
        for m in self.installed() {
            if !wanted.contains(&(m.name.clone(), m.version.clone())) && std::fs::remove_dir_all(self.version_dir(&m.name, &m.version)).is_ok() {
                n += 1;
            }
        }
        n
    }
}

/// Unpack a zip with no absolute paths, no `..`, no symlinks, no duplicate
/// paths, at most 20 000 entries and 1 GiB.
pub fn unzip_safe(archive: &Path, dest: &Path) -> Result<()> {
    let bad = |m: &str| Error::Other(format!("{}: {m}", archive.display()));
    let f = std::fs::File::open(archive)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| bad(&e.to_string()))?;
    if z.len() > MAX_ENTRIES {
        return Err(bad("too many files"));
    }
    std::fs::create_dir_all(dest)?;
    let mut total = 0u64;
    let mut seen = HashSet::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| bad(&e.to_string()))?;
        let rel = e.enclosed_name().ok_or_else(|| bad("unsafe path"))?;
        if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(bad("symlinks are not allowed"));
        }
        let key = rel.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            return Err(bad("duplicate path"));
        }
        let out = dest.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        total += e.size();
        if total > MAX_UNPACKED {
            return Err(bad("unpacks to more than 1 GiB"));
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        let declared = e.size();
        let copied = std::io::copy(&mut (&mut e).take(declared + 1), &mut w)?;
        if copied > declared {
            return Err(bad("entry larger than declared"));
        }
        #[cfg(unix)]
        if let Some(mode) = e.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755));
        }
    }
    Ok(())
}

use std::io::Read;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (n, d) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn unzip_rejects_traversal() {
        let d = tempfile::tempdir().unwrap();
        let good = d.path().join("good.zip");
        make_zip(&good, &[("a/b.txt", b"hi"), ("c.txt", b"x")]);
        unzip_safe(&good, &d.path().join("out")).unwrap();
        assert_eq!(std::fs::read(d.path().join("out/a/b.txt")).unwrap(), b"hi");
        let evil = d.path().join("evil.zip");
        make_zip(&evil, &[("../escape.txt", b"no")]);
        assert!(unzip_safe(&evil, &d.path().join("out2")).is_err());
        assert!(!d.path().join("escape.txt").exists());
        let dup = d.path().join("dup.zip");
        make_zip(&dup, &[("A.txt", b"1"), ("a.txt", b"2")]);
        assert!(unzip_safe(&dup, &d.path().join("out3")).is_err());
    }
}
