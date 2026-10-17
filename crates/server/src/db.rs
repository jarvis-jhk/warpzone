//! SQLite schema. Rooms, sessions and relay channels live in memory only (a
//! restart drops them; clients re-host); everything here survives restarts.

use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT, -- never reused: old match and report rows keep pointing at the deleted account
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
CREATE TABLE IF NOT EXISTS feedback (
  id INTEGER PRIMARY KEY, game TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL,
  version TEXT, client TEXT, account INTEGER, count INTEGER NOT NULL DEFAULT 1,
  created INTEGER NOT NULL, last INTEGER NOT NULL, forwarded INTEGER
);
CREATE INDEX IF NOT EXISTS feedback_title ON feedback (game, kind, title);
CREATE TABLE IF NOT EXISTS lockfiles (
  hash TEXT PRIMARY KEY, game TEXT NOT NULL, body TEXT NOT NULL, created INTEGER NOT NULL
);
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch(SCHEMA)?;
    migrate_account_ids(&c)?;
    Ok(c)
}

/// Databases from 0.1.1 and older created `accounts` without AUTOINCREMENT,
/// so SQLite handed a deleted account's id to the next registration. Rebuild
/// the table with it, and start the sequence above every id still mentioned
/// anywhere (match reports and ratings have no foreign key and outlive the
/// account).
fn migrate_account_ids(c: &Connection) -> rusqlite::Result<()> {
    let sql: String = c.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'accounts'", [], |r| r.get(0))?;
    if sql.contains("AUTOINCREMENT") {
        return Ok(());
    }
    c.execute_batch(
        r#"
PRAGMA foreign_keys = OFF;
BEGIN;
CREATE TABLE accounts_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL,
  username_lc TEXT NOT NULL UNIQUE,
  created INTEGER NOT NULL
);
INSERT INTO accounts_new (id, username, username_lc, created) SELECT id, username, username_lc, created FROM accounts;
DROP TABLE accounts;
ALTER TABLE accounts_new RENAME TO accounts;
DELETE FROM sqlite_sequence WHERE name = 'accounts';
INSERT INTO sqlite_sequence (name, seq) SELECT 'accounts', MAX(
  (SELECT IFNULL(MAX(id), 0) FROM accounts),
  (SELECT IFNULL(MAX(account), 0) FROM match_reports),
  (SELECT IFNULL(MAX(account), 0) FROM match_ratings),
  (SELECT IFNULL(MAX(reporter), 0) FROM reports));
COMMIT;
PRAGMA foreign_keys = ON;
"#,
    )?;
    log::info!("accounts: ids are no longer reused (AUTOINCREMENT)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids_are_never_reused() {
        let dir = std::env::temp_dir().join(format!("wz-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let _ = std::fs::remove_file(&path);
        // A 0.1.1 database: plain INTEGER PRIMARY KEY, account 3 deleted
        // after a match it played.
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(&SCHEMA.replace("PRIMARY KEY AUTOINCREMENT", "PRIMARY KEY")).unwrap();
            c.execute_batch(
                "INSERT INTO accounts (id, username, username_lc, created) VALUES (1, 'A', 'a', 0), (2, 'B', 'b', 0), (3, 'C', 'c', 0);
                 INSERT INTO keys VALUES ('k1', 1, 0);
                 INSERT INTO matches VALUES ('m', 'g', 'q', 's', 'r', 'x', '[]', 'rated', 0, 0);
                 INSERT INTO match_ratings VALUES ('m', 4, 1000, 1010);
                 DELETE FROM accounts WHERE id = 3;",
            )
            .unwrap();
        }
        let c = open(&path).unwrap();
        let c2 = open(&path).unwrap(); // a second open leaves it alone
        drop(c2);
        c.execute("INSERT INTO accounts (username, username_lc, created) VALUES ('D', 'd', 0)", []).unwrap();
        assert_eq!(c.last_insert_rowid(), 5);
        c.execute("DELETE FROM accounts WHERE id = 5", []).unwrap();
        c.execute("INSERT INTO accounts (username, username_lc, created) VALUES ('E', 'e', 0)", []).unwrap();
        assert_eq!(c.last_insert_rowid(), 6);
        // Foreign keys still cascade to the renamed table.
        c.execute("DELETE FROM accounts WHERE id = 1", []).unwrap();
        let keys: i64 = c.query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0)).unwrap();
        assert_eq!(keys, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
