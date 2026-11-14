//! Chat-Dock: Twitch-Chat live mitlesen (über den anonymen Reader) und an
//! das OBS-Chat-Dock (`/chat`, WebSocket `/ws/chat`) weitergeben.
//!
//! Der Verlauf der letzten Nachrichten liegt hier, damit ein neu geöffnetes
//! Dock sofort etwas zeigt. Gesendete Nachrichten merkt sich die Datenbank.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;

const KEEP: usize = 200;

struct Hub {
    lines: Mutex<VecDeque<Value>>,
    connected: Mutex<(bool, String)>,
    tx: broadcast::Sender<String>,
}

fn hub() -> &'static Hub {
    static H: OnceLock<Hub> = OnceLock::new();
    H.get_or_init(|| Hub {
        lines: Mutex::new(VecDeque::new()),
        connected: Mutex::new((false, String::new())),
        tx: broadcast::channel(256).0,
    })
}

fn emit(v: Value) {
    let _ = hub().tx.send(v.to_string());
}

/// Momentaufnahme für ein neu verbundenes Dock plus Empfänger für alles Weitere.
pub fn subscribe() -> (String, broadcast::Receiver<String>) {
    let h = hub();
    let rx = h.tx.subscribe();
    let lines: Vec<Value> = h.lines.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default();
    let (connected, channel) = h.connected.lock().map(|c| c.clone()).unwrap_or_default();
    let snap = json!({"type": "history", "lines": lines, "connected": connected, "channel": channel});
    (snap.to_string(), rx)
}

/// Verbindungszustand des Chat-Lesers
pub fn set_connected(on: bool, channel: &str) {
    if let Ok(mut c) = hub().connected.lock() {
        *c = (on, channel.to_string());
    }
    emit(json!({"type": "status", "connected": on, "channel": channel}));
}

/// Gespeicherte Nachrichten haben sich geändert – Docks laden die Liste neu.
pub fn saved_changed() {
    emit(json!({"type": "saved"}));
}

/// IRC-Tags (`@a=1;b=2 …`) als Tabelle
pub fn parse_tags(line: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(rest) = line.strip_prefix('@') {
        let tags = rest.split_once(' ').map_or(rest, |(t, _)| t);
        for kv in tags.split(';') {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            let v = v
                .replace("\\s", " ")
                .replace("\\:", ";")
                .replace("\\\\", "\\")
                .replace("\\r", "")
                .replace("\\n", "");
            m.insert(k.to_string(), v);
        }
    }
    m
}

/// Eine Chatnachricht für das Dock aufbereiten.
pub fn line_from_irc(line: &str) -> Option<Value> {
    let (user, msg) = crate::twitch::parse_privmsg(line)?;
    let tags = parse_tags(line);
    let get = |k: &str| tags.get(k).filter(|v| !v.is_empty()).cloned();
    // „/me …“ kommt als CTCP ACTION
    let (text, action) = match msg
        .strip_prefix("\u{1}ACTION ")
        .map(|m| m.trim_end_matches('\u{1}'))
    {
        Some(m) => (m.to_string(), true),
        None => (msg, false),
    };
    let badges: Vec<String> = get("badges")
        .unwrap_or_default()
        .split(',')
        .filter_map(|b| b.split('/').next())
        .filter(|b| matches!(*b, "broadcaster" | "moderator" | "vip" | "subscriber" | "founder"))
        .map(str::to_string)
        .collect();
    Some(json!({
        "id": get("id"),
        "user": user,
        "name": get("display-name").unwrap_or_else(|| user.clone()),
        "color": get("color"),
        "badges": badges,
        "emotes": get("emotes"),
        "text": text,
        "action": action,
        "ts": chrono::Local::now().format("%H:%M").to_string(),
    }))
}

/// Jede IRC-Zeile des Lesers hier durchreichen (Nachrichten, Löschungen).
pub fn handle_irc(line: &str) {
    let h = hub();
    if let Some(v) = line_from_irc(line) {
        if let Ok(mut l) = h.lines.lock() {
            l.push_back(v.clone());
            while l.len() > KEEP {
                l.pop_front();
            }
        }
        emit(json!({"type": "msg", "line": v}));
        return;
    }
    let body = line
        .strip_prefix('@')
        .map_or(line, |r| r.split_once(' ').map_or("", |(_, b)| b));
    if body.contains(" CLEARMSG #") {
        // Moderator hat eine einzelne Nachricht gelöscht
        if let Some(id) = parse_tags(line).get("target-msg-id").cloned() {
            if let Ok(mut l) = h.lines.lock() {
                l.retain(|x| x["id"].as_str() != Some(id.as_str()));
            }
            emit(json!({"type": "delete", "id": id}));
        }
    } else if let Some(rest) = body.split_once(" CLEARCHAT #").map(|(_, r)| r) {
        // Timeout/Bann (mit Name) oder ganzer Chat geleert
        let user = rest.split_once(" :").map(|(_, u)| u.trim().to_lowercase());
        if let Ok(mut l) = h.lines.lock() {
            match &user {
                Some(u) => l.retain(|x| x["user"].as_str() != Some(u.as_str())),
                None => l.clear(),
            }
        }
        emit(json!({"type": "clear", "user": user}));
    }
}

/// Text aus dem Dock für IRC vorbereiten: „/me“ wird zur ACTION,
/// andere Slash-Befehle nimmt Twitch über IRC nicht mehr an.
pub fn outgoing(text: &str) -> Result<String, String> {
    let t: String = text.replace(['\r', '\n'], " ").trim().chars().take(450).collect();
    if t.is_empty() {
        return Err(t!("Die Nachricht ist leer."));
    }
    if let Some(m) = t.strip_prefix("/me ") {
        return Ok(format!("\u{1}ACTION {}\u{1}", m.trim()));
    }
    if t.starts_with('/') && !t.starts_with("//") {
        return Err(t!("Befehle wie /ban oder /timeout nimmt Twitch über den Bot nicht an – nur /me geht."));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nachricht_mit_tags() {
        let l = r"@badge-info=;badges=moderator/1,subscriber/12;color=#FF4500;display-name=Areadox;emotes=25:0-4;id=abc-1 :areadox!areadox@areadox.tmi.twitch.tv PRIVMSG #areadox :Kappa hallo\s";
        let v = line_from_irc(l).unwrap();
        assert_eq!(v["name"], "Areadox");
        assert_eq!(v["color"], "#FF4500");
        assert_eq!(v["badges"], json!(["moderator", "subscriber"]));
        assert_eq!(v["emotes"], "25:0-4");
        assert_eq!(v["id"], "abc-1");
        assert_eq!(v["action"], false);
        let me = line_from_irc(":foo!foo@foo PRIVMSG #x :\u{1}ACTION winkt\u{1}").unwrap();
        assert_eq!(me["text"], "winkt");
        assert_eq!(me["action"], true);
        assert_eq!(me["name"], "foo");
    }

    #[test]
    fn ausgehend() {
        assert_eq!(outgoing(" hallo\nwelt ").unwrap(), "hallo welt");
        assert_eq!(outgoing("/me winkt").unwrap(), "\u{1}ACTION winkt\u{1}");
        assert!(outgoing("/ban x").is_err());
        assert!(outgoing("   ").is_err());
    }

    #[test]
    fn tags_escapes() {
        let t = parse_tags(r"@a=x\sy;b=1\:2;c= :rest");
        assert_eq!(t["a"], "x y");
        assert_eq!(t["b"], "1;2");
    }
}
