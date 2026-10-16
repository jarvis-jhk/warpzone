//! Installing and updating games from their GitHub releases.
//!
//! The Warpzone server only says *which* release is current. The installer
//! asks GitHub itself for the asset, its SHA-256 digest and the release's
//! checksum file, so a compromised Warpzone server cannot hand out other
//! binaries. Descriptors with a `minisign_key` additionally require a valid
//! `<asset>.minisig` signature.
//!
//! Layout: `<home>/games/<game>/<version>/` (unpacked release), `current`
//! (the version that runs) and `.warpzone-managed` in each version directory,
//! which tells the game's own updater to stay silent: the launcher updates
//! games it installed.

use crate::api::Api;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use warpzone_protocol::api::ReleaseInfo;
use warpzone_protocol::descriptor::{GameDescriptor, glob_match};

/// Marker file name in a launcher-managed install.
pub const MANAGED_MARKER: &str = ".warpzone-managed";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Installed {
    pub game: String,
    pub version: String,
    pub platform: String,
    pub asset: String,
    pub sha256: String,
    /// What vouched for the download: `github-digest`, `checksum-file` or
    /// `https-only` (the release publishes no checksum).
    #[serde(default)]
    pub verified_by: String,
    pub installed: i64,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// `linux`, `deck` (SteamOS) or `windows`.
pub fn platform() -> &'static str {
    if cfg!(windows) {
        return "windows";
    }
    if std::fs::read_to_string("/etc/os-release").is_ok_and(|s| s.lines().any(|l| l == "ID=steamos" || l == "ID=\"steamos\"")) {
        return "deck";
    }
    "linux"
}

/// Asset for this platform (the Deck falls back to the Linux build).
pub fn asset_for<'a>(release: &'a ReleaseInfo, platform: &str) -> Option<(&'a str, &'a warpzone_protocol::api::AssetInfo)> {
    release
        .assets
        .get_key_value(platform)
        .or_else(|| if platform == "deck" { release.assets.get_key_value("linux") } else { None })
        .map(|(k, v)| (k.as_str(), v))
}

pub fn installed(game: &str) -> Option<Installed> {
    let dir = crate::paths::game_dir(game);
    let v = std::fs::read_to_string(dir.join("current")).ok()?;
    let vdir = dir.join(v.trim());
    let t = std::fs::read_to_string(vdir.join(MANAGED_MARKER)).ok()?;
    let mut i: Installed = serde_json::from_str(&t).ok()?;
    i.dir = vdir;
    Some(i)
}

/// SHA-256 of an asset as GitHub states it: the API's `digest` field, else
/// the release's checksum file.
pub fn github_digest(api: &Api, repo: &str, version: &str, asset: &str, checksums: Option<&str>) -> Result<Option<(String, &'static str)>> {
    for tag in [format!("v{version}"), version.to_string()] {
        let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
        let Ok(bytes) = api.fetch(&url, 4 * 1024 * 1024) else { continue };
        let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| Error::Other(e.to_string()))?;
        let assets = v["assets"].as_array().cloned().unwrap_or_default();
        if let Some(d) = assets.iter().find(|a| a["name"] == asset).and_then(|a| a["digest"].as_str())
            && let Some(h) = d.strip_prefix("sha256:")
        {
            return Ok(Some((h.to_string(), "github-digest")));
        }
        if let Some(pattern) = checksums
            && let Some(url) =
                assets.iter().find(|a| a["name"].as_str().is_some_and(|n| glob_match(pattern, n))).and_then(|a| a["browser_download_url"].as_str())
        {
            let text = String::from_utf8_lossy(&api.fetch(url, 1024 * 1024)?).to_string();
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(h), Some(n)) = (parts.next(), parts.next())
                    && n.trim_start_matches('*') == asset
                {
                    return Ok(Some((h.to_lowercase(), "checksum-file")));
                }
            }
        }
        return Ok(None);
    }
    Ok(None)
}

/// Replace the running launcher (or any self-contained tool) with the asset
/// of `release` for this platform: verified against GitHub, unpacked, and the
/// file named `binary` swapped in. Takes effect on the next start.
pub fn self_update(api: &Api, repo: &str, release: &ReleaseInfo, binary: &str) -> Result<String> {
    let exe = std::env::current_exe()?;
    let (_, asset) = asset_for(release, platform()).ok_or_else(|| Error::Other("no launcher build for this platform".into()))?;
    let digest = github_digest(api, repo, &release.version, &asset.name, Some("SHA256SUMS"))?;
    let dir = exe.parent().ok_or_else(|| Error::Other("cannot locate the launcher".into()))?;
    let staging = dir.join(".warpzone-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let file = staging.join(&asset.name);
    api.fetch_to(&asset.url, digest.as_ref().map(|d| d.0.as_str()), &file, &mut |_| {})?;
    let unpacked = staging.join("files");
    if asset.name.ends_with(".zip") {
        crate::modstore::unzip_safe(&file, &unpacked)?;
    } else {
        untar_stream(&mut std::fs::File::open(&file)?, &unpacked)?;
    }
    let mut found = None;
    let mut stack = vec![unpacked.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else if e.file_name().to_string_lossy() == binary {
                found = Some(e.path());
            }
        }
    }
    let new = found.ok_or_else(|| Error::Other(format!("{binary} not found in {}", asset.name)))?;
    // Windows cannot overwrite a running executable but can rename it.
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::copy(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
        let _ = std::fs::remove_file(&old);
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(release.version.clone())
}

/// Install (or update to) `release`. `progress(bytes)` while downloading.
pub fn install(api: &Api, desc: &GameDescriptor, release: &ReleaseInfo, progress: &mut dyn FnMut(u64)) -> Result<Installed> {
    let inst = desc.install.as_ref().ok_or_else(|| Error::Other(format!("{} has no installer source", desc.name)))?;
    let platform = platform();
    let (plat, asset) = asset_for(release, platform).ok_or_else(|| Error::Other(format!("{} has no build for {platform}", desc.name)))?;
    let digest = github_digest(api, &inst.github, &release.version, &asset.name, inst.checksums.as_deref())?;
    let gdir = crate::paths::game_dir(&desc.id);
    let staging = gdir.join(".staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let unpacked = staging.join("files");
    let is_tar = asset.name.ends_with(".tar.gz") || asset.name.ends_with(".tgz");
    let sha = if is_tar && inst.unpack != "file" && inst.minisign_key.is_none() {
        // Fastest path: download, hash and unpack in one stream. Nothing is
        // switched in unless the hash matches at the end.
        let sha = api.stream_url(&asset.url, &mut |r: &mut dyn std::io::Read| {
            let mut hr = HashingReader::new(r, progress);
            untar_stream(&mut hr, &unpacked)?;
            std::io::copy(&mut hr, &mut std::io::sink())?;
            Ok(hr.finish())
        })?;
        if let Some((want, _)) = &digest
            && !sha.eq_ignore_ascii_case(want)
        {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(Error::Other(format!("checksum mismatch for {}", asset.name)));
        }
        sha
    } else {
        let file = staging.join(&asset.name);
        let sha = api.fetch_to(&asset.url, digest.as_ref().map(|d| d.0.as_str()), &file, progress)?;
        if let Some(pk) = &inst.minisign_key {
            let sig_url = format!("{}.minisig", asset.url);
            let sig = String::from_utf8_lossy(&api.fetch(&sig_url, 64 * 1024)?).to_string();
            verify_minisign(pk, &sig, &std::fs::read(&file)?)?;
        }
        if inst.unpack == "file" {
            std::fs::create_dir_all(&unpacked)?;
            std::fs::rename(&file, unpacked.join(&asset.name))?;
        } else if asset.name.ends_with(".zip") {
            crate::modstore::unzip_safe(&file, &unpacked)?;
        } else if is_tar {
            untar_stream(&mut std::fs::File::open(&file)?, &unpacked)?;
        } else {
            std::fs::create_dir_all(&unpacked)?;
            std::fs::rename(&file, unpacked.join(&asset.name))?;
        }
        sha
    };
    if digest.is_none() {
        log::warn!("{}: GitHub states no checksum for {}; installed by its HTTPS download alone", desc.id, asset.name);
    }
    let info = Installed {
        game: desc.id.clone(),
        version: release.version.clone(),
        platform: plat.to_string(),
        asset: asset.name.clone(),
        sha256: sha,
        verified_by: digest.map(|d| d.1).unwrap_or("https-only").to_string(),
        installed: warpzone_protocol::now(),
        dir: PathBuf::new(),
    };
    std::fs::write(unpacked.join(MANAGED_MARKER), serde_json::to_vec_pretty(&info).unwrap())?;
    let dest = gdir.join(&release.version);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&unpacked, &dest)?;
    let previous = std::fs::read_to_string(gdir.join("current")).ok().map(|s| s.trim().to_string());
    crate::paths::write_atomic(&gdir.join("current"), release.version.as_bytes())?;
    if let Some(prev) = previous.as_deref() {
        crate::paths::write_atomic(&gdir.join("previous"), prev.as_bytes())?;
    }
    // Keep the current and the previous version (rollback), drop older ones.
    if let Ok(rd) = std::fs::read_dir(&gdir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() && n != release.version && Some(n.as_str()) != previous.as_deref() && !n.starts_with('.') {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    installed(&desc.id).ok_or_else(|| Error::Other("install did not complete".into()))
}

/// Go back to the version installed before the last update.
pub fn rollback(game: &str) -> Result<Installed> {
    let gdir = crate::paths::game_dir(game);
    let prev = std::fs::read_to_string(gdir.join("previous")).map_err(|_| Error::Other("no previous version to go back to".into()))?;
    let prev = prev.trim();
    if !gdir.join(prev).is_dir() {
        return Err(Error::Other("the previous version is gone".into()));
    }
    let cur = std::fs::read_to_string(gdir.join("current")).unwrap_or_default();
    crate::paths::write_atomic(&gdir.join("current"), prev.as_bytes())?;
    crate::paths::write_atomic(&gdir.join("previous"), cur.trim().as_bytes())?;
    installed(game).ok_or_else(|| Error::Other("rollback failed".into()))
}

pub fn uninstall(game: &str) -> Result<()> {
    let d = crate::paths::game_dir(game);
    if d.exists() {
        std::fs::remove_dir_all(d)?;
    }
    Ok(())
}

/// Resolve the descriptor's executable for this install (`*` matches one
/// path segment).
pub fn executable(desc: &GameDescriptor, inst: &Installed) -> Option<PathBuf> {
    let launch = desc.launch.as_ref()?;
    let pattern = launch.exec.get(&inst.platform).or_else(|| if inst.platform == "deck" { launch.exec.get("linux") } else { None })?;
    let mut dirs = vec![inst.dir.clone()];
    for seg in pattern.split('/') {
        let mut next = Vec::new();
        for d in &dirs {
            if seg.contains('*') {
                if let Ok(rd) = std::fs::read_dir(d) {
                    for e in rd.flatten() {
                        if glob_match(seg, &e.file_name().to_string_lossy()) {
                            next.push(e.path());
                        }
                    }
                }
            } else {
                let p = d.join(seg);
                if p.exists() {
                    next.push(p);
                }
            }
        }
        dirs = next;
    }
    dirs.into_iter().find(|p| p.is_file())
}

/// Start the game. `extra` are added after the descriptor's own `args`.
pub fn launch(desc: &GameDescriptor, inst: &Installed, extra: &[String]) -> Result<std::process::Child> {
    let exe = executable(desc, inst).ok_or_else(|| Error::Other(format!("cannot find the {} executable in {}", desc.name, inst.dir.display())))?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.current_dir(exe.parent().unwrap_or(&inst.dir));
    if let Some(l) = &desc.launch {
        cmd.args(&l.args);
    }
    cmd.args(extra);
    cmd.env("WARPZONE_LAUNCHED", "1");
    cmd.env("WARPZONE_SERVER", crate::server_url());
    Ok(cmd.spawn()?)
}

/// Hashes everything read through it and reports progress.
struct HashingReader<'a, 'b> {
    inner: &'a mut dyn std::io::Read,
    hash: sha2::Sha256,
    total: u64,
    progress: &'b mut dyn FnMut(u64),
}

impl<'a, 'b> HashingReader<'a, 'b> {
    fn new(inner: &'a mut dyn std::io::Read, progress: &'b mut dyn FnMut(u64)) -> Self {
        HashingReader { inner, hash: <sha2::Sha256 as sha2::Digest>::new(), total: 0, progress }
    }
    fn finish(self) -> String {
        hex::encode(sha2::Digest::finalize(self.hash))
    }
}

impl std::io::Read for HashingReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        sha2::Digest::update(&mut self.hash, &buf[..n]);
        self.total += n as u64;
        (self.progress)(self.total);
        Ok(n)
    }
}

/// Unpack a .tar.gz stream with path and link checks.
fn untar_stream(r: &mut dyn std::io::Read, dest: &Path) -> Result<()> {
    let mut t = tar::Archive::new(flate2::read::GzDecoder::new(r));
    std::fs::create_dir_all(dest)?;
    for e in t.entries()? {
        let mut e = e?;
        let path = e.path()?.to_path_buf();
        if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(Error::Other(format!("unsafe path in archive: {}", path.display())));
        }
        let kind = e.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            // Release tarballs carry library symlinks (libSDL2.so -> ...): allow
            // only relative ones that stay inside the archive.
            let target = e.link_name()?.map(|p| p.to_path_buf()).unwrap_or_default();
            if target.is_absolute() || target.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Err(Error::Other(format!("unsafe link in archive: {}", path.display())));
            }
        }
        e.unpack_in(dest)?;
    }
    Ok(())
}

/// Check a minisign signature. `public_key` is the base64 key line
/// (`RWQ...`) as `minisign -G` prints it.
fn verify_minisign(public_key: &str, sig: &str, data: &[u8]) -> Result<()> {
    let pk = minisign_verify::PublicKey::from_base64(public_key.trim()).map_err(|e| Error::Other(format!("bad minisign key: {e}")))?;
    let sig = minisign_verify::Signature::decode(sig).map_err(|e| Error::Other(format!("bad signature file: {e}")))?;
    pk.verify(data, &sig, false).map_err(|e| Error::Other(format!("release signature does not verify: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use warpzone_protocol::api::AssetInfo;

    #[test]
    fn deck_falls_back_to_linux() {
        let mut assets = BTreeMap::new();
        assets.insert("linux".to_string(), AssetInfo { name: "x.tar.gz".into(), url: "u".into(), size: 1 });
        let r = ReleaseInfo {
            version: "1".into(),
            name: "".into(),
            published: "".into(),
            prerelease: false,
            page: "".into(),
            assets,
            checksums: None,
            notes: String::new(),
        };
        assert_eq!(asset_for(&r, "deck").unwrap().0, "linux");
        assert!(asset_for(&r, "windows").is_none());
    }
}
