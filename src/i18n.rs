//! Sprache des Programms (Deutsch/Englisch).
//!
//! Alle Texte stehen im Quelltext auf Deutsch. `t!("Text")` liefert bei
//! Englisch die Übersetzung aus `assets/i18n/en.json` (Schlüssel = deutscher
//! Text), sonst den deutschen Text. Platzhalter heißen `{name}` und werden mit
//! `t!("… {name} …", name = wert)` ersetzt. Das Dock nutzt dieselbe Datei.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

pub const EN_JSON: &str = include_str!("../assets/i18n/en.json");

static ENGLISH: AtomicBool = AtomicBool::new(false);

fn table() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(EN_JSON).unwrap_or_default())
}

/// "de" oder "en"; alles andere gilt als Deutsch.
pub fn set_lang(lang: &str) {
    ENGLISH.store(lang == "en", Ordering::Relaxed);
}

pub fn is_en() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

#[allow(dead_code)]
pub fn lang() -> &'static str {
    if is_en() {
        "en"
    } else {
        "de"
    }
}

/// Übersetzung eines deutschen Textes (ohne Eintrag: der Text selbst).
pub fn tr(de: &str) -> String {
    if is_en() {
        if let Some(en) = table().get(de) {
            return en.clone();
        }
    }
    de.to_string()
}


/// `t!("Text")` oder `t!("Hallo {name}", name = x)`.
macro_rules! t {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut s = $crate::i18n::tr($s);
        $( s = s.replace(concat!("{", stringify!($k), "}"), &($v).to_string()); )+
        s
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texte, die in beiden Sprachen gleich bleiben (Technik, Eigennamen).
    const SAME: &[&str] = &[
        "Text", "… {name} …", "Hallo {name}", "Cyber Giveaway", "Asset Safety", "mm:ss",
        "Sprache / Language", "Deutsch", "English", "Giveaway", "EVE", "EU", "US", "KR",
        "TW", "Port", "develop.battle.net", "CyberGiveaway (WoW character lookup)",
        "ESI HTTP {code}: {body}", "KEYWORD", "RUN (s)", "START", "BREACH", "RESET",
        "Twitch", "Name", "Info", "Updates", "Item", "Overlay", "Sound", "Keyword",
        "Run (s)", "Pause", "Start", "Scopes:", "Dock: START/STOP", "Dock: BREACH",
        "OVERLAY: {a1} | DEMO: {a1}/test", "STATUS: {s}", "COUNTDOWN: {s}",
        "NICK {nick}\r\nJOIN #{channel}\r\n", "PASS {token}\r\nNICK {a1}\r\nJOIN #{channel}\r\n",
    ];

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(k, _)| k.to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn platzhalter_stimmen_ueberein() {
        for (de, en) in table() {
            assert_eq!(placeholders(de), placeholders(en), "{de} → {en}");
        }
    }

    /// Jeder `t!("…")`-Text im Quelltext hat eine englische Übersetzung.
    #[test]
    fn alle_texte_uebersetzt() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs")
                || path.ends_with("i18n.rs")
            {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            let mut rest = src.as_str();
            while let Some(i) = rest.find("t!(\"") {
                let before = rest[..i].chars().last();
                rest = &rest[i + 4..];
                if matches!(before, Some(c) if c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let mut lit = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some('n') => lit.push('\n'),
                            Some('r') => lit.push('\r'),
                            Some('\n') => {
                                let tail = chars.as_str().trim_start();
                                chars = tail.chars();
                            }
                            Some(o) => lit.push(o),
                            None => break,
                        },
                        '"' => break,
                        c => lit.push(c),
                    }
                }
                if !table().contains_key(&lit) && !SAME.contains(&lit.as_str()) {
                    missing.push(format!("{}: {lit}", path.display()));
                }
            }
        }
        assert!(missing.is_empty(), "ohne Übersetzung:\n{}", missing.join("\n"));
    }

    // Kein set_lang hier: Tests laufen parallel und die Sprache ist global.
    #[test]
    fn woerterbuch_lesbar() {
        assert_eq!(table().get("Speichern").map(String::as_str), Some("Save"));
        assert_eq!(t!("@{name} ist dabei.", name = "x"), "@x ist dabei.");
    }
}
