//! EVE-Online-Anbindung: ESI-Charaktersuche, eve2twitch-API und Chat-Import.

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct EveInfo {
    pub name: String,
    pub id: i64,
    pub corp: Option<String>,
    pub alliance: Option<String>,
}

const QUOTES: &[char] = &['"', '\'', '“', '”', '‘', '’', '„', '‟', '«', '»'];
const UA: &str = "Cyber-Giveaway/2.0 (EVE character lookup)";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .user_agent(UA)
        .build()
}

fn cache() -> &'static Mutex<HashMap<String, EveInfo>> {
    static C: OnceLock<Mutex<HashMap<String, EveInfo>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clean_name(name: &str) -> String {
    name.trim()
        .trim_matches(QUOTES)
        .trim()
        .chars()
        .take(500)
        .collect()
}

fn esi_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let body: String = body
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect();
            format!("ESI HTTP {code}: {body}")
        }
        other => format!("ESI nicht erreichbar: {other}"),
    }
}

fn ticker(agent: &ureq::Agent, url: &str) -> Option<String> {
    let v: Value = agent
        .get(url)
        .set("Accept", "application/json")
        .call()
        .ok()?
        .into_json()
        .ok()?;
    v.get("ticker").and_then(Value::as_str).map(str::to_string)
}

/// Sucht einen EVE-Charakter per Name über ESI (inkl. Corp-/Allianz-Ticker).
pub fn validate_character(name: &str) -> Result<EveInfo, String> {
    let clean = clean_name(name);
    if clean.is_empty() {
        return Err("Bitte einen EVE-Charakternamen angeben, z.B. !eve Chribba.".into());
    }
    let key = clean.to_lowercase();
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(hit);
    }
    let a = agent();
    let ids: Value = a
        .post("https://esi.evetech.net/latest/universe/ids/")
        .set("Accept", "application/json")
        .send_json(serde_json::json!([clean]))
        .map_err(esi_error)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let ch = ids
        .get("characters")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or("EVE-Charakter nicht gefunden")?;
    let id = ch
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("EVE-Charakter nicht gefunden")?;
    let real_name = ch
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&clean)
        .to_string();
    let detail: Value = a
        .get(&format!("https://esi.evetech.net/latest/characters/{id}/"))
        .set("Accept", "application/json")
        .call()
        .map_err(esi_error)?
        .into_json()
        .map_err(|e| e.to_string())?;
    let corp = detail
        .get("corporation_id")
        .and_then(Value::as_i64)
        .and_then(|c| {
            ticker(
                &a,
                &format!("https://esi.evetech.net/latest/corporations/{c}/"),
            )
        });
    let alliance = detail
        .get("alliance_id")
        .and_then(Value::as_i64)
        .and_then(|c| {
            ticker(
                &a,
                &format!("https://esi.evetech.net/latest/alliances/{c}/"),
            )
        });
    let info = EveInfo {
        name: real_name,
        id,
        corp,
        alliance,
    };
    if let Ok(mut c) = cache().lock() {
        c.insert(key, info.clone());
    }
    Ok(info)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Fragt die eve2twitch-API nach dem Hauptcharakter eines Twitch-Logins.
pub fn lookup_eve2twitch(base: &str, key: &str, login: &str) -> Option<EveInfo> {
    let url = format!(
        "{}/api/lookup/twitch/{}",
        base.trim_end_matches('/'),
        percent_encode(login)
    );
    let v: Value = agent()
        .get(&url)
        .set("X-API-Key", key)
        .set("Accept", "application/json")
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let c = v.get("main_character")?;
    let name = c.get("name")?.as_str()?.to_string();
    let id = c.get("id")?.as_i64()?;
    if name.is_empty() || id == 0 {
        return None;
    }
    Some(EveInfo {
        name,
        id,
        corp: c.get("corp").and_then(Value::as_str).map(str::to_string),
        alliance: c
            .get("alliance")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Erkennt Nachrichten des Eve2Twitch-Bots: `twitchname: IGN: "Charaktername"`.
pub fn parse_eve2twitch_message(msg: &str) -> Option<(String, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?i)^\s*(?:[A-Za-z0-9_]+\s*:\s*)?@?(?P<twitch>[A-Za-z0-9_]{1,25})\s*:\s*IGN\s*:?\s*["'“”‘’„‟«»](?P<ign>[^"'“”‘’„‟«»]+)["'“”‘’„‟«»]\s*$"#)
            .expect("regex")
    });
    let c = re.captures(msg)?;
    let twitch = c
        .name("twitch")?
        .as_str()
        .trim()
        .trim_start_matches('@')
        .to_lowercase();
    let ign = c.name("ign")?.as_str().trim().to_string();
    if twitch.is_empty() || ign.is_empty() {
        return None;
    }
    Some((twitch, ign))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eve2twitch_parse() {
        assert_eq!(
            parse_eve2twitch_message(r#"areadox: IGN: "daulin""#),
            Some(("areadox".into(), "daulin".into()))
        );
        assert_eq!(
            parse_eve2twitch_message("bot: @Foo_1 : IGN „Some Pilot“"),
            Some(("foo_1".into(), "Some Pilot".into()))
        );
        assert_eq!(parse_eve2twitch_message("hallo"), None);
        assert_eq!(clean_name("  „Chribba“ "), "Chribba");
    }
}
