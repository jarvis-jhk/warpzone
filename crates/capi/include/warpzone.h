/*
 * warpzone.h — the Warpzone client library for games.
 *
 * Online rooms, NAT traversal with relay fallback, encrypted peer channels,
 * ranked matchmaking, leaderboards, update checks and mod sync, for any game
 * written in C or C++. Link the static library for your platform
 * (libwarpzone.a / warpzone.lib) from the Warpzone release; no Rust toolchain
 * is needed. Guide: docs/integration.md. License: MIT OR Apache-2.0.
 *
 * Threading: wz_* functions must be called from one thread (your game loop).
 * The library runs two internal threads for HTTP; it never calls back into
 * your code. Call wz_poll() once per frame, then drain wz_next_event().
 *
 * Requests (host, join, queue, ...) return at once; their outcome arrives as
 * an event. Complex data travels as JSON strings in event.json, with the same
 * shapes as the HTTP API (docs/api.md).
 */
#ifndef WARPZONE_H
#define WARPZONE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define WZ_ABI_VERSION 1

/* Largest payload of one wz_send() datagram. */
#define WZ_MAX_DATAGRAM 1100

/* Peer channels 250-255 are reserved by Warpzone. */
#define WZ_RESERVED_CHANNELS 250

typedef struct wz_client wz_client;
typedef uint32_t wz_peer;

typedef enum wz_event_type {
    WZ_EVENT_NONE = 0,
    WZ_EVENT_LOGGED_IN = 1,       /* json {"account": {...} | null, "roles": [...]} */
    WZ_EVENT_REGISTERED = 2,      /* json account {"id","username","created"} */
    WZ_EVENT_ROOM_HOSTED = 3,     /* json {"room": RoomInfo, "code": str|null, "invite": str} */
    WZ_EVENT_ROOM_JOINED = 4,     /* json {"room": RoomInfo, "host": bool} */
    WZ_EVENT_KNOCKING = 5,        /* json {"room": RoomInfo} — waiting for the host */
    WZ_EVENT_KNOCK_DENIED = 6,    /* json {"room": id} */
    WZ_EVENT_KNOCK = 7,           /* host: json {"knock","account","username"} → wz_answer_knock() */
    WZ_EVENT_PEER_JOINED = 8,     /* peer; json {"account","username"} */
    WZ_EVENT_PEER_CONNECTED = 9,  /* peer; json {"relayed": bool} */
    WZ_EVENT_PEER_PATH = 10,      /* peer; json {"relayed": bool} */
    WZ_EVENT_PEER_DATA = 11,      /* peer, channel, reliable, data/len */
    WZ_EVENT_PEER_LEFT = 12,      /* peer; json {"account"} */
    WZ_EVENT_ROOM_CLOSED = 13,    /* json {"room"} */
    WZ_EVENT_KICKED = 14,         /* json {"room","banned"} */
    WZ_EVENT_ROOM_LIST = 15,      /* json [RoomInfo, ...] */
    WZ_EVENT_QUEUED = 16,         /* json QueueStatus */
    WZ_EVENT_MATCH_FOUND = 17,    /* json {"match_id","queue","room","host","teams"} — joined automatically */
    WZ_EVENT_MATCH_FINISHED = 18, /* json {"match_id","state"}: rated | disputed | abandoned */
    WZ_EVENT_MATCH_REPORTED = 19, /* json MatchInfo */
    WZ_EVENT_SCORE = 20,          /* json {"board", "result": {"best","rank","improved"}} */
    WZ_EVENT_STANDINGS = 21,      /* json StandingsResponse */
    WZ_EVENT_BOARD = 22,          /* json BoardResponse */
    WZ_EVENT_UPDATE = 23,         /* json UpdateInfo {"update_available","latest","changes":[...]} */
    WZ_EVENT_MODS_PROGRESS = 24,  /* json {"done","total"} bytes */
    WZ_EVENT_MODS_SYNCED = 25,    /* json {"lockfile": Lockfile, "paths": [str, ...]} in load order */
    WZ_EVENT_ERROR = 26           /* json {"op","code","message"} */
} wz_event_type;

typedef struct wz_event {
    wz_event_type type;
    wz_peer peer;          /* PEER_* events */
    uint8_t channel;       /* PEER_DATA */
    uint8_t reliable;      /* PEER_DATA: 1 if sent reliably */
    const uint8_t *data;   /* PEER_DATA payload */
    size_t len;
    /* NUL-terminated JSON (or "{}"). data and json stay valid until the next
     * wz_next_event(), wz_poll() or wz_destroy() on this client. */
    const char *json;
} wz_event;

typedef struct wz_config {
    uint32_t abi;               /* WZ_ABI_VERSION */
    const char *server;         /* NULL: WARPZONE_SERVER or the public instance */
    const char *game;           /* your descriptor id, e.g. "funny-furries" */
    const char *game_version;   /* your version, e.g. "0.3.0" */
    const char *home;           /* NULL: the shared data directory (identity, mods) */
    int auto_join_matches;      /* 1: join match rooms automatically */
} wz_config;

/* Library version string ("0.1.0"). */
const char *wz_version(void);

/* Last error of a call on this thread that returned NULL / -1. */
const char *wz_last_error(void);

/* Create a client and log in with the shared identity. NULL on failure. */
wz_client *wz_create(const wz_config *config);

/* Leave/close the current room and free everything. */
void wz_destroy(wz_client *c);

/* Process network traffic and results. Call every frame. */
void wz_poll(wz_client *c);

/* 1 and fills *out if an event was pending, else 0. */
int wz_next_event(wz_client *c, wz_event *out);

/* ---- identity ---------------------------------------------------------- */

/* Copies the username into buf (NUL-terminated) and returns its length,
 * or 0 when this device has no username yet (WZ_EVENT_LOGGED_IN tells). */
size_t wz_username(wz_client *c, char *buf, size_t cap);

/* Claim a username: WZ_EVENT_REGISTERED or WZ_EVENT_ERROR. */
void wz_register(wz_client *c, const char *username);

/* ---- rooms ------------------------------------------------------------- */

/* Host. options_json (all optional):
 * {"max_players": 4, "visibility": "public"|"unlisted",
 *  "entry": "open"|"code"|"approve",
 *  "audience": {"languages": ["en"], "skill": "beginners"|"mixed"|"experienced",
 *               "style": "casual"|"serious"|"learning"},
 *  "preset": "ffa", "metadata": {"map": "Arena 3"}, "lockfile": "<hash>"} */
void wz_host(wz_client *c, const char *options_json);

/* Join a room by id; code and/or invite secret may be NULL. */
void wz_join(wz_client *c, const char *room, const char *code, const char *secret);

/* Join by the 6-character code alone. */
void wz_join_code(wz_client *c, const char *code);

/* Read --warpzone-join/--warpzone-host from argv (and the environment).
 * Returns 1 if it started joining a room, 2 if the launcher asked for the
 * host screen (open it and call wz_host), 0 otherwise. */
int wz_handle_launch(wz_client *c, int argc, const char *const *argv);

void wz_answer_knock(wz_client *c, const char *knock, int accept);

/* Host: replace the metadata shown in browsers (JSON object of strings). */
void wz_set_metadata(wz_client *c, const char *json_object);

/* Host: 1 = no new joiners (game started), 0 = open again. */
void wz_set_closed(wz_client *c, int closed);

void wz_kick(wz_client *c, int64_t account, int ban);

/* Leave (guest) or close (host) the current room. */
void wz_leave(wz_client *c);

/* Public rooms of this game: WZ_EVENT_ROOM_LIST. */
void wz_list_rooms(wz_client *c, int only_with_space);

int wz_is_host(wz_client *c);

/* ---- peers ------------------------------------------------------------- */

/* Send up to WZ_MAX_DATAGRAM bytes. reliable=0: unreliable datagram (dropped
 * if not connected); 1: reliable, ordered per peer. Returns 1 if queued. */
int wz_send(wz_client *c, wz_peer peer, uint8_t channel, const void *data, size_t len, int reliable);

/* Send to every peer; returns how many accepted it. */
int wz_broadcast(wz_client *c, uint8_t channel, const void *data, size_t len, int reliable);

size_t wz_peer_name(wz_client *c, wz_peer peer, char *buf, size_t cap);
int64_t wz_peer_account(wz_client *c, wz_peer peer);   /* -1 if unknown */
int wz_peer_rtt_ms(wz_client *c, wz_peer peer);        /* -1 if unknown */
int wz_peer_relayed(wz_client *c, wz_peer peer);       /* 1 relay, 0 direct, -1 unknown */

/* ---- ladder & leaderboards --------------------------------------------- */

/* Enter a ranked queue (ruleset NULL = "stock", else a lockfile hash). */
void wz_queue(wz_client *c, const char *queue, const char *ruleset);
void wz_leave_queue(wz_client *c);

/* report_json: {"placements": [[winner accounts], [second], ...],
 *               "state_hash": "optional final-state hash"} */
void wz_report_match(wz_client *c, const char *match_id, const char *report_json);

/* value: milliseconds for "time" boards, points for "score" boards. */
void wz_submit_score(wz_client *c, const char *board, int64_t value, const char *ruleset, const char *replay_hash);
void wz_request_standings(wz_client *c, const char *queue, const char *season);
void wz_request_board(wz_client *c, const char *board, const char *ruleset, const char *season);

/* ---- updates & mods ---------------------------------------------------- */

/* Is there a newer release, and what changed since game_version:
 * WZ_EVENT_UPDATE. Show "changes" on the first start of a new version. */
void wz_check_update(wz_client *c);

/* Install (if needed) and activate the mod set of a room's lockfile:
 * WZ_EVENT_MODS_PROGRESS..., then WZ_EVENT_MODS_SYNCED with the paths. */
void wz_sync_mods(wz_client *c, const char *lockfile_hash);

/* ---- crash, problem and idea reports ----------------------------------- */

/* kind: "crash" (the game died), "bug" (something failed that should not:
 * a corrupt save, a failed load, an assertion) or "idea". title is one
 * line and stays the same for the same fault (put varying parts such as
 * addresses into detail): identical titles are counted, not repeated.
 * detail: stack, log lines, steps; at most 16 KiB. The home directory and
 * login name are removed before anything is sent. Report real faults only,
 * never ordinary player mistakes, and never passwords or tokens.
 *
 * wz_report sends in the background for this client's game and version and
 * never blocks; nothing comes back. */
void wz_report(wz_client *c, const char *kind, const char *title, const char *detail);

/* The same without a client, for crash handlers and for reporting a crash on
 * the next start (write it to a file when the game dies, send it then).
 * Blocks until the server answered (connect timeout 10 s). server NULL:
 * $WARPZONE_SERVER, else the default server. Returns 0 when the server took
 * it, -1 on failure (wz_last_error). Not async-signal-safe: do not call it
 * from a signal handler. */
int wz_report_problem(const char *server, const char *game, const char *game_version, const char *kind, const char *title, const char *detail);

#ifdef __cplusplus
}
#endif

#endif /* WARPZONE_H */
