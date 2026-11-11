//! World of Warcraft: Charaktersuche über die offizielle Blizzard-API
//! (Client-Credentials, kein Login der Zuschauer nötig).
//!
//! Zugang: develop.battle.net → API Access → Client anlegen → Client-ID und
//! Secret in den Einstellungen eintragen.

use crate::esi::percent_encode;
use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const REGIONS: [&str; 4] = ["eu", "us", "kr", "tw"];

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WowChar {
    pub name: String,
    pub realm: String,
    pub level: Option<u32>,
    pub class: Option<String>,
    pub race: Option<String>,
    pub guild: Option<String>,
    pub faction: Option<String>,
    pub image: Option<String>,
    pub region: String,
}

impl WowChar {
    /// Kurzzeile für Overlay und Chat, z. B. „Stufe 80 Magier · <Gilde>“.
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        let lc = match (self.level, &self.class) {
            (Some(l), Some(c)) => t!("Stufe {l} {c}", l = l, c = c),
            (Some(l), None) => t!("Stufe {l}", l = l),
            (None, Some(c)) => c.clone(),
            _ => String::new(),
        };
        if !lc.is_empty() {
            parts.push(lc);
        }
        if let Some(g) = self.guild.as_ref().filter(|g| !g.is_empty()) {
            parts.push(format!("<{g}>"));
        }
        parts.join(" · ")
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .user_agent(&t!("CyberGiveaway (WoW character lookup)"))
        .build()
}

/// `!wow Name-Realm`, `!wow Name Realm` oder `!wow Name` (Standard-Realm).
pub fn parse_command(arg: &str, default_realm: &str) -> Option<(String, String)> {
    let arg = crate::esi::clean_name(arg);
    if arg.is_empty() {
        return None;
    }
    let (name, realm) = match arg.split_once(['-', ' ', '@']) {
        Some((n, r)) => (n.trim(), r.trim().trim_start_matches(['-', '@']).trim()),
        None => (arg.as_str(), ""),
    };
    let realm = if realm.is_empty() {
        default_realm.trim()
    } else {
        realm
    };
    if name.is_empty()
        || name.chars().count() > 12
        || name.chars().any(|c| !c.is_alphabetic())
        || realm.is_empty()
    {
        return None;
    }
    Some((name.to_string(), realm.to_string()))
}

/// Realm-Name → Slug, wenn der Realm-Index nicht abrufbar ist.
pub fn slugify(realm: &str) -> String {
    let mut out = String::new();
    for c in realm.trim().to_lowercase().chars() {
        match c {
            ' ' | '_' => out.push('-'),
            '\'' | '’' | '`' => {}
            'ä' => out.push('a'),
            'ö' => out.push('o'),
            'ü' => out.push('u'),
            'ß' => out.push_str("ss"),
            c if c.is_alphanumeric() || c == '-' => out.push(c),
            _ => {}
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

struct Token {
    key: String,
    token: String,
    until: Instant,
}

fn token(id: &str, secret: &str) -> Result<String, String> {
    static T: OnceLock<Mutex<Option<Token>>> = OnceLock::new();
    let lock = T.get_or_init(|| Mutex::new(None));
    let key = format!("{id}:{secret}");
    if let Ok(g) = lock.lock() {
        if let Some(t) = g
            .as_ref()
            .filter(|t| t.key == key && t.until > Instant::now())
        {
            return Ok(t.token.clone());
        }
    }
    let basic = base64::engine::general_purpose::STANDARD.encode(key.as_bytes());
    let v: Value = agent()
        .post("https://oauth.battle.net/token")
        .set("Authorization", &format!("Basic {basic}", basic = basic))
        .send_form(&[("grant_type", "client_credentials")])
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => t!("Blizzard-API: Client-ID oder Secret falsch").to_string(),
            ureq::Error::Status(c, _) => {
                t!("Blizzard-API: Anmeldung fehlgeschlagen (HTTP {c})", c = c)
            }
            other => t!("Blizzard-API nicht erreichbar: {other}", other = other),
        })?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tok = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or(t!("Blizzard-API: kein Token erhalten"))?
        .to_string();
    let secs = v
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600)
        .saturating_sub(120);
    if let Ok(mut g) = lock.lock() {
        *g = Some(Token {
            key,
            token: tok.clone(),
            until: Instant::now() + Duration::from_secs(secs),
        });
    }
    Ok(tok)
}

fn get(url: &str, token: &str) -> Result<Value, ureq::Error> {
    agent()
        .get(url)
        .set("Authorization", &format!("Bearer {token}", token = token))
        .call()?
        .into_json()
        .map_err(|e| ureq::Error::from(std::io::Error::other(e)))
}

/// Realm-Slug über den Realm-Index der Region (zwischengespeichert).
fn realm_slug(region: &str, realm: &str, token: &str) -> String {
    static IDX: OnceLock<Mutex<HashMap<String, Vec<(String, String)>>>> = OnceLock::new();
    let idx = IDX.get_or_init(|| Mutex::new(HashMap::new()));
    let want = norm(realm);
    let have = idx.lock().ok().and_then(|m| m.get(region).cloned());
    let list = match have {
        Some(l) => l,
        None => {
            let locale = if crate::i18n::is_en() { "en_US" } else { "de_DE" };
            let url = format!("https://{region}.api.blizzard.com/data/wow/realm/index?namespace=dynamic-{region}&locale={locale}");
            let l: Vec<(String, String)> = get(&url, token)
                .ok()
                .and_then(|v| v.get("realms").and_then(Value::as_array).cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|r| {
                    Some((
                        r.get("name")?.as_str()?.to_string(),
                        r.get("slug")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            if !l.is_empty() {
                if let Ok(mut m) = idx.lock() {
                    m.insert(region.to_string(), l.clone());
                }
            }
            l
        }
    };
    list.iter()
        .find(|(n, s)| norm(n) == want || norm(s) == want)
        .map(|(_, s)| s.clone())
        .unwrap_or_else(|| slugify(realm))
}

fn sname(v: &Value, key: &str) -> Option<String> {
    let x = v.get(key)?;
    x.get("name")
        .and_then(Value::as_str)
        .or_else(|| x.as_str())
        .map(str::to_string)
}

/// Sucht einen Charakter und dessen Bild. Fehlermeldungen sind für den Chat gedacht.
pub fn lookup(
    id: &str,
    secret: &str,
    region: &str,
    name: &str,
    realm: &str,
) -> Result<WowChar, String> {
    let region = if REGIONS.contains(&region) {
        region
    } else {
        "eu"
    };
    let tok = token(id.trim(), secret.trim())?;
    let slug = realm_slug(region, realm, &tok);
    let base = format!(
        "https://{region}.api.blizzard.com/profile/wow/character/{}/{}",
        percent_encode(&slug),
        percent_encode(&name.to_lowercase())
    );
    let v = get(
        &format!("{base}?namespace=profile-{region}&locale={}", if crate::i18n::is_en() { "en_US" } else { "de_DE" }),
        &tok,
    )
    .map_err(|e| match e {
        ureq::Error::Status(404, _) => {
            t!("Charakter „{name}“ auf {realm} nicht gefunden (oder Profil nicht öffentlich)", name = name, realm = realm)
        }
        ureq::Error::Status(c, _) => t!("Blizzard-API: HTTP {c}", c = c),
        other => t!("Blizzard-API nicht erreichbar: {other}", other = other),
    })?;
    let image = get(
        &format!("{base}/character-media?namespace=profile-{region}"),
        &tok,
    )
    .ok()
    .and_then(|m| {
        let assets = m.get("assets")?.as_array()?.clone();
        ["inset", "avatar", "main-raw"].iter().find_map(|k| {
            assets
                .iter()
                .find(|a| a.get("key").and_then(Value::as_str) == Some(k))
                .and_then(|a| a.get("value").and_then(Value::as_str))
                .map(str::to_string)
        })
    });
    Ok(WowChar {
        name: v
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string(),
        realm: sname(&v, "realm").unwrap_or_else(|| realm.to_string()),
        level: v.get("level").and_then(Value::as_u64).map(|l| l as u32),
        class: sname(&v, "character_class"),
        race: sname(&v, "race"),
        guild: sname(&v, "guild"),
        faction: sname(&v, "faction"),
        image,
        region: region.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(
            parse_command("Thrall-Blackrock", ""),
            Some(("Thrall".into(), "Blackrock".into()))
        );
        assert_eq!(
            parse_command("Thrall Die Aldor", ""),
            Some(("Thrall".into(), "Die Aldor".into()))
        );
        assert_eq!(
            parse_command("\"Jaina\"", "Antonidas"),
            Some(("Jaina".into(), "Antonidas".into()))
        );
        assert_eq!(
            parse_command("Jaina", ""),
            None,
            "ohne Realm und ohne Standard"
        );
        assert_eq!(parse_command("", "x"), None);
        assert_eq!(
            parse_command("Xx_Pro_xX-Realm", ""),
            None,
            "WoW-Namen nur Buchstaben"
        );
        assert_eq!(
            parse_command("Lìlith-Arthas", ""),
            Some(("Lìlith".into(), "Arthas".into()))
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Die Aldor"), "die-aldor");
        assert_eq!(slugify("Anub'arak"), "anubarak");
        assert_eq!(slugify("Kult der Verdammten"), "kult-der-verdammten");
    }

    #[test]
    fn summary() {
        let c = WowChar {
            level: Some(80),
            class: Some("Magier".into()),
            guild: Some("Nachtwache".into()),
            ..Default::default()
        };
        assert_eq!(c.summary(), "Stufe 80 Magier · <Nachtwache>");
        assert_eq!(WowChar::default().summary(), "");
    }
}
