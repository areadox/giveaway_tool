//! Designs (Themes): Look (CSS) und Begriffe (Wörterbuch) je Spiel.
//!
//! Ein Design besteht aus `theme.json` (Name, Form der Knoten, Beschriftungen
//! des Overlays, Texte des Runs, Listen) und optional `theme.css` (Farben,
//! Schrift, Rahmen). Fehlende Einträge kommen aus dem Standard-Design
//! (Shadowrun). Eigene Designs liegen im Ordner `themes/<id>/` neben der .exe;
//! Ordner, die mit `_` beginnen (z. B. `_vorlage`), werden nicht geladen.

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

pub const DEFAULT_ID: &str = "shadowrun";
const DEFAULT_JSON: &str = include_str!("../assets/themes/shadowrun/theme.json");

/// Eingebaute Designs: (id, theme.json, theme.css)
const BUILTIN: &[(&str, &str, &str)] = &[
    (
        "shadowrun",
        DEFAULT_JSON,
        include_str!("../assets/themes/shadowrun/theme.css"),
    ),
    (
        "eve",
        include_str!("../assets/themes/eve/theme.json"),
        include_str!("../assets/themes/eve/theme.css"),
    ),
    (
        "wow",
        include_str!("../assets/themes/wow/theme.json"),
        include_str!("../assets/themes/wow/theme.css"),
    ),
    (
        "palia",
        include_str!("../assets/themes/palia/theme.json"),
        include_str!("../assets/themes/palia/theme.css"),
    ),
    (
        "anime",
        include_str!("../assets/themes/anime/theme.json"),
        include_str!("../assets/themes/anime/theme.css"),
    ),
];

const TEMPLATE_CSS: &str = include_str!("../assets/themes/_vorlage.css");
const TEMPLATE_README: &str = include_str!("../assets/themes/LIESMICH.txt");

/// Texte und Listen des Runs.
#[derive(Debug, Clone, Default)]
pub struct Lexicon {
    texts: HashMap<String, String>,
    lists: HashMap<String, Vec<String>>,
}

impl Lexicon {
    /// Text zu einem Schlüssel; `{name}` wird durch die Argumente ersetzt.
    pub fn t(&self, key: &str, args: &[(&str, String)]) -> String {
        let mut s = self
            .texts
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string());
        for (k, v) in args {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }

    pub fn list(&self, key: &str) -> &[String] {
        self.lists.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    #[cfg(test)]
    pub fn standard() -> Self {
        let v: Value = serde_json::from_str(DEFAULT_JSON).expect("Standard-Design");
        let mut lx = Lexicon::default();
        lx.merge(&v);
        lx
    }

    fn merge(&mut self, v: &Value) {
        if let Some(Value::Object(t)) = v.get("texts") {
            for (k, val) in t {
                if let Some(s) = val.as_str() {
                    self.texts.insert(k.clone(), s.to_string());
                }
            }
        }
        if let Some(Value::Object(l)) = v.get("lists") {
            for (k, val) in l {
                if let Some(arr) = val.as_array() {
                    let items: Vec<String> = arr
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                    if !items.is_empty() {
                        self.lists.insert(k.clone(), items);
                    }
                }
            }
        }
    }
}

/// Was Overlay und Timer über ein Design wissen müssen.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub font_url: String,
    /// Knotenform im Overlay: hex | ring | diamond | circle | star
    pub shape: String,
    pub ui: Map<String, Value>,
    pub builtin: bool,
}

#[derive(Debug)]
pub struct Theme {
    pub info: ThemeInfo,
    pub lexicon: Lexicon,
    pub css: String,
}

fn parse(id: &str, json: &str, css: &str, builtin: bool) -> Result<Theme, String> {
    let base: Value = serde_json::from_str(DEFAULT_JSON).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(json).map_err(|e| format!("theme.json: {e}"))?;
    let mut lexicon = Lexicon::default();
    lexicon.merge(&base);
    lexicon.merge(&v);
    let mut ui = base
        .get("ui")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(Value::Object(u)) = v.get("ui") {
        for (k, val) in u {
            ui.insert(k.clone(), val.clone());
        }
    }
    let s = |k: &str, d: &str| v.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
    let base_font = base.get("font_url").and_then(Value::as_str).unwrap_or("");
    let shape = s("shape", "hex");
    let shape = if ["hex", "ring", "diamond", "circle", "star"].contains(&shape.as_str()) {
        shape
    } else {
        "hex".into()
    };
    Ok(Theme {
        info: ThemeInfo {
            id: id.to_string(),
            name: s("name", id),
            description: s("description", ""),
            font_url: s("font_url", base_font),
            shape,
            ui,
            builtin,
        },
        lexicon,
        css: css.to_string(),
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('_')
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Alle verfügbaren Designs; wird beim Start und auf Wunsch neu geladen.
pub struct Registry {
    dir: Option<PathBuf>,
    themes: RwLock<BTreeMap<String, Arc<Theme>>>,
    /// Zählt Neuladungen hoch, damit Overlays geänderte Designs neu abholen.
    version: AtomicU64,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// Gemeinsame Design-Liste (eigene Designs aus `themes/` neben der Config).
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let dir = if cfg!(test) {
            None
        } else {
            crate::config::config_path()
                .parent()
                .map(|p| p.join("themes"))
        };
        let (reg, msgs) = Registry::load(dir);
        for m in msgs {
            eprintln!("{m}");
        }
        reg
    })
}

impl Registry {
    /// Lädt eingebaute Designs und – falls `dir` gesetzt – eigene aus dem Ordner.
    /// Gibt zusätzlich Meldungen zu fehlerhaften eigenen Designs zurück.
    pub fn load(dir: Option<PathBuf>) -> (Self, Vec<String>) {
        let reg = Self {
            dir,
            themes: RwLock::new(BTreeMap::new()),
            version: AtomicU64::new(0),
        };
        let msgs = reg.reload();
        (reg, msgs)
    }

    pub fn reload(&self) -> Vec<String> {
        let mut map = BTreeMap::new();
        let mut msgs = vec![];
        for (id, json, css) in BUILTIN {
            match parse(id, json, css, true) {
                Ok(t) => {
                    map.insert(id.to_string(), Arc::new(t));
                }
                Err(e) => msgs.push(format!("Eingebautes Design {id}: {e}")),
            }
        }
        if let Some(dir) = &self.dir {
            ensure_template(dir);
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
                entries.sort_by_key(|e| e.file_name());
                for e in entries {
                    let id = e.file_name().to_string_lossy().to_lowercase();
                    let path = e.path();
                    if !path.is_dir() || id.starts_with('_') {
                        continue;
                    }
                    if !valid_id(&id) {
                        msgs.push(format!(
                            "Design-Ordner „{id}“: Name nur aus a–z, 0–9, - und _"
                        ));
                        continue;
                    }
                    let Ok(json) = std::fs::read_to_string(path.join("theme.json")) else {
                        msgs.push(format!("Design „{id}“: theme.json fehlt"));
                        continue;
                    };
                    let css = std::fs::read_to_string(path.join("theme.css")).unwrap_or_default();
                    match parse(&id, &json, &css, false) {
                        Ok(t) => {
                            map.insert(id, Arc::new(t));
                        }
                        Err(err) => msgs.push(format!("Design „{id}“: {err}")),
                    }
                }
            }
        }
        if let Ok(mut w) = self.themes.write() {
            *w = map;
        }
        self.version.fetch_add(1, Ordering::Relaxed);
        msgs
    }

    /// Design zu einer ID; unbekannte IDs fallen auf das Standard-Design zurück.
    pub fn get(&self, id: &str) -> Arc<Theme> {
        let map = self.themes.read().expect("themes");
        map.get(id)
            .or_else(|| map.get(DEFAULT_ID))
            .cloned()
            .expect("Standard-Design vorhanden")
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// (id, Name, eingebaut) aller Designs, Standard zuerst.
    pub fn list(&self) -> Vec<(String, String, bool)> {
        let map = self.themes.read().expect("themes");
        let mut v: Vec<_> = map
            .values()
            .map(|t| (t.info.id.clone(), t.info.name.clone(), t.info.builtin))
            .collect();
        v.sort_by_key(|(id, name, builtin)| (id != DEFAULT_ID, !builtin, name.to_lowercase()));
        v
    }
}

/// Legt `themes/_vorlage` (vollständige Vorlage) und `LIESMICH.txt` an, falls nicht vorhanden.
fn ensure_template(dir: &Path) {
    let tpl = dir.join("_vorlage");
    if tpl.exists() {
        return;
    }
    if std::fs::create_dir_all(&tpl).is_ok() {
        let json = DEFAULT_JSON.replacen("\"Shadowrun – Matrix-Run\"", "\"Mein Design\"", 1);
        let _ = std::fs::write(tpl.join("theme.json"), json);
        let _ = std::fs::write(tpl.join("theme.css"), TEMPLATE_CSS);
        let _ = std::fs::write(dir.join("LIESMICH.txt"), TEMPLATE_README);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_complete() {
        let (reg, msgs) = Registry::load(None);
        assert!(msgs.is_empty(), "{msgs:?}");
        let list = reg.list();
        assert_eq!(list[0].0, DEFAULT_ID);
        assert_eq!(list.len(), BUILTIN.len());
        let std = Lexicon::standard();
        for (id, ..) in BUILTIN {
            let t = reg.get(id);
            // Jedes Design hat alle Listen mit sinnvollen Längen
            assert_eq!(t.lexicon.list("nodes").len(), 7, "{id}: 7 Knoten");
            for l in ["hosts", "sculpts", "decks", "folders"] {
                assert!(!t.lexicon.list(l).is_empty(), "{id}: Liste {l}");
            }
            // Platzhalter in überschriebenen Texten müssen zum Standard passen
            for (k, v) in &t.lexicon.texts {
                let std_v = std
                    .texts
                    .get(k)
                    .unwrap_or_else(|| panic!("{id}: unbekannter Schlüssel {k}"));
                // {decker} und {host} sind überall erlaubt
                for ph in [
                    "{ice}", "{n}", "{s}", "{os}", "{max}", "{list}", "{action}", "{sculpt}",
                    "{deck}", "{folder}", "{turn}", "{marks}", "{target}",
                ] {
                    if v.contains(ph) {
                        assert!(std_v.contains(ph), "{id}: {k} nutzt {ph}, Standard nicht");
                    }
                }
            }
            for (k, _) in &t.info.ui {
                assert!(
                    reg.get(DEFAULT_ID).info.ui.contains_key(k),
                    "{id}: unbekannter UI-Schlüssel {k}"
                );
            }
        }
        assert_eq!(reg.get("gibtsnicht").info.id, DEFAULT_ID);
    }

    #[test]
    fn own_theme_from_folder() {
        let dir = std::env::temp_dir().join(format!("cg-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("meinspiel")).unwrap();
        std::fs::write(
            dir.join("meinspiel/theme.json"),
            r#"{"name":"Mein Spiel","shape":"circle","ui":{"open_title":"Los geht's"},"texts":{"pt.jackin":"{decker} startet"},"lists":{"hosts":["Burg"]}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("Kaputt Name")).unwrap();
        let (reg, msgs) = Registry::load(Some(dir.clone()));
        assert!(dir.join("_vorlage/theme.json").exists());
        assert!(dir.join("LIESMICH.txt").exists());
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        let t = reg.get("meinspiel");
        assert_eq!(t.info.name, "Mein Spiel");
        assert_eq!(t.info.shape, "circle");
        assert_eq!(t.info.ui["open_title"], "Los geht's");
        assert_eq!(
            t.info.ui["open_count"], "Personas erfasst",
            "Rest aus dem Standard"
        );
        assert_eq!(
            t.lexicon.t("pt.jackin", &[("decker", "X".into())]),
            "X startet"
        );
        assert_eq!(t.lexicon.list("hosts"), ["Burg"]);
        assert_eq!(t.lexicon.list("nodes").len(), 7, "Knoten aus dem Standard");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
