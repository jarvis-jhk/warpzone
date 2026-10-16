//! SQLite schema. Rooms, sessions and relay channels live in memory only (a
//! restart drops them; clients re-host); everything here survives restarts.

use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY,
  username TEXT NOT NULL,
  username_lc TEXT NOT NULL UNIQUE,
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS keys (
  key TEXT PRIMARY KEY,
  account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  added INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ratings (
  game TEXT NOT NULL, queue TEXT NOT NULL, season TEXT NOT NULL,
  account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  mu REAL NOT NULL, sigma REAL NOT NULL,
  games INTEGER NOT NULL DEFAULT 0, wins INTEGER NOT NULL DEFAULT 0,
  updated INTEGER NOT NULL,
  PRIMARY KEY (game, queue, season, account)
);
CREATE TABLE IF NOT EXISTS matches (
  id TEXT PRIMARY KEY, game TEXT NOT NULL, queue TEXT NOT NULL, season TEXT NOT NULL,
  room TEXT NOT NULL, ruleset TEXT NOT NULL,
  teams TEXT NOT NULL,          -- JSON [[account, ...], ...]
  state TEXT NOT NULL,          -- open | rated | disputed | abandoned
  created INTEGER NOT NULL, finished INTEGER
);
CREATE TABLE IF NOT EXISTS match_reports (
  match TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
  account INTEGER NOT NULL,
  report TEXT NOT NULL,         -- JSON MatchReport
  created INTEGER NOT NULL,
  PRIMARY KEY (match, account)
);
CREATE TABLE IF NOT EXISTS match_ratings (
  match TEXT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
  account INTEGER NOT NULL, before INTEGER NOT NULL, after INTEGER,
  PRIMARY KEY (match, account)
);
CREATE TABLE IF NOT EXISTS scores (
  game TEXT NOT NULL, board TEXT NOT NULL, ruleset TEXT NOT NULL,
  season TEXT NOT NULL,         -- 'all' or a season id
  account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  value INTEGER NOT NULL, replay_hash TEXT, submitted INTEGER NOT NULL,
  PRIMARY KEY (game, board, ruleset, season, account)
);
CREATE TABLE IF NOT EXISTS mods (
  game TEXT NOT NULL, name TEXT NOT NULL,
  owner INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
  title TEXT NOT NULL, category TEXT NOT NULL, description TEXT NOT NULL,
  tags TEXT NOT NULL, license TEXT NOT NULL, homepage TEXT,
  downloads INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL, updated INTEGER NOT NULL,
  PRIMARY KEY (game, name)
);
CREATE TABLE IF NOT EXISTS mod_versions (
  game TEXT NOT NULL, name TEXT NOT NULL, version TEXT NOT NULL,
  manifest TEXT NOT NULL, game_version TEXT NOT NULL, deps TEXT NOT NULL,
  sha256 TEXT NOT NULL, size INTEGER NOT NULL, filename TEXT NOT NULL,
  state TEXT NOT NULL,          -- pending | approved | rejected | hidden
  reason TEXT, yanked INTEGER NOT NULL DEFAULT 0,
  submitted INTEGER NOT NULL, decided INTEGER,
  PRIMARY KEY (game, name, version)
);
CREATE TABLE IF NOT EXISTS notes (
  game TEXT NOT NULL, name TEXT NOT NULL, version TEXT NOT NULL,
  role TEXT NOT NULL, text TEXT NOT NULL, created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reports (
  id INTEGER PRIMARY KEY, target TEXT NOT NULL, game TEXT, name TEXT NOT NULL, version TEXT,
  reason TEXT NOT NULL, detail TEXT, reporter INTEGER, created INTEGER NOT NULL, closed INTEGER
);
CREATE TABLE IF NOT EXISTS lockfiles (
  hash TEXT PRIMARY KEY, game TEXT NOT NULL, body TEXT NOT NULL, created INTEGER NOT NULL
);
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch(SCHEMA)?;
    Ok(c)
}
