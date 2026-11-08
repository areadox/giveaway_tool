//! Konfiguration (`config.json` neben der .exe) – kompatibel zur Python-Version.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const PLACEHOLDER_CHANNEL: &str = "dein_kanalname";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub channel: String,
    pub bot_username: String,
    pub oauth_token: String,
    pub keyword: String,
    pub web_port: u16,
    pub timer_duration: String,
    pub win_message: String,
    pub open_message: String,
    pub close_message: String,
    pub sound: bool,
    pub eve_enabled: bool,
    pub eve_command: String,
    pub eve_api_url: String,
    pub eve_api_key: String,
    pub eve2twitch_bot_name: String,
    /// Dauer des simulierten Matrix-Runs in Sekunden (neu in der Rust-Version).
    pub hack_duration: f64,
    /// Name des Deckers im Overlay; leer = Twitch-Channel (neu in der Rust-Version).
    pub decker_name: String,
    /// EVE-Charakternamen im Overlay durch „[EVE-NAME]“ ersetzen und keine Porträts zeigen.
    pub hide_eve_names: bool,
    /// Auto-Giveaway: öffnet nach `auto_interval` selbst ein Giveaway und
    /// beginnt nach dem Abschluss von vorn.
    pub auto_enabled: bool,
    /// Pause bis zum nächsten Giveaway (mm:ss oder Minuten).
    pub auto_interval: String,
    /// Wie lange der Gewinner im Auto-Modus stehen bleibt (Sekunden).
    pub auto_winner_hold: f64,
    /// Anzahl der Gewinner pro Giveaway.
    pub winners: u32,
    /// Freitext, was es zu gewinnen gibt (überschreibt die Kurzfassung der Preisliste).
    pub prize_text: String,
    /// Preisliste: Gewinner 1 bekommt Preis 1 usw.
    pub prizes: Vec<crate::prize::Prize>,
    /// Client-ID der eigenen EVE-Developer-App (für den Asset-Abruf).
    pub eve_sso_client_id: String,
    /// Refresh-Token der EVE-Anmeldung (wie das OAuth-Token: nicht weitergeben).
    pub eve_sso_refresh_token: String,
    pub eve_sso_character_id: Option<i64>,
    pub eve_sso_character_name: String,
    /// Zusätzlich den Scope für Strukturnamen anfragen (muss in der EVE-App eingetragen sein).
    pub eve_sso_structures: bool,
    /// Unbekannte Schlüssel bleiben beim Speichern erhalten.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel: PLACEHOLDER_CHANNEL.into(),
            bot_username: String::new(),
            oauth_token: String::new(),
            keyword: "!join".into(),
            web_port: 8766,
            timer_duration: "5:00".into(),
            win_message: "🎉 BREACH ERFOLGREICH! Gewinner: @{winner} - herzlichen Glückwunsch!"
                .into(),
            open_message: "🔓 Giveaway geöffnet! Schreibe {keyword}, um teilzunehmen.".into(),
            close_message: "🔒 Giveaway geschlossen. Viel Glück!".into(),
            sound: true,
            eve_enabled: false,
            eve_command: "!eve".into(),
            eve_api_url: String::new(),
            eve_api_key: String::new(),
            eve2twitch_bot_name: String::new(),
            hack_duration: 30.0,
            hide_eve_names: false,
            auto_enabled: false,
            auto_interval: "30:00".into(),
            auto_winner_hold: 20.0,
            winners: 1,
            prize_text: String::new(),
            prizes: Vec::new(),
            eve_sso_client_id: String::new(),
            eve_sso_refresh_token: String::new(),
            eve_sso_character_id: None,
            eve_sso_character_name: String::new(),
            eve_sso_structures: false,
            decker_name: String::new(),
            extra: Map::new(),
        }
    }
}

impl Config {
    pub fn channel_clean(&self) -> String {
        self.channel.trim().trim_start_matches('#').to_lowercase()
    }

    pub fn channel_configured(&self) -> bool {
        let c = self.channel_clean();
        !c.is_empty() && c != PLACEHOLDER_CHANNEL
    }

    pub fn keyword_or_default(&self) -> String {
        let k = self.keyword.trim();
        if k.is_empty() {
            "!join".into()
        } else {
            k.to_string()
        }
    }

    pub fn eve_command_or_default(&self) -> String {
        let k = self.eve_command.trim();
        if k.is_empty() {
            "!eve".into()
        } else {
            k.to_string()
        }
    }

    pub fn needs_setup(&self) -> bool {
        !self.channel_configured() || self.oauth_token.trim().is_empty()
    }

    pub fn decker(&self) -> String {
        let d = self.decker_name.trim();
        if !d.is_empty() {
            return d.to_string();
        }
        let c = self.channel_clean();
        if c.is_empty() || c == PLACEHOLDER_CHANNEL {
            "NULLSEKTOR".into()
        } else {
            c
        }
    }
}

pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

pub fn db_path() -> PathBuf {
    app_dir().join("giveaway.db")
}

/// Lädt die Konfiguration; fehlende oder kaputte Datei → Standardwerte.
pub fn load(path: &Path) -> Config {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    // Erst als freies JSON lesen, damit falsche Typen einzelner Felder
    // (z. B. web_port als String) nicht die ganze Datei verwerfen.
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(&text) else {
        return Config::default();
    };
    let defaults = serde_json::to_value(Config::default()).unwrap_or(Value::Null);
    if let Value::Object(def) = defaults {
        for (k, dv) in def {
            let ok = match (map.get(&k), &dv) {
                (None, _) => false,
                (Some(Value::String(_)), Value::String(_)) => true,
                (Some(Value::Bool(_)), Value::Bool(_)) => true,
                (Some(Value::Number(_)), Value::Number(_)) => true,
                (Some(Value::Array(_)), Value::Array(_)) => true,
                (Some(Value::Number(_) | Value::Null), Value::Null) => true,
                (Some(Value::String(s)), Value::Number(_)) => {
                    if let Ok(n) = s.trim().parse::<f64>() {
                        map.insert(k.clone(), serde_json::json!(n));
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if !ok {
                map.insert(k, dv);
            }
        }
    }
    // web_port als Ganzzahl erzwingen
    if let Some(p) = map.get("web_port").and_then(Value::as_f64) {
        map.insert(
            "web_port".into(),
            serde_json::json!(p.clamp(1.0, 65535.0) as u16),
        );
    }
    serde_json::from_value(Value::Object(map)).unwrap_or_default()
}

pub fn save(path: &Path, cfg: &Config) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Giveaway-Dauer: „mm:ss“ oder Minuten (auch „2,5“). 0 = manuelles Ende.
pub fn parse_duration(value: &str) -> Result<u64, String> {
    let text = value.trim();
    if text.is_empty() {
        return Err("Dauer fehlt".into());
    }
    if text.contains(':') {
        let parts: Vec<&str> = text.split(':').collect();
        if parts.len() != 2
            || !parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Err("Format muss mm:ss sein".into());
        }
        let m: u64 = parts[0]
            .parse()
            .map_err(|_| "Format muss mm:ss sein".to_string())?;
        let s: u64 = parts[1]
            .parse()
            .map_err(|_| "Format muss mm:ss sein".to_string())?;
        if s > 59 {
            return Err("Sekunden müssen zwischen 00 und 59 liegen".into());
        }
        return Ok(m * 60 + s);
    }
    let minutes: f64 = text
        .replace(',', ".")
        .parse()
        .map_err(|_| "Ungültige Zahl".to_string())?;
    if !minutes.is_finite() || minutes < 0.0 {
        return Err("Minuten müssen eine nicht-negative Zahl sein".into());
    }
    Ok((minutes * 60.0).round() as u64)
}

pub fn format_clock(seconds: f64) -> String {
    let s = seconds.max(0.0).ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("5:00").unwrap(), 300);
        assert_eq!(parse_duration("2,5").unwrap(), 150);
        assert_eq!(parse_duration("0").unwrap(), 0);
        assert!(parse_duration("1:75").is_err());
        assert!(parse_duration("abc").is_err());
        assert_eq!(format_clock(61.2), "01:02");
    }

    #[test]
    fn loads_python_config() {
        let dir = std::env::temp_dir().join(format!("cg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.json");
        std::fs::write(
            &p,
            r#"{"channel":"areadox","keyword":"!hack","web_port":"8766","sound":false,"foo":1}"#,
        )
        .unwrap();
        let c = load(&p);
        assert_eq!(c.channel, "areadox");
        assert_eq!(c.keyword, "!hack");
        assert_eq!(c.web_port, 8766);
        assert!(!c.sound);
        assert_eq!(c.hack_duration, 30.0);
        assert!(c.extra.contains_key("foo"));
        assert_eq!(c.winners, 1);
    }

    #[test]
    fn keeps_prizes_and_sso() {
        let dir = std::env::temp_dir().join(format!("cg-test-p-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.json");
        let mut c = Config::default();
        c.winners = 3;
        c.prizes = vec![crate::prize::Prize {
            name: "Gila".into(),
            quantity: 1,
            type_id: Some(17715),
            ..Default::default()
        }];
        c.eve_sso_character_id = Some(2112625428);
        save(&p, &c).unwrap();
        let back = load(&p);
        assert_eq!(back.winners, 3);
        assert_eq!(back.prizes, c.prizes);
        assert_eq!(back.eve_sso_character_id, Some(2112625428));
    }
}
