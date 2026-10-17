//! Sending crashes, problems and ideas to the server (`POST /feedback`).
//!
//! Fire-and-forget helpers for games, the launcher and `wz`. What goes out is
//! cleaned of the obvious personal bits first: the home directory and the
//! login name become `~` and `<user>`.

use crate::Result;
use crate::api::Api;
use warpzone_protocol::api::{FEEDBACK_DETAIL_MAX, FEEDBACK_TITLE_MAX, FeedbackRequest, FeedbackResponse};

/// `<program> <version> (<os> <arch>)`, for [`FeedbackRequest::client`].
pub fn client_name(program: &str, version: &str) -> String {
    format!("{program} {version} ({} {})", std::env::consts::OS, std::env::consts::ARCH)
}

/// Replace the home directory and the login name in `text`.
pub fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    for h in [home.clone(), home.replace('\\', "/")] {
        if h.len() > 3 {
            out = out.replace(&h, "~");
        }
    }
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    if user.len() >= 2 { replace_word(&out, &user, "<user>") } else { out }
}

/// Replace `word` where it is not part of a longer word.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(word) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + word.len()..].chars().next();
        let inside = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..i]);
        out.push_str(if inside(before) || inside(after) { word } else { with });
        rest = &rest[i + word.len()..];
    }
    out.push_str(rest);
    out
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub(3);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// A request with title and detail scrubbed and cut to the server's limits
/// (the detail keeps its end, where a log's last lines are).
pub fn request(game: &str, kind: &str, title: &str, detail: &str, version: Option<&str>, client: &str) -> FeedbackRequest {
    let title = scrub(title.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("(no title)"));
    let mut detail = scrub(detail);
    if detail.len() > FEEDBACK_DETAIL_MAX {
        let mut start = detail.len() - (FEEDBACK_DETAIL_MAX - 8);
        while !detail.is_char_boundary(start) {
            start += 1;
        }
        detail = format!("…\n{}", &detail[start..]);
    }
    FeedbackRequest {
        game: game.into(),
        kind: kind.into(),
        title: clip(&title, FEEDBACK_TITLE_MAX),
        detail,
        version: version.map(|v| clip(v, 40)),
        client: Some(clip(client, 120)),
    }
}

/// Send now and wait for the answer.
pub fn send(api: &Api, req: &FeedbackRequest) -> Result<FeedbackResponse> {
    api.feedback(req)
}

/// Send from a background thread; failures are only printed to stderr.
pub fn send_in_background(api: std::sync::Arc<Api>, req: FeedbackRequest) {
    let _ = std::thread::Builder::new().name("wz-feedback".into()).spawn(move || {
        if let Err(e) = api.feedback(&req) {
            eprintln!("warpzone: feedback not sent: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_only() {
        assert_eq!(replace_word("jan: /x/jan/y january jan_x", "jan", "<user>"), "<user>: /x/<user>/y january jan_x");
    }

    #[test]
    fn requests_fit_the_limits() {
        let r = request("g", "bug", "\n\nfirst line\nsecond", &"x".repeat(FEEDBACK_DETAIL_MAX * 2), Some("1.0"), "t");
        assert_eq!(r.title, "first line");
        assert!(r.detail.len() <= FEEDBACK_DETAIL_MAX && r.detail.ends_with('x'));
        assert!(request("g", "bug", &"ä".repeat(200), "", None, "t").title.len() <= FEEDBACK_TITLE_MAX);
    }
}
