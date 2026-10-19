//! How a game learns what the launcher (or an invite link) wants it to do.
//!
//! The launcher starts a game with
//!
//! * `--warpzone-join <room>` (or `--warpzone-join=<room>`): join that room.
//!   The launcher has already joined it with the same shared identity, so the
//!   game's join succeeds without a code.
//! * `--warpzone-host`: open the game's own host screen.
//! * `--warpzone-code <CODE>`: join a private game by its 6-letter code (the
//!   Android launcher has no account of its own, so it cannot look the code
//!   up first and hands it to the game). Together with `--warpzone-join` it
//!   is the code for that room (a listed room that wants its code).
//!
//! and sets `WARPZONE_LAUNCHED=1` and `WARPZONE_SERVER`. The environment
//! variables `WARPZONE_JOIN=<room>` and `WARPZONE_HOST=1` work too. On
//! Android the same arguments arrive as the string-array extra
//! `warpzone.args` of the launch intent. Once the player has a name there,
//! the Android launcher also passes its key as the string extra
//! `warpzone.identity`: give it to [`adopt_identity`] before creating the
//! client, so every Android game plays as that player (on the desktop all
//! games read the one shared identity file anyway).

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Intent {
    pub join: Option<String>,
    pub secret: Option<String>,
    pub host: bool,
    pub launched: bool,
    /// A private game's code (`--warpzone-code`), normalised.
    pub code: Option<String>,
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
        } else if a == "--warpzone-code" {
            i.code = it.next().map(warpzone_protocol::names::normalize_code).filter(|c| !c.is_empty());
        } else if let Some(v) = a.strip_prefix("--warpzone-code=") {
            i.code = Some(warpzone_protocol::names::normalize_code(v)).filter(|c| !c.is_empty());
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

/// Make `seed_hex` (the `warpzone.identity` extra of the Android launcher)
/// this game's identity: written to the identity file unless it is already
/// the one there. A different key that was there is kept as
/// `identity.key.old`. Returns whether the key changed; call it before the
/// client is created, which reads the identity once.
pub fn adopt_identity(seed_hex: &str) -> std::io::Result<bool> {
    adopt_identity_at(&crate::paths::identity_file(), seed_hex)
}

fn adopt_identity_at(path: &std::path::Path, seed_hex: &str) -> std::io::Result<bool> {
    let seed: [u8; 32] = hex::decode(seed_hex.trim())
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a Warpzone identity"))?;
    let new = crate::identity::Identity::from_seed(seed);
    if let Ok(old) = std::fs::read_to_string(path) {
        if old.trim().eq_ignore_ascii_case(seed_hex.trim()) {
            return Ok(false);
        }
        let mut keep = path.as_os_str().to_owned();
        keep.push(".old");
        std::fs::rename(path, keep)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    new.save(path)?;
    Ok(true)
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
    fn adopts_the_launchers_identity() {
        let dir = std::env::temp_dir().join(format!("wz-adopt-{}", std::process::id()));
        let p = dir.join("identity.key");
        let own = crate::identity::Identity::load_or_create(&p).unwrap();
        let seed = "11".repeat(32);
        assert!(adopt_identity_at(&p, &seed).unwrap());
        assert!(!adopt_identity_at(&p, &seed).unwrap());
        assert_eq!(crate::identity::Identity::load_or_create(&p).unwrap().public_hex(), crate::identity::Identity::from_seed([0x11; 32]).public_hex());
        let old = crate::identity::Identity::load_or_create(&dir.join("identity.key.old")).unwrap();
        assert_eq!(old.public_hex(), own.public_hex());
        assert!(adopt_identity_at(&p, "nope").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn args() {
        let i = from_args(&["game", "--warpzone-join", "room1", "--warpzone-host"]);
        assert_eq!(i.join.as_deref(), Some("room1"));
        assert!(i.host);
        let i = from_args(&["game", "--warpzone-join=warpzone://join/r2#sec"]);
        assert_eq!((i.join.as_deref(), i.secret.as_deref()), (Some("r2"), Some("sec")));
        let i = from_args(&["game", "--warpzone-code", "k7qm3x"]);
        assert_eq!((i.join, i.code.as_deref()), (None, Some("K7QM3X")));
    }
}
