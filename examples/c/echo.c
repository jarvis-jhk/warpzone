/*
 * echo.c — the smallest complete Warpzone game client.
 *
 *   ./echo host            host an unlisted room and echo what peers send
 *   ./echo join K7QM3X     join by code and send a numbered ping every second
 *
 * Build (Linux): see examples/c/Makefile. Uses the game id "demo-echo"; any
 * id works for rooms on a server whose descriptors list it.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "warpzone.h"

static void sleep_ms(int ms) {
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&t, NULL);
}

int main(int argc, char **argv) {
    if (argc < 2 || (strcmp(argv[1], "host") && (strcmp(argv[1], "join") || argc < 3))) {
        fprintf(stderr, "usage: %s host | join <code>\n", argv[0]);
        return 2;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);
    wz_config cfg = { WZ_ABI_VERSION, getenv("WARPZONE_SERVER"), getenv("WZ_GAME") ? getenv("WZ_GAME") : "demo", "1.0.0", NULL, 1 };
    wz_client *c = wz_create(&cfg);
    if (!c) {
        fprintf(stderr, "wz_create: %s\n", wz_last_error());
        return 1;
    }
    printf("libwarpzone %s\n", wz_version());
    int hosting = !strcmp(argv[1], "host");
    int started = 0, pings = 0, echoes = 0;
    time_t last = 0;
    for (int frame = 0; frame < 60 * 60 * 10; frame++) {
        wz_poll(c);
        wz_event e;
        while (wz_next_event(c, &e)) {
            switch (e.type) {
            case WZ_EVENT_LOGGED_IN:
                if (!started) {
                    started = 1;
                    if (hosting)
                        wz_host(c, "{\"max_players\": 4, \"visibility\": \"unlisted\"}");
                    else
                        wz_join_code(c, argv[2]);
                }
                break;
            case WZ_EVENT_ROOM_HOSTED: printf("hosting: %s\n", e.json); break;
            case WZ_EVENT_PEER_CONNECTED: printf("peer %u connected %s\n", e.peer, e.json); break;
            case WZ_EVENT_PEER_DATA:
                if (hosting) {
                    wz_send(c, e.peer, e.channel, e.data, e.len, e.reliable);
                } else {
                    echoes++;
                    printf("echo %.*s (rtt %d ms, relayed %d)\n", (int)e.len, (const char *)e.data, wz_peer_rtt_ms(c, e.peer), wz_peer_relayed(c, e.peer));
                    if (getenv("WZ_ECHO_ONCE")) { wz_destroy(c); return 0; }
                }
                break;
            case WZ_EVENT_PEER_LEFT: printf("peer %u left\n", e.peer); break;
            case WZ_EVENT_ROOM_CLOSED: printf("room closed\n"); wz_destroy(c); return 0;
            case WZ_EVENT_ERROR: fprintf(stderr, "error: %s\n", e.json); break;
            default: break;
            }
        }
        if (!hosting && time(NULL) != last) {
            last = time(NULL);
            char msg[32];
            int n = snprintf(msg, sizeof msg, "ping %d", ++pings);
            wz_broadcast(c, 0, msg, (size_t)n, 0);
        }
        sleep_ms(16);
    }
    wz_destroy(c);
    return echoes > 0 ? 0 : 1;
}
