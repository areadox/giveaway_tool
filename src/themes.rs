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

const DEFAULT_EN: &str = include_str!("../assets/themes/shadowrun/theme.en.json");

/// Eingebautes Design: id, theme.json, theme.en.json, theme.css
macro_rules! builtin {
    ($id:literal) => {
        (
            $id,
            include_str!(concat!("../assets/themes/", $id, "/theme.json")),
            include_str!(concat!("../assets/themes/", $id, "/theme.en.json")),
            include_str!(concat!("../assets/themes/", $id, "/theme.css")),
        )
    };
}
const BUILTIN: &[(&str, &str, &str, &str)] = &[
    builtin!("shadowrun"),
    builtin!("eve"),
    builtin!("wow"),
    builtin!("palia"),
    builtin!("anime"),
];

/// Standardtexte je Spielablauf (zwischen Standard-Design und Design gemischt): (ablauf, deutsch, englisch)
macro_rules! game {
    ($g:literal) => {
        (
            $g,
            include_str!(concat!("../assets/games/", $g, ".json")),
            include_str!(concat!("../assets/games/", $g, ".en.json")),
        )
    };
}
const GAME_JSON: &[(&str, &str, &str)] = &[game!("datasite"), game!("treasure"), game!("riddle"), game!("battle")];

fn game_json(game: &str) -> Option<&'static str> {
    GAME_JSON.iter().find(|(g, ..)| *g == game).map(|(_, j, _)| *j)
}

fn game_json_en(game: &str) -> Option<&'static str> {
    GAME_JSON.iter().find(|(g, ..)| *g == game).map(|(.., j)| *j)
}

const TEMPLATE_CSS: &str = include_str!("../assets/themes/_vorlage.css");
const TEMPLATE_README: &str = include_str!("../assets/themes/LIESMICH.txt");
const TEMPLATE_README_EN: &str = include_str!("../assets/themes/README.txt");

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
    /// Spielablauf: dice | datasite | treasure | riddle | battle
    pub game: String,
    /// Welcher Spielname beim Gewinner gezeigt wird: "eve" oder eine Spiel-Kennung (z. B. "wow")
    pub ign_game: String,
    pub ui: Map<String, Value>,
    pub builtin: bool,
}

#[derive(Debug)]
pub struct Theme {
    pub info: ThemeInfo,
    pub lexicon: Lexicon,
    pub css: String,
}

/// Liest ein Design. Bei Englisch liegt über jeder Ebene (Standard, Ablauf, Design)
/// ihre englische Fassung; was dort fehlt, bleibt deutsch.
fn parse(id: &str, json: &str, json_en: Option<&str>, css: &str, builtin: bool) -> Result<Theme, String> {
    let en = crate::i18n::is_en();
    let base: Value = serde_json::from_str(DEFAULT_JSON).map_err(|e| e.to_string())?;
    let base_en: Option<Value> = if en { serde_json::from_str(DEFAULT_EN).ok() } else { None };
    let mut v: Value = serde_json::from_str(json).map_err(|e| format!("theme.json: {e}", e = e))?;
    let v_en: Option<Value> = match json_en.filter(|_| en) {
        Some(j) => Some(serde_json::from_str(j).map_err(|e| format!("theme.en.json: {e}", e = e))?),
        None => None,
    };
    // Name/Beschreibung aus der englischen Fassung
    if let (Some(Value::Object(o)), Some(Value::Object(e))) = (Some(&mut v), v_en.as_ref()) {
        for k in ["name", "description"] {
            if let Some(x) = e.get(k) {
                o.insert(k.into(), x.clone());
            }
        }
    }
    let game = v.get("game").and_then(Value::as_str).unwrap_or("dice");
    if !crate::games::GAMES.contains(&game) {
        return Err(t!("unbekannter Ablauf \"{game}\" (möglich: {a1})", game = game, a1 = crate::games::GAMES.join(", ")));
    }
    let game_v: Option<Value> = match game_json(game) {
        Some(j) => Some(serde_json::from_str(j).map_err(|e| format!("Ablauf {game}: {e}", game = game, e = e))?),
        None => None,
    };
    let game_en: Option<Value> = match game_json_en(game).filter(|_| en) {
        Some(j) => Some(serde_json::from_str(j).map_err(|e| t!("Ablauf {game} (en): {e}", game = game, e = e))?),
        None => None,
    };
    let layers: Vec<&Value> = std::iter::once(&base)
        .chain(base_en.as_ref())
        .chain(game_v.as_ref())
        .chain(game_en.as_ref())
        .chain([&v])
        .chain(v_en.as_ref())
        .collect();
    let mut lexicon = Lexicon::default();
    let mut ui = Map::new();
    for l in &layers {
        lexicon.merge(l);
        if let Some(Value::Object(u)) = l.get("ui") {
            for (k, val) in u {
                ui.insert(k.clone(), val.clone());
            }
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
            game: game.to_string(),
            ign_game: v
                .get("ign_game")
                .and_then(Value::as_str)
                .map(|g| g.trim().to_lowercase())
                .filter(|g| !g.is_empty())
                .unwrap_or_else(|| "eve".into()),
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
        for (id, json, json_en, css) in BUILTIN {
            match parse(id, json, Some(json_en), css, true) {
                Ok(t) => {
                    map.insert(id.to_string(), Arc::new(t));
                }
                Err(e) => msgs.push(t!("Eingebautes Design {id}: {e}", id = id, e = e)),
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
                        msgs.push(t!("Design-Ordner „{id}“: Name nur aus a–z, 0–9, - und _", id = id));
                        continue;
                    }
                    let Ok(json) = std::fs::read_to_string(path.join("theme.json")) else {
                        msgs.push(t!("Design „{id}“: theme.json fehlt", id = id));
                        continue;
                    };
                    let css = std::fs::read_to_string(path.join("theme.css")).unwrap_or_default();
                    let json_en = std::fs::read_to_string(path.join("theme.en.json")).ok();
                    match parse(&id, &json, json_en.as_deref(), &css, false) {
                        Ok(t) => {
                            map.insert(id, Arc::new(t));
                        }
                        Err(err) => msgs.push(t!("Design „{id}“: {err}", id = id, err = err)),
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
/// Vorlagen anlegen bzw. ergänzen: `_vorlage/` (Würfel-Run), `_ablaeufe/<ablauf>.json`
/// (alle Texte je Spielablauf zum Kopieren) und `LIESMICH.txt`. Vorhandenes bleibt.
fn ensure_template(dir: &Path) {
    let write_new = |p: PathBuf, content: &str| {
        if !p.exists() {
            let _ = std::fs::write(p, content);
        }
    };
    let tpl = dir.join("_vorlage");
    if std::fs::create_dir_all(&tpl).is_ok() {
        let json = DEFAULT_JSON.replacen("\"Shadowrun – Matrix-Run\"", "\"Mein Design\"", 1);
        write_new(tpl.join("theme.json"), &json);
        let json_en = DEFAULT_EN.replacen("\"Shadowrun – Matrix Run\"", "\"My theme\"", 1);
        write_new(tpl.join("theme.en.json"), &json_en);
        write_new(tpl.join("theme.css"), TEMPLATE_CSS);
    }
    let games = dir.join("_ablaeufe");
    if std::fs::create_dir_all(&games).is_ok() {
        for (g, j, j_en) in GAME_JSON {
            write_new(games.join(format!("{g}.json")), j);
            write_new(games.join(format!("{g}.en.json")), j_en);
        }
    }
    // Anleitungen immer aktuell halten
    let _ = std::fs::write(dir.join("LIESMICH.txt"), TEMPLATE_README);
    let _ = std::fs::write(dir.join("README.txt"), TEMPLATE_README_EN);
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
        for (id, ..) in BUILTIN {
            let t = reg.get(id);
            // Vergleich: Standard + Standardtexte des Ablaufs
            let mut std = Lexicon::standard();
            let mut std_ui = reg.get(DEFAULT_ID).info.ui.clone();
            if let Some(j) = game_json(&t.info.game) {
                let g: Value = serde_json::from_str(j).unwrap();
                std.merge(&g);
                if let Some(Value::Object(u)) = g.get("ui") {
                    std_ui.extend(u.clone());
                }
            }
            // Jedes Design hat alle Listen mit sinnvollen Längen
            assert_eq!(t.lexicon.list("nodes").len(), 7, "{id}: 7 Knoten");
            let lists: &[&str] = match t.info.game.as_str() {
                "datasite" => &["sites", "analyzers", "ships"],
                "treasure" => &["places", "tools", "clues", "events", "finds"],
                "riddle" => &["dungeons", "riddles", "traps"],
                "battle" => &[
                    "arenas",
                    "bosses",
                    "boss_lines",
                    "moves",
                    "boss_moves",
                    "ultimates",
                ],
                _ => &["hosts", "sculpts", "decks", "folders"],
            };
            for l in lists {
                assert!(!t.lexicon.list(l).is_empty(), "{id}: Liste {l}");
            }
            for r in t.lexicon.list("riddles") {
                assert!(
                    r.split('|').count() >= 3,
                    "{id}: Rätsel „{r}“ braucht Frage|Antwort|Falsch…"
                );
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
            for k in t.info.ui.keys() {
                assert!(std_ui.contains_key(k), "{id}: unbekannter UI-Schlüssel {k}");
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
        std::fs::create_dir_all(dir.join("schatz")).unwrap();
        std::fs::write(
            dir.join("schatz/theme.json"),
            r#"{"name":"Schatz","game":"treasure","texts":{"tr.heat.0":"brrr"}}"#,
        )
        .unwrap();
        let (reg, msgs) = Registry::load(Some(dir.clone()));
        assert!(dir.join("_vorlage/theme.json").exists());
        assert!(dir.join("LIESMICH.txt").exists());
        assert!(dir.join("_ablaeufe/riddle.json").exists());
        let t2 = reg.get("schatz");
        assert_eq!(t2.info.game, "treasure");
        assert_eq!(t2.lexicon.t("tr.heat.0", &[]), "brrr");
        assert_eq!(
            t2.lexicon.t("tr.heat.1", &[]),
            "kalt",
            "Rest aus dem Ablauf"
        );
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
