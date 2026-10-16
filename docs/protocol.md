# Wire protocol

Everything below is implemented in [`crates/protocol/src/wire.rs`](../crates/protocol/src/wire.rs),
[`ticket.rs`](../crates/protocol/src/ticket.rs) and
[`crates/core/src/net.rs`](../crates/core/src/net.rs). You only need this page
to implement Warpzone in another language or to debug a capture.

## The server's UDP port

One UDP socket (default 3478, dual-stack). Datagrams are told apart by their
first bytes; nothing else is answered.

| Datagram | Layout | Answer |
|---|---|---|
| STUN Binding request (RFC 8489 subset) | 20-byte header (type `0x0001`, magic `0x2112A442`), attributes | Binding success with `XOR-MAPPED-ADDRESS` — **only if the request is at least as long as the answer** (44 bytes for IPv6). Clients pad with a `SOFTWARE` attribute. |
| Relay bind | `"WZB1"` `channel:u32be` `side:u8` `token:[16]` | `"WZb1"` `channel:u32be` |
| Relay data | `"WZD1"` `channel:u32be` `payload` | forwarded byte-for-byte to the other side's bound address |

Anti-abuse: the server never sends more bytes than it received (no
amplification), forwards relay data only between the two addresses that bound
a channel with its secret tokens, applies a token bucket per channel and
direction (`relay_kbps`) and for the whole server (`relay_total_mbps`), and
rate-limits STUN and binds per source IP. Idle channels expire after three
minutes; channels die with their room.

A relay channel exists per (room, guest); the server creates it when it
issues the guest's ticket and tells each side its half (`RelayGrant`: channel
id, side 0 = host / 1 = guest, 16-byte token) in `PeerInfo`. Clients bind at
once (and every 20 s, which also keeps their NAT mapping to the server open)
and only *use* the channel when no direct path works.

## Candidates

A client's candidates are its host addresses (the local IPv4/IPv6 address the
OS would use towards the server, with the client's UDP port) and its
server-reflexive addresses from STUN, IPv6 first. They are sent to the server
with `create`/`join`/`heartbeat`/`candidates` and passed to the other side in
`PeerInfo` and `peer_candidates` events.

## Peer packets

Two packet types travel between peers, directly or inside `WZD1` relay frames:

```text
HELLO   0xF1  len:u16be  ticket[len]  eph_pub[32]  sig[64]
SEALED  0xF2  ctr:u64be  ciphertext+tag
```

**HELLO.** `ticket` is the sender's server-signed [ticket](#tickets);
`eph_pub` is a fresh X25519 public key for this connection; `sig` is an
Ed25519 signature by the ticket's device key over
`"warpzone-hello-v1" || eph_pub || ticket`. A receiver accepts a HELLO only
if the ticket verifies with the server key from `/api/v1/info`, has not
expired, names the expected room and account, and its `key` equals the
expected peer key — and the signature verifies with that key.

Both sides send HELLO to every candidate of the other every 200 ms (then every
second), which punches the NAT mappings from both ends at the same time; and,
after 1.5 s without a direct path, through the relay.

**Keys.** `shared = X25519(my_eph, peer_eph)`;
`HKDF-SHA256(salt = room id, ikm = shared)` expands
`"warpzone-v1 host->guest"` and `"warpzone-v1 guest->host"` into two 32-byte
keys; each side sends with its direction's key. A new HELLO with a different
ephemeral key (the peer restarted) re-keys and resets the reliable stream.

**SEALED.** ChaCha20-Poly1305 with nonce `0x00000000 || ctr:u64be`; `ctr`
starts at 1 and increases per packet. Receivers keep a 64-packet replay
window. A path (an address, or the relay) is considered working when an
authenticated SEALED packet arrives over it; a direct path replaces the
relay as soon as one works.

## Inside SEALED

The plaintext starts with a kind byte:

| Kind | Body | Meaning |
|---|---|---|
| `1` DATA | `channel:u8 payload` | unreliable datagram for the game |
| `2` REL | `seq:u32be channel:u8 payload` | reliable, ordered message (sequence numbers start at 1) |
| `3` ACK | `cum:u32be mask:u32be` | everything ≤ `cum` received; bit *i* of `mask` = `cum + 2 + i` received |
| `4` PING | `t:u64be` (sender's ms clock) | keepalive (every 2 s of silence) and RTT probe |
| `5` PONG | `t:u64be` (echoed) | RTT sample |
| `6` BYE | — | the peer leaves |

Reliable messages use a window of 256 unacknowledged messages, retransmit
after `max(2 × RTT, 150 ms)`, and are delivered in order. A peer with no
packet for 15 s (or never connected within 30 s) is lost.

Payloads are at most 1100 bytes so that payload, encryption and relay framing
stay under the IPv6 minimum MTU of 1280.

## Tickets

```text
base64url(json) "." base64url(ed25519_signature)
signature over: "warpzone-ticket-v1\n" || json
json: {"v":1,"room","game","account","username","key","role":"host|guest","issued","expires"}
```

Signed with the server key; valid 6 hours. The server issues one only after
the room's access rules allowed the member in.

## Tier-0 tunnel

The launcher's tunnel for games without Warpzone support uses two reserved
channels: **251** (reliable, JSON lobby messages: `state`, `start`, `ready`,
`ended`) and **250** (unreliable frames `dst:i64be src:i64be payload`). Each
machine opens one loopback UDP socket per other player; the game sends to
`127.0.0.1:<that port>`, the frame goes to the host, which delivers it or
forwards it to the destination guest; the destination's launcher sends it to
its game from the socket that represents the source player, so the game sees a
stable address per player. See [`crates/core/src/tunnel.rs`](../crates/core/src/tunnel.rs).
