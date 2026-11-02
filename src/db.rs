//! SQLite-Datenbank (`giveaway.db`) – gleiches Schema wie die Python-Version.

use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct User {
    pub twitch_login: String,
    pub twitch_id: Option<String>,
    pub eve_character_name: Option<String>,
    pub eve_character_id: Option<i64>,
    pub corp_ticker: Option<String>,
    pub alliance_ticker: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl User {
    fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            twitch_login: r.get(0)?,
            twitch_id: r.get(1)?,
            eve_character_name: r.get(2)?,
            eve_character_id: r.get(3)?,
            corp_ticker: r.get(4)?,
            alliance_ticker: r.get(5)?,
            source: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NewUser {
    pub twitch_login: String,
    pub eve_character_name: Option<String>,
    pub eve_character_id: Option<i64>,
    pub twitch_id: Option<String>,
    pub corp_ticker: Option<String>,
    pub alliance_ticker: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug)]
pub enum DbError {
    /// EVE-Charakter-ID ist schon einem anderen Twitch-Konto zugeordnet.
    Duplicate,
    Invalid(String),
    Sql(rusqlite::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Duplicate => write!(
                f,
                "Dieser EVE-Charakter ist bereits einem anderen Twitch-Konto zugeordnet."
            ),
            DbError::Invalid(s) => write!(f, "{s}"),
            DbError::Sql(e) => write!(f, "Datenbankfehler: {e}"),
        }
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(err, _) = &e {
            if err.code == rusqlite::ErrorCode::ConstraintViolation {
                return DbError::Duplicate;
            }
        }
        DbError::Sql(e)
    }
}

const COLS: &str = "twitch_login, twitch_id, eve_character_name, eve_character_id, corp_ticker, alliance_ticker, source, created_at, updated_at";

#[derive(Clone)]
pub struct Db {
    path: PathBuf,
}

pub fn now_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

pub fn clean_login(s: &str) -> String {
    s.trim().trim_start_matches('@').to_lowercase()
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let db = Self {
            path: path.to_path_buf(),
        };
        let c = db.conn()?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                twitch_login TEXT NOT NULL UNIQUE COLLATE NOCASE, twitch_id TEXT,
                eve_character_name TEXT, eve_character_id INTEGER UNIQUE,
                corp_ticker TEXT, alliance_ticker TEXT, source TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS giveaway_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT, winner TEXT NOT NULL,
                played_at TEXT NOT NULL, participant_count INTEGER NOT NULL);",
        )?;
        Ok(db)
    }

    fn conn(&self) -> Result<Connection, DbError> {
        let c = Connection::open(&self.path)?;
        c.busy_timeout(std::time::Duration::from_secs(8))?;
        Ok(c)
    }

    pub fn upsert_user(&self, u: &NewUser) -> Result<(), DbError> {
        let login = clean_login(&u.twitch_login);
        if login.is_empty() {
            return Err(DbError::Invalid("Twitch-Login darf nicht leer sein".into()));
        }
        let now = now_iso();
        let c = self.conn()?;
        let created: String = c
            .query_row(
                "SELECT created_at FROM users WHERE twitch_login=?",
                [&login],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| now.clone());
        let blank = |o: &Option<String>| {
            o.as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        c.execute(
            "INSERT INTO users VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(twitch_login) DO UPDATE SET
                twitch_id=excluded.twitch_id,eve_character_name=excluded.eve_character_name,
                eve_character_id=excluded.eve_character_id,corp_ticker=excluded.corp_ticker,
                alliance_ticker=excluded.alliance_ticker,source=excluded.source,updated_at=excluded.updated_at",
            params![
                login,
                blank(&u.twitch_id),
                blank(&u.eve_character_name),
                u.eve_character_id,
                blank(&u.corp_ticker),
                blank(&u.alliance_ticker),
                blank(&u.source),
                created,
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_user(&self, login: &str) -> Option<User> {
        let c = self.conn().ok()?;
        c.query_row(
            &format!("SELECT {COLS} FROM users WHERE twitch_login=? COLLATE NOCASE"),
            [clean_login(login)],
            User::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_users(&self) -> Vec<User> {
        let Ok(c) = self.conn() else { return vec![] };
        let Ok(mut st) = c.prepare(&format!(
            "SELECT {COLS} FROM users ORDER BY twitch_login COLLATE NOCASE"
        )) else {
            return vec![];
        };
        st.query_map([], User::from_row)
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn delete_user(&self, login: &str) -> Result<(), DbError> {
        self.conn()?.execute(
            "DELETE FROM users WHERE twitch_login=? COLLATE NOCASE",
            [clean_login(login)],
        )?;
        Ok(())
    }

    pub fn add_history(&self, winner: &str, count: usize) -> Result<(), DbError> {
        self.conn()?.execute(
            "INSERT INTO giveaway_history(winner,played_at,participant_count) VALUES(?,?,?)",
            params![winner, now_iso(), count as i64],
        )?;
        Ok(())
    }

    pub fn history(&self, limit: usize) -> Vec<(String, String, i64)> {
        let Ok(c) = self.conn() else { return vec![] };
        let Ok(mut st) = c.prepare("SELECT winner, played_at, participant_count FROM giveaway_history ORDER BY id DESC LIMIT ?") else {
            return vec![];
        };
        st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// CSV-Export mit BOM (öffnet sauber in Excel), Komma-getrennt wie das Original.
    pub fn export_csv(&self, path: &Path) -> std::io::Result<()> {
        fn esc(s: &str) -> String {
            if s.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        }
        let mut out = String::from("\u{feff}");
        out.push_str("twitch_login,twitch_id,eve_character_name,eve_character_id,corp_ticker,alliance_ticker,source,created_at,updated_at\r\n");
        for u in self.list_users() {
            let o = |v: &Option<String>| esc(v.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\r\n",
                esc(&u.twitch_login),
                o(&u.twitch_id),
                o(&u.eve_character_name),
                u.eve_character_id
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                o(&u.corp_ticker),
                o(&u.alliance_ticker),
                o(&u.source),
                esc(&u.created_at),
                esc(&u.updated_at)
            ));
        }
        std::fs::write(path, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_duplicate() {
        let p = std::env::temp_dir().join(format!("cg-db-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = Db::open(&p).unwrap();
        db.upsert_user(&NewUser {
            twitch_login: "@Areadox".into(),
            eve_character_name: Some("daulin".into()),
            eve_character_id: Some(1),
            source: Some("test".into()),
            ..Default::default()
        })
        .unwrap();
        let u = db.get_user("AREADOX").unwrap();
        assert_eq!(u.eve_character_name.as_deref(), Some("daulin"));
        let e = db.upsert_user(&NewUser {
            twitch_login: "other".into(),
            eve_character_id: Some(1),
            ..Default::default()
        });
        assert!(matches!(e, Err(DbError::Duplicate)));
        db.add_history("areadox // daulin", 3).unwrap();
        assert_eq!(db.history(5).len(), 1);
        db.delete_user("areadox").unwrap();
        assert!(db.get_user("areadox").is_none());
        let _ = std::fs::remove_file(&p);
    }
}
