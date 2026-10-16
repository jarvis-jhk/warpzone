//! Join tickets.
//!
//! The server only hands out a ticket after the room's access check passed
//! (open / code / host approved / invite / match member). Peers exchange their
//! tickets in the handshake and accept each other only with a valid one, signed
//! by the server key from `GET /api/v1/info`. So knowing a host's IP address and
//! port is not enough to get into a private game.
//!
//! Format: `base64url(json) "." base64url(ed25519 signature)`, the signature
//! being over `"warpzone-ticket-v1\n" + json`.

use crate::api::Role;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"warpzone-ticket-v1\n";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Ticket {
    pub v: u8,
    pub room: String,
    pub game: String,
    pub account: i64,
    pub username: String,
    /// Device key (hex) that must sign this member's handshake.
    pub key: String,
    pub role: Role,
    pub issued: i64,
    pub expires: i64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TicketError {
    #[error("malformed ticket")]
    Malformed,
    #[error("ticket signature is not valid")]
    BadSignature,
    #[error("ticket has expired")]
    Expired,
}

impl Ticket {
    /// Sign with the server key.
    pub fn sign(&self, key: &SigningKey) -> String {
        let json = serde_json::to_vec(self).expect("ticket serialises");
        let mut msg = DOMAIN.to_vec();
        msg.extend_from_slice(&json);
        let sig = key.sign(&msg);
        format!("{}.{}", B64.encode(&json), B64.encode(sig.to_bytes()))
    }

    /// Check signature and expiry; `now` in Unix seconds.
    pub fn verify(token: &str, server_key: &VerifyingKey, now: i64) -> Result<Ticket, TicketError> {
        let (body, sig) = token.split_once('.').ok_or(TicketError::Malformed)?;
        let json = B64.decode(body).map_err(|_| TicketError::Malformed)?;
        let sig = B64.decode(sig).map_err(|_| TicketError::Malformed)?;
        let sig: [u8; 64] = sig.try_into().map_err(|_| TicketError::Malformed)?;
        let mut msg = DOMAIN.to_vec();
        msg.extend_from_slice(&json);
        server_key.verify(&msg, &Signature::from_bytes(&sig)).map_err(|_| TicketError::BadSignature)?;
        let t: Ticket = serde_json::from_slice(&json).map_err(|_| TicketError::Malformed)?;
        if t.expires < now {
            return Err(TicketError::Expired);
        }
        Ok(t)
    }

    /// Read the claims without checking anything (for display only).
    pub fn peek(token: &str) -> Option<Ticket> {
        let body = token.split_once('.')?.0;
        serde_json::from_slice(&B64.decode(body).ok()?).ok()
    }
}

/// Parse a hex Ed25519 public key.
pub fn parse_key(hex_key: &str) -> Option<VerifyingKey> {
    let b: [u8; 32] = hex::decode(hex_key).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&b).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn ticket() -> Ticket {
        Ticket { v: 1, room: "r".into(), game: "g".into(), account: 1, username: "a".into(), key: "00".repeat(32), role: Role::Guest, issued: 10, expires: 100 }
    }

    #[test]
    fn roundtrip() {
        let k = key();
        let s = ticket().sign(&k);
        assert_eq!(Ticket::verify(&s, &k.verifying_key(), 50).unwrap(), ticket());
        assert_eq!(Ticket::verify(&s, &k.verifying_key(), 101), Err(TicketError::Expired));
    }

    #[test]
    fn tamper() {
        let k = key();
        let s = ticket().sign(&k);
        let mut t = ticket();
        t.role = Role::Host;
        let forged = format!("{}.{}", B64.encode(serde_json::to_vec(&t).unwrap()), s.split_once('.').unwrap().1);
        assert_eq!(Ticket::verify(&forged, &k.verifying_key(), 50), Err(TicketError::BadSignature));
        let other = SigningKey::from_bytes(&[8u8; 32]);
        assert_eq!(Ticket::verify(&s, &other.verifying_key(), 50), Err(TicketError::BadSignature));
    }
}
