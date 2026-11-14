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

/// Spielname eines Zuschauers für ein anderes Spiel als EVE.
#[derive(Debug, Clone, PartialEq)]
pub struct GameName {
    pub twitch_login: String,
    pub game: String,
    pub name: String,
    /// Zusatzdaten (bei WoW: Realm, Klasse, Stufe, Gilde, Bild)
    pub info: Option<serde_json::Value>,
    pub updated_at: String,
}

impl GameName {
    fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        let info: Option<String> = r.get(3)?;
        Ok(Self {
            twitch_login: r.get(0)?,
            game: r.get(1)?,
            name: r.get(2)?,
            info: info.and_then(|s| serde_json::from_str(&s).ok()),
            updated_at: r.get(4)?,
        })
    }
}

/// Im Chat-Dock gesendete Nachricht (zum erneuten Senden)
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SavedMsg {
    pub id: i64,
    pub text: String,
    pub uses: i64,
    pub last_used: String,
    pub pinned: bool,
    /// automatisch wiederholen alle n Minuten (0 = aus)
    pub repeat_min: u32,
}

/// So viele nicht angeheftete Nachrichten ohne Wiederholung bleiben gespeichert.
pub const SAVED_KEEP: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub winner: String,
    pub played_at: String,
    pub participants: i64,
    pub prize: Option<String>,
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
        // Ab 0.0.5: Preis je Gewinner (ältere Datenbanken werden ergänzt)
        let has_prize = c
            .prepare("SELECT prize FROM giveaway_history LIMIT 0")
            .is_ok();
        if !has_prize {
            c.execute_batch("ALTER TABLE giveaway_history ADD COLUMN prize TEXT;")?;
        }
        // Ab 0.0.13: Spielnamen für andere Spiele (WoW, Palia, …); EVE bleibt in `users`
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS game_names (
                twitch_login TEXT NOT NULL COLLATE NOCASE, game TEXT NOT NULL,
                name TEXT NOT NULL, info TEXT, updated_at TEXT NOT NULL,
                PRIMARY KEY (twitch_login, game));",
        )?;
        // Ab 0.0.15: gesendete Chatnachrichten des Chat-Docks
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS chat_saved (
                id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL UNIQUE,
                uses INTEGER NOT NULL DEFAULT 1, last_used TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0, repeat_min INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(db)
    }

    /// Spielname eines Zuschauers für ein Spiel setzen (ersetzt den alten).
    pub fn set_game_name(
        &self,
        login: &str,
        game: &str,
        name: &str,
        info: Option<&serde_json::Value>,
    ) -> Result<(), DbError> {
        let login = clean_login(login);
        let name = name.trim();
        if login.is_empty() || name.is_empty() || game.trim().is_empty() {
            return Err(DbError::Invalid(
                "Twitch-Login, Spiel und Name dürfen nicht leer sein".into(),
            ));
        }
        self.conn()?.execute(
            "INSERT INTO game_names(twitch_login,game,name,info,updated_at) VALUES(?,?,?,?,?)
             ON CONFLICT(twitch_login,game) DO UPDATE SET name=excluded.name, info=excluded.info, updated_at=excluded.updated_at",
            params![login, game.trim().to_lowercase(), name, info.map(|v| v.to_string()), now_iso()],
        )?;
        Ok(())
    }

    pub fn get_game_name(&self, login: &str, game: &str) -> Option<GameName> {
        let c = self.conn().ok()?;
        c.query_row(
            "SELECT twitch_login, game, name, info, updated_at FROM game_names WHERE twitch_login=? COLLATE NOCASE AND game=?",
            params![clean_login(login), game.to_lowercase()],
            GameName::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_game_names(&self) -> Vec<GameName> {
        let Ok(c) = self.conn() else { return vec![] };
        let Ok(mut st) = c.prepare(
            "SELECT twitch_login, game, name, info, updated_at FROM game_names ORDER BY game, twitch_login COLLATE NOCASE",
        ) else {
            return vec![];
        };
        st.query_map([], GameName::from_row)
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn delete_game_name(&self, login: &str, game: &str) -> Result<(), DbError> {
        self.conn()?.execute(
            "DELETE FROM game_names WHERE twitch_login=? COLLATE NOCASE AND game=?",
            params![clean_login(login), game.to_lowercase()],
        )?;
        Ok(())
    }

    /// Gesendete Nachricht merken (gleicher Text zählt nur hoch).
    pub fn chat_remember(&self, text: &str) -> Result<(), DbError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let c = self.conn()?;
        c.execute(
            "INSERT INTO chat_saved(text,uses,last_used) VALUES(?,1,?)
             ON CONFLICT(text) DO UPDATE SET uses=uses+1, last_used=excluded.last_used",
            params![text, now_iso()],
        )?;
        // Alte, unwichtige Einträge aufräumen
        c.execute(
            "DELETE FROM chat_saved WHERE pinned=0 AND repeat_min=0 AND id NOT IN (
                SELECT id FROM chat_saved WHERE pinned=0 AND repeat_min=0
                ORDER BY last_used DESC LIMIT ?)",
            params![SAVED_KEEP as i64],
        )?;
        Ok(())
    }

    /// Angeheftete zuerst, dann die zuletzt gesendeten.
    pub fn chat_saved(&self) -> Vec<SavedMsg> {
        let Ok(c) = self.conn() else { return vec![] };
        let Ok(mut st) = c.prepare(
            "SELECT id, text, uses, last_used, pinned, repeat_min FROM chat_saved
             ORDER BY pinned DESC, last_used DESC",
        ) else {
            return vec![];
        };
        st.query_map([], |r| {
            Ok(SavedMsg {
                id: r.get(0)?,
                text: r.get(1)?,
                uses: r.get(2)?,
                last_used: r.get(3)?,
                pinned: r.get::<_, i64>(4)? != 0,
                repeat_min: r.get::<_, i64>(5)?.clamp(0, 1440) as u32,
            })
        })
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn chat_get(&self, id: i64) -> Option<SavedMsg> {
        self.chat_saved().into_iter().find(|m| m.id == id)
    }

    pub fn chat_update(
        &self,
        id: i64,
        pinned: Option<bool>,
        repeat_min: Option<u32>,
    ) -> Result<(), DbError> {
        let c = self.conn()?;
        if let Some(p) = pinned {
            c.execute("UPDATE chat_saved SET pinned=? WHERE id=?", params![p as i64, id])?;
        }
        if let Some(r) = repeat_min {
            c.execute(
                "UPDATE chat_saved SET repeat_min=? WHERE id=?",
                params![r.min(1440) as i64, id],
            )?;
        }
        Ok(())
    }

    pub fn chat_delete(&self, id: i64) -> Result<(), DbError> {
        self.conn()?
            .execute("DELETE FROM chat_saved WHERE id=?", params![id])?;
        Ok(())
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

    pub fn add_history(
        &self,
        winner: &str,
        count: usize,
        prize: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn()?.execute(
            "INSERT INTO giveaway_history(winner,played_at,participant_count,prize) VALUES(?,?,?,?)",
            params![winner, now_iso(), count as i64, prize],
        )?;
        Ok(())
    }

    pub fn history(&self, limit: usize) -> Vec<HistoryRow> {
        let Ok(c) = self.conn() else { return vec![] };
        let Ok(mut st) = c.prepare("SELECT winner, played_at, participant_count, prize FROM giveaway_history ORDER BY id DESC LIMIT ?") else {
            return vec![];
        };
        st.query_map([limit as i64], |r| {
            Ok(HistoryRow {
                winner: r.get(0)?,
                played_at: r.get(1)?,
                participants: r.get(2)?,
                prize: r.get(3)?,
            })
        })
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
    fn game_names() {
        let p = std::env::temp_dir().join(format!("cg-gn-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = Db::open(&p).unwrap();
        db.set_game_name(
            "@Alice",
            "WoW",
            "Thrall",
            Some(&serde_json::json!({"realm":"Blackrock"})),
        )
        .unwrap();
        db.set_game_name("alice", "palia", "Ali", None).unwrap();
        db.set_game_name("ALICE", "wow", "Jaina", None).unwrap();
        let g = db.get_game_name("alice", "wow").unwrap();
        assert_eq!(g.name, "Jaina");
        assert!(g.info.is_none());
        assert_eq!(db.list_game_names().len(), 2);
        assert!(db.set_game_name("bob", "wow", "  ", None).is_err());
        db.delete_game_name("alice", "palia").unwrap();
        assert!(db.get_game_name("alice", "palia").is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn chat_saved() {
        let p = std::env::temp_dir().join(format!("cg-chat-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = Db::open(&p).unwrap();
        db.chat_remember("Hallo Chat").unwrap();
        db.chat_remember("Hallo Chat").unwrap();
        db.chat_remember("Discord: x").unwrap();
        let l = db.chat_saved();
        assert_eq!(l.len(), 2);
        let hallo = l.iter().find(|m| m.text == "Hallo Chat").unwrap().clone();
        assert_eq!(hallo.uses, 2);
        db.chat_update(hallo.id, Some(true), Some(15)).unwrap();
        let first = &db.chat_saved()[0];
        assert!(first.pinned);
        assert_eq!(first.repeat_min, 15);
        for i in 0..SAVED_KEEP + 5 {
            db.chat_remember(&format!("m{i}")).unwrap();
        }
        // Angeheftete bleiben, der Rest wird begrenzt
        assert_eq!(db.chat_saved().len(), SAVED_KEEP + 1);
        db.chat_delete(hallo.id).unwrap();
        assert!(db.chat_get(hallo.id).is_none());
        let _ = std::fs::remove_file(&p);
    }

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
        db.add_history("areadox // daulin", 3, Some("1× Gila"))
            .unwrap();
        let h = db.history(5);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].prize.as_deref(), Some("1× Gila"));
        // Öffnen einer bereits migrierten Datenbank klappt erneut
        Db::open(&p).unwrap();
        db.delete_user("areadox").unwrap();
        assert!(db.get_user("areadox").is_none());
        let _ = std::fs::remove_file(&p);
    }
}
