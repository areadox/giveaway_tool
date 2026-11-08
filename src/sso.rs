//! EVE-Anmeldung (SSO v2 mit PKCE, ohne Client-Secret) und Abruf der
//! eigenen Assets über ESI, damit Items als Giveaway-Preise markiert werden können.
//!
//! Benötigt eine eigene App auf <https://developers.eveonline.com> mit der
//! Callback-URL `http://localhost:<Port>/eve/callback` und den Scopes
//! `esi-assets.read_assets.v1` und `esi-universe.read_structures.v1`.

use crate::esi::percent_encode;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const SCOPES: &str = "esi-assets.read_assets.v1 esi-universe.read_structures.v1";
const AUTHORIZE: &str = "https://login.eveonline.com/v2/oauth/authorize/";
const TOKEN: &str = "https://login.eveonline.com/v2/oauth/token";
const ESI: &str = "https://esi.evetech.net/latest";
const UA: &str = "Cyber-Giveaway (asset picker)";

/// Laufende Anmeldung (zwischen Browser-Aufruf und Callback).
#[derive(Debug, Clone)]
pub struct Pending {
    pub state: String,
    pub verifier: String,
    pub redirect: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Login {
    pub refresh_token: String,
    pub access_token: String,
    pub character_id: i64,
    pub character_name: String,
}

/// Ein Asset in lesbarer Form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetRow {
    pub item_id: i64,
    pub type_id: i64,
    pub name: String,
    pub quantity: u64,
    /// Station/Struktur, ggf. mit Container-/Schiffsnamen: „Jita IV - Moon 4 › Preis-Box“
    pub location: String,
}

pub fn callback_url(port: u16) -> String {
    format!("http://localhost:{port}/eve/callback")
}

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Erzeugt die Login-URL für den Browser.
pub fn begin(client_id: &str, port: u16) -> (String, Pending) {
    let pending = Pending {
        state: random_b64(16),
        verifier: random_b64(32),
        redirect: callback_url(port),
    };
    let url = format!(
        "{AUTHORIZE}?response_type=code&redirect_uri={}&client_id={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        percent_encode(&pending.redirect),
        percent_encode(client_id.trim()),
        percent_encode(SCOPES),
        challenge(&pending.verifier),
        pending.state
    );
    (url, pending)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body: String = resp
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            format!("HTTP {code}: {}", body.trim())
        }
        other => format!("nicht erreichbar: {other}"),
    }
}

/// Liest Charakter-ID und -Name aus dem Access-Token (JWT).
pub fn character_from_jwt(token: &str) -> Result<(i64, String), String> {
    let payload = token.split('.').nth(1).ok_or("Token ohne Inhalt")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|e| format!("Token nicht lesbar: {e}"))?;
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Token nicht lesbar: {e}"))?;
    let sub = v
        .get("sub")
        .and_then(Value::as_str)
        .ok_or("Token ohne Charakter")?;
    let id = sub
        .rsplit(':')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or("Charakter-ID fehlt im Token")?;
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((id, name))
}

fn token_request(form: &[(&str, &str)]) -> Result<Login, String> {
    let v: Value = agent()
        .post(TOKEN)
        .set("Host", "login.eveonline.com")
        .send_form(form)
        .map_err(|e| format!("EVE-Login {}", http_err(e)))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let access = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Kein Access-Token erhalten")?
        .to_string();
    let refresh = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .ok_or("Kein Refresh-Token erhalten")?
        .to_string();
    let (character_id, character_name) = character_from_jwt(&access)?;
    Ok(Login {
        refresh_token: refresh,
        access_token: access,
        character_id,
        character_name,
    })
}

/// Tauscht den Code aus dem Callback gegen Tokens.
pub fn exchange(client_id: &str, code: &str, pending: &Pending) -> Result<Login, String> {
    token_request(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id.trim()),
        ("code_verifier", &pending.verifier),
    ])
}

/// Holt mit dem Refresh-Token ein frisches Access-Token (der Refresh-Token kann sich dabei ändern).
pub fn refresh(client_id: &str, refresh_token: &str) -> Result<Login, String> {
    token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id.trim()),
    ])
}

#[derive(Deserialize, Clone, Debug)]
struct RawAsset {
    item_id: i64,
    type_id: i64,
    quantity: i64,
    location_id: i64,
    location_flag: String,
    #[serde(default)]
    is_singleton: bool,
    #[serde(default)]
    is_blueprint_copy: Option<bool>,
}

#[allow(clippy::result_large_err)] // ureq::Error ist groß, aber selten
fn esi_get(a: &ureq::Agent, path: &str, token: &str) -> Result<ureq::Response, ureq::Error> {
    a.get(&format!("{ESI}{path}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .call()
}

/// Lädt alle Assets des angemeldeten Charakters inklusive Namen und Orten.
pub fn load_assets(login: &Login) -> Result<Vec<AssetRow>, String> {
    let a = agent();
    let mut raw: Vec<RawAsset> = Vec::new();
    let mut page = 1;
    loop {
        let resp = esi_get(
            &a,
            &format!("/characters/{}/assets/?page={page}", login.character_id),
            &login.access_token,
        )
        .map_err(|e| format!("Assets {}", http_err(e)))?;
        let pages: u32 = resp
            .header("X-Pages")
            .and_then(|p| p.parse().ok())
            .unwrap_or(1);
        let mut chunk: Vec<RawAsset> = resp.into_json().map_err(|e| e.to_string())?;
        raw.append(&mut chunk);
        if page >= pages || page >= 100 {
            break;
        }
        page += 1;
    }
    Ok(resolve(&a, login, raw))
}

const SKIP_FLAGS: [&str; 3] = ["Implant", "Skill", "SkillInTraining"];

/// Wurzel-Ort eines Items (Station/Struktur/System) und die Kette der
/// Container/Schiffe von außen nach innen.
fn root_path(by_id: &HashMap<i64, &RawAsset>, r: &RawAsset) -> (i64, Vec<i64>) {
    let mut chain = vec![];
    let mut loc = r.location_id;
    while let Some(parent) = by_id.get(&loc) {
        chain.push(parent.item_id);
        loc = parent.location_id;
        if chain.len() > 20 {
            break;
        }
    }
    chain.reverse();
    (loc, chain)
}

fn resolve(a: &ureq::Agent, login: &Login, raw: Vec<RawAsset>) -> Vec<AssetRow> {
    let by_id: HashMap<i64, &RawAsset> = raw.iter().map(|r| (r.item_id, r)).collect();
    let root = |r: &RawAsset| root_path(&by_id, r);

    // Namen von Typen sowie Stationen/Systemen über /universe/names
    let mut ids: HashSet<i64> = raw.iter().map(|r| r.type_id).collect();
    let mut structures: HashSet<i64> = HashSet::new();
    for r in &raw {
        let (loc, _) = root(r);
        if (30_000_000..34_000_000).contains(&loc) || (60_000_000..64_000_000).contains(&loc) {
            ids.insert(loc);
        } else if loc > 1_000_000_000_000 {
            structures.insert(loc);
        }
    }
    let mut names: HashMap<i64, String> = HashMap::new();
    let id_list: Vec<i64> = ids.into_iter().collect();
    for chunk in id_list.chunks(1000) {
        if let Ok(resp) = a
            .post(&format!("{ESI}/universe/names/"))
            .send_json(serde_json::json!(chunk))
        {
            if let Ok(Value::Array(arr)) = resp.into_json::<Value>() {
                for e in arr {
                    if let (Some(id), Some(n)) = (
                        e.get("id").and_then(Value::as_i64),
                        e.get("name").and_then(Value::as_str),
                    ) {
                        names.insert(id, n.to_string());
                    }
                }
            }
        }
    }
    for sid in structures {
        let n = esi_get(
            a,
            &format!("/universe/structures/{sid}/"),
            &login.access_token,
        )
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("Struktur {sid}"));
        names.insert(sid, n);
    }
    // Eigene Namen von Schiffen/Containern, in denen etwas liegt
    let parents: Vec<i64> = raw
        .iter()
        .filter(|r| r.is_singleton && raw.iter().any(|c| c.location_id == r.item_id))
        .map(|r| r.item_id)
        .collect();
    let mut item_names: HashMap<i64, String> = HashMap::new();
    for chunk in parents.chunks(1000) {
        let resp = a
            .post(&format!(
                "{ESI}/characters/{}/assets/names/",
                login.character_id
            ))
            .set("Authorization", &format!("Bearer {}", login.access_token))
            .send_json(serde_json::json!(chunk));
        if let Ok(Value::Array(arr)) = resp
            .map_err(|_| ())
            .and_then(|r| r.into_json::<Value>().map_err(|_| ()))
        {
            for e in arr {
                if let (Some(id), Some(n)) = (
                    e.get("item_id").and_then(Value::as_i64),
                    e.get("name").and_then(Value::as_str),
                ) {
                    if !n.trim().is_empty() && n != "None" {
                        item_names.insert(id, n.to_string());
                    }
                }
            }
        }
    }

    build_rows(&raw, &names, &item_names)
}

/// Setzt aus Rohdaten und aufgelösten Namen die lesbaren Zeilen zusammen.
fn build_rows(
    raw: &[RawAsset],
    names: &HashMap<i64, String>,
    item_names: &HashMap<i64, String>,
) -> Vec<AssetRow> {
    let by_id: HashMap<i64, &RawAsset> = raw.iter().map(|r| (r.item_id, r)).collect();
    let mut rows: Vec<AssetRow> = raw
        .iter()
        .filter(|r| !SKIP_FLAGS.contains(&r.location_flag.as_str()))
        .map(|r| {
            let (loc, chain) = root_path(&by_id, r);
            let mut location = if r.location_flag == "AssetSafety" || loc == 2004 {
                "Asset Safety".to_string()
            } else {
                names
                    .get(&loc)
                    .cloned()
                    .unwrap_or_else(|| format!("Ort {loc}"))
            };
            for c in chain {
                let label = item_names
                    .get(&c)
                    .cloned()
                    .or_else(|| by_id.get(&c).and_then(|p| names.get(&p.type_id).cloned()))
                    .unwrap_or_else(|| "Container".into());
                location.push_str(" › ");
                location.push_str(&label);
            }
            let mut name = names
                .get(&r.type_id)
                .cloned()
                .unwrap_or_else(|| format!("Typ {}", r.type_id));
            if r.is_blueprint_copy == Some(true) {
                name.push_str(" (Kopie)");
            }
            AssetRow {
                item_id: r.item_id,
                type_id: r.type_id,
                name,
                quantity: r.quantity.max(1) as u64,
                location,
            }
        })
        .collect();
    rows.sort_by(|x, y| x.location.cmp(&y.location).then(x.name.cmp(&y.name)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_and_url() {
        // Testvektor aus RFC 7636, Anhang B
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (url, p) = begin("abc123", 8766);
        assert!(
            url.starts_with("https://login.eveonline.com/v2/oauth/authorize/?response_type=code")
        );
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8766%2Feve%2Fcallback"));
        assert!(url.contains("scope=esi-assets.read_assets.v1%20esi-universe.read_structures.v1"));
        assert!(url.contains(&format!("state={}", p.state)));
        assert!(url.contains(&challenge(&p.verifier)));
        assert!(p.verifier.len() >= 43);
    }

    #[test]
    fn jwt_character() {
        let payload =
            URL_SAFE_NO_PAD.encode(r#"{"sub":"CHARACTER:EVE:2112625428","name":"daulin"}"#);
        let token = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        assert_eq!(
            character_from_jwt(&token).unwrap(),
            (2112625428, "daulin".into())
        );
        assert!(character_from_jwt("kaputt").is_err());
    }

    #[test]
    fn rows_with_containers() {
        let asset = |item_id, type_id, quantity, location_id, flag: &str, singleton| RawAsset {
            item_id,
            type_id,
            quantity,
            location_id,
            location_flag: flag.into(),
            is_singleton: singleton,
            is_blueprint_copy: None,
        };
        let mut raw = vec![
            asset(1, 17715, 1, 60003760, "Hangar", true), // Gila im Hangar
            asset(2, 3001, 1500, 1, "Cargo", false),      // Paste im Frachtraum der Gila
            asset(3, 9999, 1, 60003760, "Implant", true), // Implantat: ausgeblendet
            asset(4, 17715, 1, 1_035_466_617_946, "Hangar", true), // in einer Struktur
        ];
        raw.push(RawAsset {
            is_blueprint_copy: Some(true),
            ..asset(5, 683, 1, 60003760, "Hangar", true)
        });
        let names: HashMap<i64, String> = [
            (17715, "Gila"),
            (3001, "Nanite Repair Paste"),
            (60003760, "Jita IV - Moon 4 - Caldari Navy Assembly Plant"),
            (683, "Rifter Blueprint"),
            (1_035_466_617_946, "Perimeter - Tranquility Trading Tower"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
        let item_names: HashMap<i64, String> =
            [(1, "Preis-Gila".to_string())].into_iter().collect();
        let rows = build_rows(&raw, &names, &item_names);
        assert_eq!(rows.len(), 4);
        let paste = rows.iter().find(|r| r.item_id == 2).unwrap();
        assert_eq!(paste.quantity, 1500);
        assert_eq!(
            paste.location,
            "Jita IV - Moon 4 - Caldari Navy Assembly Plant › Preis-Gila"
        );
        assert!(rows.iter().any(|r| r.name == "Rifter Blueprint (Kopie)"));
        assert!(rows
            .iter()
            .any(|r| r.item_id == 4 && r.location.starts_with("Perimeter")));
        assert!(rows.iter().all(|r| r.item_id != 3));
    }
}
