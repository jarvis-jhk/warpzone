# Self-hosting: run your own ecosystem

Everything in Warpzone is open source (MIT or Apache-2.0). You can run an
instance for your own games, your community or your LAN party — with your own
descriptors, moderators and website — and ship the launcher and library
pointed at it.

* [What you need](#what-you-need)
* [Quick start with Docker](#quick-start-with-docker)
* [Reverse proxy and TLS](#reverse-proxy-and-tls)
* [Configuration](#configuration)
* [Games, staff, the website](#games-staff-the-website)
* [Pointing clients at your instance](#pointing-clients-at-your-instance)
* [Operations](#operations)
* [Without Docker](#without-docker)

## What you need

* A machine reachable from the internet with a public IPv4 address — ideally
  IPv6 too (many home connections behind DS-Lite/CGNAT only connect directly
  over IPv6). The server is light: about 20 MB of RAM and almost no CPU;
  relayed traffic is the only real cost (capped by configuration).
* A domain name for HTTPS (`warpzone.example.org`).
* **One UDP port open** (default 3478) for STUN and the relay, on IPv4 and
  IPv6, plus HTTPS (443) through your reverse proxy.
* Docker with Compose, or any Linux machine for the static binary.

## Quick start with Docker

```sh
git clone https://github.com/jarvis-jhk/warpzone
cd warpzone/deploy
cp env.example .env                                 # version + its SHA-256
cp config/server.example.toml config/server.toml    # public_url, udp_public
cp config/staff.example.toml  config/staff.toml     # your moderator key
mkdir -p config/games && cp ../games/*.toml config/games/   # or your own
mkdir -p data && sudo chown 10001:10001 data
docker compose up -d --build
curl -s http://127.0.0.1:2998/healthz               # ok
```

`.env` holds `WARPZONE_VERSION` and `WARPZONE_SHA256` (from the release's
`SHA256SUMS`, line `warpzone-server-<version>-x86_64-linux-musl`);
`Dockerfile.release` downloads that binary and checks it. Alternatives:
`Dockerfile.local` (a binary you built) and `Dockerfile` (build from source:
`docker build -f deploy/Dockerfile -t warpzone-server .` from the repository
root). Releases also publish `ghcr.io/jarvis-jhk/warpzone:<version>`.

The compose file uses **host networking on purpose**: STUN must see each
client's real address, and Docker's port publishing (its userland proxy, for
IPv6 in particular) would replace it with the proxy's — NAT traversal would
then silently fail for everyone. The container runs as an unprivileged user
with a read-only root filesystem, no capabilities and a memory cap; it only
writes `data/`.

## Reverse proxy and TLS

The daemon speaks plain HTTP on `http_listen` (default `127.0.0.1:2998`).
Put a TLS reverse proxy in front: [`deploy/nginx/warpzone.conf`](../deploy/nginx/warpzone.conf)
is a complete nginx example (certbot `--nginx` can manage the certificate).
Things that matter:

* `client_max_body_size` above your largest mod limit (uploads).
* `proxy_read_timeout` above 30 s — `/api/v1/events` is a long poll.
* `X-Forwarded-For` set, and `trust_forwarded_for = true` in the config, so
  rate limits apply per client and not to the proxy.

Caddy equivalent:

```text
warpzone.example.org {
  request_body { max_size 80MB }
  reverse_proxy 127.0.0.1:2998 { flush_interval -1 }
}
```

## Configuration

`config/server.toml` ([example](../deploy/config/server.example.toml)):

| Key | Default | Meaning |
|---|---|---|
| `name` | `Warpzone` | shown on the website and in `/info` |
| `public_url` | — | your HTTPS URL, no trailing slash |
| `http_listen` | `127.0.0.1:2998` | HTTP listener behind the proxy |
| `udp_listen` | `[::]:3478` | UDP socket; `[::]` is dual-stack |
| `udp_public` | from `public_url` | UDP endpoints clients use; list literal `ip:port` per family when DNS has no AAAA but the host has IPv6 |
| `data_dir` | — | database, server key, mod archives, replays |
| `games_dir` | — | descriptors, re-read on change |
| `staff_file` | — | moderators/reviewers/blocked words, re-read on change |
| `notify_url` | none | plain-text POST per mod submission and report (ntfy works) |
| `trust_forwarded_for` | `false` | use `X-Forwarded-For` (only behind your proxy) |
| `launcher` | none | GitHub release source for the launcher's download buttons (same shape as a descriptor's `[install]`) |
| `source_url` | the project | where the website links for source and docs |
| `legal_url` | none | imprint/contact page linked from every footer |
| `fault_reports` | none | `{ url = "…", lane = "warpzone" }`: the server's own faults (500s, errors in background jobs, a failed start, panics) as a JSON POST `{"lane","kind":"bug","title","detail","source"}` each; identical faults once per 6 h, at most 20 per hour |

`[limits]`:

| Key | Default | Meaning |
|---|---|---|
| `max_room_players` | 64 | upper bound for any game |
| `max_metadata_bytes` | 1024 | room metadata size |
| `rooms_per_account` | 3 | rooms one player may host at once |
| `mod_store_gb` | 10 | total size of all stored mod archives |
| `relay_kbps` | 2000 | relay bandwidth per channel and direction |
| `relay_total_mbps` | 100 | relay bandwidth for the whole server |
| `registrations_per_ip_per_day` | 5 | new usernames per address |
| `logins_per_ip_per_minute` | 30 | |
| `code_guesses_per_ip_per_minute` | 10 | join and link codes |
| `uploads_per_account_per_day` | 10 | mod versions |
| `reports_per_account_per_day` | 20 | |
| `max_replay_kb` | 512 | leaderboard replay files |

## Games, staff, the website

* **Games**: put [descriptors](descriptors.md) into `games_dir`. Use the
  project's (`games/`), write your own, or mix. Changes apply within a minute.
* **Staff**: generate a key on your machine (`wz whoami` creates one and
  prints it) and put it into `staff.toml` under `moderators`. See
  [moderation](moderation.md).
* **Website**: the start page lists your games with download buttons from
  their GitHub releases, open rooms, ladders, leaderboards and mods. Style and
  logo are compiled in (`site/`); fork and rebuild to change them.
* **Server key**: created in `data_dir/server.key` on first start. It signs
  join tickets; keep it with your backups (a new key only means that tickets
  issued before the change stop working).

## Pointing clients at your instance

* **At run time**: `WARPZONE_SERVER=https://warpzone.example.org` for the
  launcher, `wz` and every game; the launcher also has a server field in
  Settings; games can pass `server` in `wz_config`.
* **At build time** (your own launcher and library builds):
  `WARPZONE_DEFAULT_SERVER=https://warpzone.example.org cargo build --release`.
* Players' identities are per instance: the same device key can register on
  several instances; usernames, ratings and mods are separate.

## Operations

* **Health**: `GET /healthz` (`ok`), `GET /api/v1/stats` (rooms, players,
  relay channels and bytes, STUN requests, uptime). The compose file has a
  health check.
* **Logs**: stderr, one line per event; `WARPZONE_LOG=debug|info|warn`.
  Client IP addresses are never logged.
* **Backups**: everything persistent is in `data_dir`: `warpzone.db` (SQLite
  in WAL mode — back up with `sqlite3 warpzone.db ".backup copy.db"` or stop
  the container briefly), `blobs/` (mod archives), `replays/`, `server.key`.
* **Upgrades**: set the new `WARPZONE_VERSION`/`WARPZONE_SHA256` in `.env`,
  `docker compose up -d --build`. The schema migrates itself. Rooms in
  progress are dropped; clients host again.
* **Capacity**: rooms and relay channels are in memory (bytes each); SQLite
  handles thousands of matches per hour. Relay bandwidth is the limit to
  watch — `relay_total_mbps` caps it.

## Without Docker

```sh
curl -LO https://github.com/jarvis-jhk/warpzone/releases/download/v0.1.0/warpzone-server-0.1.0-x86_64-linux-musl
sha256sum warpzone-server-0.1.0-x86_64-linux-musl   # compare with SHA256SUMS
install -m 755 warpzone-server-0.1.0-x86_64-linux-musl /usr/local/bin/warpzone-server
warpzone-server --config /etc/warpzone/server.toml
```

A systemd unit:

```ini
[Unit]
Description=Warpzone server
After=network-online.target

[Service]
ExecStart=/usr/local/bin/warpzone-server --config /etc/warpzone/server.toml
DynamicUser=yes
StateDirectory=warpzone
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
NoNewPrivileges=yes
MemoryMax=256M
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

(with `data_dir = "/var/lib/warpzone"`). The binary is static (musl) and
also available for aarch64.
