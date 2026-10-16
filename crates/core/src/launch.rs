//! How a game learns what the launcher (or an invite link) wants it to do.
//!
//! The launcher starts a game with
//!
//! * `--warpzone-join <room>` (or `--warpzone-join=<room>`): join that room.
//!   The launcher has already joined it with the same shared identity, so the
//!   game's join succeeds without a code.
//! * `--warpzone-host`: open the game's own host screen.
//!
//! and sets `WARPZONE_LAUNCHED=1` and `WARPZONE_SERVER`. The environment
//! variables `WARPZONE_JOIN=<room>` and `WARPZONE_HOST=1` work too.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Intent {
    pub join: Option<String>,
    pub secret: Option<String>,
    pub host: bool,
    pub launched: bool,
}

/// Read the intent from command-line arguments and the environment.
pub fn from_args<S: AsRef<str>>(args: &[S]) -> Intent {
    let mut i = Intent { launched: std::env::var("WARPZONE_LAUNCHED").is_ok_and(|v| v == "1"), ..Default::default() };
    let mut it = args.iter().map(|s| s.as_ref());
    while let Some(a) = it.next() {
        if a == "--warpzone-join" {
            i.join = it.next().map(String::from);
        } else if let Some(v) = a.strip_prefix("--warpzone-join=") {
            i.join = Some(v.into());
        } else if a == "--warpzone-host" {
            i.host = true;
        } else if let Some((room, secret)) = parse_invite(a) {
            i.join = Some(room);
            i.secret = secret;
        }
    }
    if i.join.is_none()
        && let Ok(r) = std::env::var("WARPZONE_JOIN")
        && !r.is_empty()
    {
        i.join = Some(r);
    }
    if std::env::var("WARPZONE_HOST").is_ok_and(|v| v == "1") {
        i.host = true;
    }
    // A join value may itself be an invite link.
    if let Some(j) = i.join.clone()
        && let Some((room, secret)) = parse_invite(&j)
    {
        i.join = Some(room);
        i.secret = secret.or(i.secret);
    }
    i
}

/// `warpzone://join/<room>#<secret>` or `https://<host>/join/<room>#<secret>`.
pub fn parse_invite(s: &str) -> Option<(String, Option<String>)> {
    let rest = s.strip_prefix("warpzone://join/").or_else(|| s.split_once("://").and_then(|(_, r)| r.split_once("/join/").map(|(_, r)| r)))?;
    let (room, secret) = match rest.split_once('#') {
        Some((r, s)) => (r, Some(s.to_string()).filter(|s| !s.is_empty())),
        None => (rest, None),
    };
    let room = room.trim_end_matches('/');
    (!room.is_empty() && room.len() <= 16 && room.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| (room.to_string(), secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invites() {
        assert_eq!(parse_invite("warpzone://join/ab12cd#s3"), Some(("ab12cd".into(), Some("s3".into()))));
        assert_eq!(parse_invite("https://warpzone.example.org/join/ab12cd"), Some(("ab12cd".into(), None)));
        assert_eq!(parse_invite("warpzone://join/../x"), None);
    }

    #[test]
    fn args() {
        let i = from_args(&["game", "--warpzone-join", "room1", "--warpzone-host"]);
        assert_eq!(i.join.as_deref(), Some("room1"));
        assert!(i.host);
        let i = from_args(&["game", "--warpzone-join=warpzone://join/r2#sec"]);
        assert_eq!((i.join.as_deref(), i.secret.as_deref()), (Some("r2"), Some("sec")));
    }
}
