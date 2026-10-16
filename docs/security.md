# Security & privacy

## What is stored

| Data | Where | Why |
|---|---|---|
| username, creation time | server | display, uniqueness |
| device public keys | server | login |
| ratings, match results, leaderboard entries | server | ranked play |
| mods (archives, manifests), reports, moderator notes | server | the archive |
| rooms, candidate addresses, relay bindings | server memory only | while you play |
| identity key (private) | your device, mode 0600 | your login |

Not stored: e-mail, passwords, IP addresses (used in memory for connections
and rate limits, never logged or written), chat (there is none), play time,
hardware. Account deletion removes the account, its devices, ratings and
scores; mods it published stay available without an owner because other mods
and saved games may depend on them.

## Threat model

| Threat | Mitigation |
|---|---|
| Someone joins a private game by knowing the host's address | Peers accept only server-signed tickets for the right room, account and device key, plus a signature by that key over the handshake. |
| Eavesdropping or tampering on peer traffic, including by the relay | X25519 per connection, HKDF-derived keys per direction bound to the room, ChaCha20-Poly1305 with a replay window. The relay sees ciphertext. |
| The UDP port used for reflection or amplification attacks | The server never answers with more bytes than it received; relay data is forwarded only between the two addresses that bound a channel with its secret tokens; per-IP limits on STUN and binds; bandwidth caps per channel and in total. |
| Brute-forcing join or link codes | 31^6 / 31^8 code spaces, per-IP guess limits, codes die with their room or after 10 minutes. |
| Spam accounts | per-IP registration limit, reserved names, a private blocked-word list, moderator renames. |
| Malicious or infringing mods | Every version is reviewed before it is public; archives are never opened by the server; clients unpack with checks against path traversal, symlinks, duplicate names (case-insensitive), more than 20 000 entries and more than 1 GiB; games whose mods may contain code are marked. |
| A compromised Warpzone server serving malicious game binaries | The launcher verifies downloads against GitHub's own digests (or checksum file), optionally minisign signatures, not against data from the Warpzone server alone. |
| Replay of a login signature against another instance | The signed login message includes the server's key. |
| Unauthorised moderation | Staff keys live in a file read by the server, not in the database; there is no API to change them. |

## Reporting a vulnerability

Please open a [security advisory](https://github.com/jarvis-jhk/warpzone/security/advisories/new)
on GitHub (private) rather than a public issue. Include steps to reproduce.
We answer quickly; fixes ship as a new release with a changelog entry.
