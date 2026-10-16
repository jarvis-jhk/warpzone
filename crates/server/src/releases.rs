//! Cache of each game's newest GitHub release, for the website's download
//! buttons and the launcher's installer. Refreshed every 15 minutes with
//! anonymous API calls (well inside GitHub's 60 requests/hour).

use crate::state::AppState;
use std::collections::BTreeMap;
use std::time::Duration;
use warpzone_protocol::api::{AssetInfo, ChangelogEntry, ReleaseInfo, UpdateInfo};
use warpzone_protocol::descriptor::{Install, glob_match};

/// Key of the launcher's own release in the cache.
pub const LAUNCHER: &str = "@launcher";

fn fetch(install: &Install) -> Result<(Option<ReleaseInfo>, Vec<ChangelogEntry>), String> {
    let url = format!("https://api.github.com/repos/{}/releases?per_page=10", install.github);
    let mut resp = ureq::get(&url)
        .header("User-Agent", "warpzone-server")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let list: serde_json::Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    let list = list.as_array().ok_or("unexpected GitHub response")?;
    let usable = |r: &&serde_json::Value| !r["draft"].as_bool().unwrap_or(false);
    let full = list.iter().filter(usable).find(|r| !r["prerelease"].as_bool().unwrap_or(false));
    let any = list.iter().find(usable);
    let pick = if install.prerelease { any } else { full };
    let history: Vec<ChangelogEntry> = list
        .iter()
        .filter(usable)
        .filter(|r| install.prerelease || !r["prerelease"].as_bool().unwrap_or(false))
        .map(|r| ChangelogEntry {
            version: r["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string(),
            name: r["name"].as_str().unwrap_or("").to_string(),
            published: r["published_at"].as_str().unwrap_or("").to_string(),
            prerelease: r["prerelease"].as_bool().unwrap_or(false),
            page: r["html_url"].as_str().unwrap_or("").to_string(),
            notes: r["body"].as_str().unwrap_or("").chars().take(20_000).collect(),
        })
        .collect();
    let Some(r) = pick else { return Ok((None, history)) };
    let assets: Vec<(&str, &str, u64)> = r["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| Some((x["name"].as_str()?, x["browser_download_url"].as_str()?, x["size"].as_u64().unwrap_or(0))))
                .collect()
        })
        .unwrap_or_default();
    let mut out = BTreeMap::new();
    for (platform, pattern) in &install.assets {
        if let Some((name, url, size)) = assets.iter().find(|(n, _, _)| glob_match(pattern, n)) {
            out.insert(platform.clone(), AssetInfo { name: name.to_string(), url: url.to_string(), size: *size });
        }
    }
    let checksums = install
        .checksums
        .as_ref()
        .and_then(|c| assets.iter().find(|(n, _, _)| glob_match(c, n)))
        .map(|(_, u, _)| u.to_string());
    Ok((Some(ReleaseInfo {
        notes: r["body"].as_str().unwrap_or("").chars().take(20_000).collect(),
        version: r["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string(),
        name: r["name"].as_str().unwrap_or("").to_string(),
        published: r["published_at"].as_str().unwrap_or("").to_string(),
        prerelease: r["prerelease"].as_bool().unwrap_or(false),
        page: r["html_url"].as_str().unwrap_or("").to_string(),
        assets: out,
        checksums,
    }), history))
}

pub async fn refresh(state: &AppState) {
    let mut jobs: Vec<(String, Install)> = state
        .games
        .read()
        .unwrap()
        .by_id
        .values()
        .filter_map(|g| g.install.clone().map(|i| (g.id.clone(), i)))
        .collect();
    if let Some(l) = &state.cfg.launcher {
        jobs.push((LAUNCHER.to_string(), l.clone()));
    }
    let results = tokio::task::spawn_blocking(move || jobs.into_iter().map(|(id, i)| (id, fetch(&i))).collect::<Vec<_>>())
        .await
        .unwrap_or_default();
    let mut cache = state.releases.write().unwrap();
    let mut logs = state.changelogs.write().unwrap();
    for (id, r) in results {
        match r {
            Ok((latest, history)) => {
                match latest {
                    Some(info) => cache.insert(id.clone(), info),
                    None => cache.remove(&id),
                };
                logs.insert(id, history);
            }
            Err(e) => log::warn!("release check for {id}: {e}"),
        }
    }
}

fn newer(a: &str, b: &str) -> bool {
    use warpzone_protocol::mods::parse_version;
    match (parse_version(a), parse_version(b)) {
        (Ok(x), Ok(y)) => x > y,
        _ => a != b,
    }
}

/// What changed since `current` (newest first) and whether an update exists.
pub fn updates(state: &crate::state::State, key: &str, current: &str) -> UpdateInfo {
    let latest = state.releases.read().unwrap().get(key).cloned();
    let changes: Vec<ChangelogEntry> = state
        .changelogs
        .read()
        .unwrap()
        .get(key)
        .map(|h| h.iter().filter(|e| current.is_empty() || newer(&e.version, current)).cloned().collect())
        .unwrap_or_default();
    let update_available = latest.as_ref().is_some_and(|l| !current.is_empty() && newer(&l.version, current));
    UpdateInfo { current: current.to_string(), update_available, latest, changes }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_order() {
        assert!(super::newer("0.1.10", "0.1.9"));
        assert!(!super::newer("0.1.9", "0.1.10"));
        assert!(!super::newer("1.0", "1.0.0"));
    }
}

pub async fn run(state: AppState) {
    loop {
        state.reload_games();
        refresh(&state).await;
        tokio::time::sleep(Duration::from_secs(900)).await;
    }
}
