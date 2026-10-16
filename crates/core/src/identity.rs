//! The device identity: one Ed25519 key per device, created on first use.
//! There is no password; the key *is* the login. A recovery code (the key's
//! seed in a readable form) restores it on a new installation. Several devices
//! of one player link to the same account with a link code instead.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct Identity {
    key: SigningKey,
}

/// Crockford base32 without padding; readable, no `I L O U`.
const B32: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn b32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for c in s.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = B32.iter().position(|x| *x as char == c)? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push(((buf >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

impl Identity {
    pub fn generate() -> Identity {
        Identity { key: SigningKey::from_bytes(&rand::random()) }
    }

    pub fn from_seed(seed: [u8; 32]) -> Identity {
        Identity { key: SigningKey::from_bytes(&seed) }
    }

    /// Load the key at `path`, creating it on first use.
    pub fn load_or_create(path: &Path) -> std::io::Result<Identity> {
        if let Ok(text) = std::fs::read_to_string(path) {
            let seed: [u8; 32] = hex::decode(text.trim())
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{} is not a Warpzone identity", path.display())))?;
            return Ok(Identity::from_seed(seed));
        }
        let id = Identity::generate();
        id.save(path)?;
        Ok(id)
    }

    /// The shared identity of this user (`paths::identity_file()`).
    pub fn shared() -> std::io::Result<Identity> {
        Identity::load_or_create(&crate::paths::identity_file())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::paths::write_atomic(path, hex::encode(self.key.to_bytes()).as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Public key, hex.
    pub fn public_hex(&self) -> String {
        hex::encode(self.key.verifying_key().to_bytes())
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.key
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.key.sign(msg).to_bytes()
    }

    /// `XXXX-XXXX-...`: the seed in base32 plus a 2-byte checksum. Whoever has
    /// it *is* this player — show it only on request and say so.
    pub fn recovery_code(&self) -> String {
        let seed = self.key.to_bytes();
        let mut data = seed.to_vec();
        data.extend_from_slice(&Sha256::digest(seed)[..2]);
        let s = b32_encode(&data);
        s.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
    }

    pub fn from_recovery_code(code: &str) -> Option<Identity> {
        let data = b32_decode(code)?;
        if data.len() != 34 {
            return None;
        }
        let seed: [u8; 32] = data[..32].try_into().ok()?;
        (Sha256::digest(seed)[..2] == data[32..]).then(|| Identity::from_seed(seed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_roundtrip() {
        let id = Identity::generate();
        let code = id.recovery_code();
        assert_eq!(Identity::from_recovery_code(&code).unwrap().public_hex(), id.public_hex());
        assert_eq!(Identity::from_recovery_code(&code.to_lowercase().replace('-', " ")).unwrap().public_hex(), id.public_hex());
        let mut bad = code.clone();
        bad.replace_range(0..1, if &code[0..1] == "A" { "B" } else { "A" });
        assert!(Identity::from_recovery_code(&bad).is_none());
    }

    #[test]
    fn persists() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("id.key");
        let a = Identity::load_or_create(&p).unwrap();
        let b = Identity::load_or_create(&p).unwrap();
        assert_eq!(a.public_hex(), b.public_hex());
    }
}
