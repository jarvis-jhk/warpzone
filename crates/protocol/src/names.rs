//! Validation and generation of the short identifiers players see or type.

use rand::Rng;

/// Alphabet of join codes and link codes: no `0/O`, `1/I/L` look-alikes, so a
/// code read aloud or typed with a controller cannot be mistyped that way.
pub const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Length of a room join code (`K7QM3X`). 31^6 ≈ 887 million combinations; the
/// server rate-limits guesses, so brute force is not practical.
pub const ROOM_CODE_LEN: usize = 6;

/// Length of an account link code (adding a second device).
pub const LINK_CODE_LEN: usize = 8;

/// A fresh random code of `len` characters from [`CODE_ALPHABET`].
pub fn generate_code(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

/// Normalise what a player typed: upper case, spaces and dashes dropped.
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Whether `code` (already normalised) could be a code of `len` characters.
pub fn is_code(code: &str, len: usize) -> bool {
    code.len() == len && code.bytes().all(|b| CODE_ALPHABET.contains(&b))
}

/// Usernames: 3–16 ASCII letters, digits, `_` or `-`, starting with a letter or
/// digit. ASCII only, so no two names can look alike through Unicode
/// confusables; uniqueness is case-insensitive (enforced by the server).
pub fn valid_username(name: &str) -> bool {
    let b = name.as_bytes();
    (3..=16).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

/// Mod names: 1–48 characters of `a-z 0-9 _ -`, starting with a letter or digit.
pub fn valid_mod_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=48).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// Game ids: 2–40 characters of `a-z 0-9 -`, starting with a letter or digit.
pub fn valid_game_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=40).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Short identifiers chosen by game authors (queue ids, board ids, categories,
/// presets, metadata keys): 1–32 characters of `a-z 0-9 _ - .`.
pub fn valid_slug(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=32).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.'))
}

/// Language tags in room audiences: ISO 639-1 two-letter codes (`en`, `de`).
pub fn valid_language(tag: &str) -> bool {
    tag.len() == 2 && tag.bytes().all(|c| c.is_ascii_lowercase())
}

/// Names nobody may register: they would impersonate the service or its staff.
pub const RESERVED_USERNAMES: &[&str] = &[
    "admin", "administrator", "moderator", "mod", "root", "system", "server", "warpzone",
    "support", "staff", "official", "jarvis", "host", "guest", "anonymous", "null", "nobody",
];

/// Whether `name` is reserved (case-insensitive).
pub fn reserved_username(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    RESERVED_USERNAMES.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        let c = generate_code(ROOM_CODE_LEN);
        assert!(is_code(&c, ROOM_CODE_LEN));
        assert_eq!(normalize_code(" k7q-m3x "), "K7QM3X");
        assert!(!is_code("K7QM3O", 6), "O is not in the alphabet");
    }

    #[test]
    fn usernames() {
        assert!(valid_username("Jan_99"));
        assert!(!valid_username("ab"));
        assert!(!valid_username("_lead"));
        assert!(!valid_username("Jän"));
        assert!(reserved_username("Admin"));
    }

    #[test]
    fn ids() {
        assert!(valid_mod_name("cave-tiles"));
        assert!(!valid_mod_name("Cave"));
        assert!(valid_game_id("super-mario-land-port"));
        assert!(valid_slug("any-1.1"));
        assert!(valid_language("de"));
        assert!(!valid_language("deu"));
    }
}
