# Ladder & leaderboards

Ranked play is always optional and declared per game (`[[ladder]]` in the
[descriptor](descriptors.md)).

## Ratings

Warpzone uses **OpenSkill** (Weng-Lin, Plackett-Luce model) — an open
alternative to TrueSkill that handles 1v1, teams and free-for-all with one
formula. Every player has μ (skill estimate, starts at 25) and σ
(uncertainty, starts at 25/3). After a match each team's μ moves according to
how surprising its placement was; σ shrinks as the system learns.

Displayed rating:

```text
rating = 1500 + 20 × (μ − 3σ)
```

It starts at 1500 and rises for most players while σ shrinks (the system
becomes sure about you); standings sort by the same conservative value, so a
lucky streak from few games does not top the table.

## Seasons

Seasons follow the calendar in UTC: **quarterly** by default (`2026-Q4`
starts 1 October), or `monthly` / `yearly` per game. A new season carries
each player's last rating over with a soft reset — μ halfway back to 25, σ
halfway back to its start value — so good players start ahead but everyone
moves quickly again. You appear in the standings after **5** matches in a
season ("placing" until then). Past seasons' standings stay available
(`?season=2026-Q3`).

## Matchmaking

* A queue is keyed by game, queue id, **game version** and **ruleset**
  (`stock`, or the lockfile hash of a mod set): different versions and
  modded rules never meet.
* Every second the matchmaker sorts each queue by rating and forms groups of
  `teams × team_size` whose rating spread fits a window that starts at 150
  points and widens by 15 per second the oldest member waited (up to 1500).
* Teams are drafted snake-style (best to team 1, second to team 2, …,
  then backwards) for balance.
* A match is a private room the server creates; every player is pre-approved,
  the strongest player hosts, and the library joins automatically.

## Results

Every player reports the placements (`placements[0]` = winners); games with
deterministic simulation also send a final-state hash.

| Situation | Outcome |
|---|---|
| all reports identical (and hashes, where sent) | **rated** |
| reports disagree | **disputed** — unrated |
| some players never report | after 10 minutes the reports present are used if they agree |
| nobody reports within 3 hours | **abandoned** — unrated |

Leaving a match early is reported by the others as a loss.

## Leaderboards

`[[leaderboards]]` boards keep each player's best value per ruleset:
`time` (milliseconds, lower wins) or `score` (higher wins), all-time and —
with `seasonal = true` — per season. A submission may include a replay
(base64, its SHA-256 as `replay_hash`); deterministic games can verify a
record by replaying it (`GET /api/v1/replays/{hash}`).
