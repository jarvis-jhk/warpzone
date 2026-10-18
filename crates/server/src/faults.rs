//! Fault reports: the server's own failures (a 500, an error in a background
//! job, a failed start, a panic) go out as one JSON POST each to
//! `[fault_reports] url`, instead of only into a log nobody reads.
//!
//! Fire-and-forget: a report never blocks a request. Identical faults are
//! sent once per [`REPEAT`], and at most [`PER_HOUR`] reports leave per hour,
//! so a fault loop cannot flood the receiver. Only the error text goes out —
//! never request bodies, tokens or addresses.

use crate::config::FaultReports;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const REPEAT: Duration = Duration::from_secs(6 * 3600);
const PER_HOUR: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Reporter {
    cfg: FaultReports,
    source: String,
    sent: Mutex<(HashMap<String, Instant>, Vec<Instant>)>,
}

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Turn reporting on (once, after the config is loaded) and report panics:
/// release builds abort on panic, so the hook sends before the process ends.
pub fn init(cfg: Option<FaultReports>, public_url: &str) {
    let Some(cfg) = cfg else { return };
    let host = public_url.split("://").nth(1).unwrap_or(public_url).split(['/', ':']).next().unwrap_or("");
    let source = format!("warpzone-server {} on {host}", crate::VERSION);
    if REPORTER.set(Reporter { cfg, source, sent: Mutex::default() }).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let trace = std::backtrace::Backtrace::force_capture();
        report_now(&format!("panic: {msg}"), &format!("panic in thread {thread}{at}: {msg}\n\n{trace}"));
    }));
}

/// Report in the background (from the log, request handlers, tasks).
pub fn report(title: &str, detail: &str) {
    if let Some((r, body)) = prepare(title, detail) {
        let _ = std::thread::Builder::new().name("fault-report".into()).spawn(move || r.send(body));
    }
}

/// Report and wait for it (at most a few seconds): for faults the process
/// does not survive.
pub fn report_now(title: &str, detail: &str) {
    if let Some((r, body)) = prepare(title, detail) {
        r.send(body);
    }
}

fn prepare(title: &str, detail: &str) -> Option<(&'static Reporter, serde_json::Value)> {
    let r = REPORTER.get()?;
    let title = clip(title.lines().next().unwrap_or(""), 160);
    if !r.admit(&title) {
        return None;
    }
    let body = serde_json::json!({
        "lane": r.cfg.lane,
        "kind": "bug",
        "title": format!("warpzone-server: {title}"),
        "detail": clip(detail, 8000),
        "source": r.source,
    });
    Some((r, body))
}

impl Reporter {
    /// Whether a fault with this title may go out now (and count it).
    fn admit(&self, title: &str) -> bool {
        let mut sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        let (seen, recent) = &mut *sent;
        let now = Instant::now();
        seen.retain(|_, t| now.duration_since(*t) < REPEAT);
        recent.retain(|t| now.duration_since(*t) < Duration::from_secs(3600));
        if seen.contains_key(title) || recent.len() >= PER_HOUR {
            return false;
        }
        seen.insert(title.to_string(), now);
        recent.push(now);
        true
    }

    fn send(&self, body: serde_json::Value) {
        let agent: ureq::Agent = crate::http::http_config().timeout_global(Some(TIMEOUT)).build().into();
        if let Err(e) = agent.post(&self.cfg.url).send_json(&body) {
            // Not log::error: that would report the failed report.
            eprintln!("{} WARN  fault report failed: {e}", warpzone_protocol::now());
        }
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_char_boundaries() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("äöü", 3), "ä…");
    }

    #[test]
    fn repeats_and_floods_are_held_back() {
        let r = Reporter { cfg: FaultReports { url: String::new(), lane: "x".into() }, source: String::new(), sent: Mutex::default() };
        assert!(r.admit("a"));
        assert!(!r.admit("a"));
        for i in 1..PER_HOUR {
            assert!(r.admit(&i.to_string()));
        }
        assert!(!r.admit("one too many"));
    }
}
