# Moderation

Warpzone has deliberately little to moderate: no chat, no free-text room
names, structured room audiences. What remains is **mods** (every version is
reviewed before it is public), **usernames** and **reports**.

## Roles

Staff are device keys in the server's `staff.toml` (re-read within a minute):

```toml
moderators = ["<hex public key>"]   # decide: approve, reject, hide, rename, close reports
reviewers  = ["<hex public key>"]   # read the queue and add notes (e.g. an automated pre-review)
blocked_words = ["…"]               # substrings no username may contain
```

`wz whoami` prints a device's key. Staff do not need a username. Keep the
staff file outside anything an automated agent can write: a moderator key
is the only thing that makes a mod public.

## Getting notified

* `notify_url` in `server.toml` receives a plain-text POST for every
  submission and report (an ntfy topic works well).
* Staff receive `mod_submitted` events; the launcher's **Moderation** tab and
  `wz queue` list everything waiting.

## Reviewing a mod version

The queue shows the manifest, size, SHA-256, whether the game's mods may
contain code, and notes from reviewers. Download the archive to inspect it
(the launcher saves it to Downloads; `wz queue` prints the URL — pending
versions are only downloadable by the author and staff).

Check:

1. **Rights** — original or licensed content; nothing extracted from the
   original games (ROM bytes, sprites, music, maps); a sensible `license`.
2. **Safety** — no executables or scripts for games whose mods are data
   only; for games with `contains_code`, nothing obviously malicious.
3. **Fit** — the category is right, the title and description are honest,
   dependencies make sense.

Then decide:

```sh
wz approve <game> <name> <version>
wz reject  <game> <name> <version> copyright      # reason is shown to the author
wz hide    <game> <name> <version>                # take an approved version down
wz unhide  <game> <name> <version>
```

Rejection reasons: `copyright`, `malware`, `broken`, `inappropriate`, `spam`,
`wrong-category`, `missing-license`, `other`. A rejected archive is deleted
from storage unless another version uses the same bytes.

## Reports and usernames

Players report mods or usernames (`POST /api/v1/reports`, reasons:
`copyright`, `malware`, `broken`, `inappropriate`, `spam`, `impersonation`,
`other`). They appear in the queue; act, then `wz close-report <id>`.
Offensive or impersonating usernames: `wz rename-user <old> <new>`.

## Takedown requests

Copyright holders can ask for removal through the instance's contact
(`legal_url`) or an issue on the project. Hide the version at once while you
look into it.
