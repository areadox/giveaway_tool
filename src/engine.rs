//! Spiellogik: Teilnahme, Countdown, Matrix-Run, Chat, Overlay-Status.
//!
//! Läuft in einem eigenen Thread (Tick alle 50 ms), damit Countdown und Run
//! auch weiterlaufen, wenn das Fenster minimiert ist. Die GUI greift über
//! `Arc<Mutex<Engine>>` zu.

use crate::config::{self, Config};
use crate::db::{clean_login, Db, NewUser};
use crate::esi;
use crate::matrix::{Frame, RunSetup, RunView};
use crate::overlay::OverlayServer;
use crate::prize::{self, Prize, PrizeView};
use crate::sound::{self, Sfx};
use crate::sso::{self, AssetRow};
use crate::themes;
use crate::twitch::{TwitchReader, TwitchSender};
use crate::{update, wow};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const EXCLUDE_USERS: [&str; 3] = ["streamelements", "moobot", "nightbot"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Green,
    Red,
    Cyan,
    Yellow,
    Magenta,
    Dark,
}

#[derive(Debug)]
pub enum Inbound {
    /// Befehl aus dem OBS-Bedien-Dock.
    Dock(DockCmd, Option<mpsc::Sender<DockReply>>),
    Chat {
        user: String,
        msg: String,
    },
    Log(String, Tone),
    EveDone {
        login: String,
        ign: Option<String>,
        error: Option<String>,
    },
    /// Charakter-ID (und Ticker) wurde über ESI nachgeschlagen.
    /// Callback der EVE-Anmeldung (vom Webserver)
    SsoCode {
        code: String,
        state: String,
    },
    SsoLogin(Result<sso::Login, String>),
    Assets {
        result: Result<Vec<AssetRow>, String>,
        /// neuer Refresh-Token, falls EVE ihn beim Auffrischen gewechselt hat
        refresh: Option<String>,
    },
    IdResolved {
        login: String,
        info: Option<esi::EveInfo>,
        error: Option<String>,
    },
    /// Spielname (WoW, Palia, …) gespeichert oder abgelehnt; `reply` geht in den Chat.
    GameDone {
        ok: bool,
        reply: String,
        log: String,
    },
    /// Ergebnis der Update-Prüfung (`manual` = per Knopf ausgelöst)
    UpdateChecked {
        result: Result<Option<update::Release>, String>,
        manual: bool,
    },
    UpdateInstalled(Result<std::path::PathBuf, String>),
}

/// Antwort auf einen Dock-Befehl: optionale Daten oder Fehlermeldung.
pub type DockReply = Result<Option<Value>, String>;

/// Einstellungen aus dem Dock; nur gesetzte Felder werden übernommen.
/// Tokens/Schlüssel werden nur geändert, wenn ein neuer Wert kommt.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SettingsPatch {
    pub channel: Option<String>,
    pub bot_username: Option<String>,
    pub oauth_token: Option<String>,
    pub keyword: Option<String>,
    pub web_port: Option<u32>,
    pub timer_duration: Option<String>,
    pub hack_duration: Option<f64>,
    pub decker_name: Option<String>,
    pub sound: Option<bool>,
    pub auto_enabled: Option<bool>,
    pub auto_interval: Option<String>,
    pub auto_winner_hold: Option<f64>,
    pub open_message: Option<String>,
    pub close_message: Option<String>,
    pub win_message: Option<String>,
    pub eve_enabled: Option<bool>,
    pub eve_command: Option<String>,
    pub hide_eve_names: Option<bool>,
    pub start_in_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub overlay_show_open: Option<bool>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub wow_client_id: Option<String>,
    pub wow_client_secret: Option<String>,
    pub wow_region: Option<String>,
    pub wow_realm: Option<String>,
    pub update_check: Option<bool>,
    /// Spiele als Text („wow !wow WoW-Charakter“ je Zeile)
    pub games_text: Option<String>,
}

impl SettingsPatch {
    /// Wendet den Patch auf eine Kopie der Konfiguration an und prüft Zahlenbereiche.
    pub fn apply_to(&self, base: &Config) -> Result<Config, String> {
        let mut c = base.clone();
        if let Some(l) = &self.language {
            c.switch_language(l);
        }
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(v) = &self.$f { c.$f = v.clone(); } )* };
        }
        set!(
            channel,
            bot_username,
            keyword,
            timer_duration,
            decker_name,
            sound,
            auto_enabled,
            auto_interval,
            open_message,
            close_message,
            win_message,
            eve_enabled,
            eve_command,
            hide_eve_names,
            start_in_tray,
            close_to_tray,
            overlay_show_open,
            theme,
            wow_client_id,
            wow_region,
            wow_realm,
            update_check
        );
        if let Some(k) = self
            .wow_client_secret
            .as_ref()
            .filter(|k| !k.trim().is_empty())
        {
            c.wow_client_secret = k.trim().to_string();
        }
        if let Some(t) = &self.games_text {
            c.games = config::parse_games(t)?;
        }
        if let Some(t) = self.oauth_token.as_ref().filter(|t| !t.trim().is_empty()) {
            c.oauth_token = t.trim().to_string();
        }
        if let Some(p) = self.web_port {
            if !(1..=65535).contains(&p) {
                return Err(t!("Port muss 1–65535 sein").into());
            }
            c.web_port = p as u16;
        }
        if let Some(h) = self.hack_duration {
            if !h.is_finite() || h < 8.0 {
                return Err(t!("Run-Dauer: Zahl ab 8 Sekunden").into());
            }
            c.hack_duration = h.min(600.0);
        }
        if let Some(h) = self.auto_winner_hold {
            if !h.is_finite() || h < 3.0 {
                return Err(t!("Gewinner-Anzeige: mindestens 3 Sekunden").into());
            }
            c.auto_winner_hold = h.min(3600.0);
        }
        if c.keyword.trim().is_empty() {
            c.keyword = "!join".into();
        }
        Ok(c)
    }
}

/// Befehle des OBS-Bedien-Docks (`POST /api/dock`).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum DockCmd {
    /// START bzw. STOP & AUSLOSEN
    Toggle {
        keyword: String,
        duration: String,
        #[serde(default)]
        winners: Option<u32>,
    },
    Breach {
        keyword: String,
        hack: String,
        #[serde(default)]
        winners: Option<u32>,
    },
    Reset,
    /// Auto-Giveaway ein-/ausschalten (wird in config.json gespeichert)
    Auto {
        enabled: bool,
    },
    /// Einstellungen speichern und übernehmen
    Settings {
        settings: SettingsPatch,
    },
    Winners {
        n: u32,
    },
    Prizes {
        prizes: Vec<Prize>,
        text: String,
    },
    /// Liste der geladenen EVE-Assets abrufen
    Assets,
    LoadAssets,
    SsoConnect,
    SsoLogout,
    SsoClient {
        id: String,
    },
    SsoStructures {
        enabled: bool,
    },
    /// Design wechseln (wird gespeichert)
    Theme {
        id: String,
    },
    /// Eigene Designs aus dem Ordner `themes/` neu einlesen
    ReloadThemes,
    /// Auf GitHub nach einer neuen Version suchen
    UpdateCheck,
    /// Neue Version herunterladen, einsetzen und neu starten
    UpdateInstall,
    /// Spielname löschen
    GameNameDelete {
        login: String,
        game: String,
    },
    /// Chat-Dock: Nachricht über den Bot senden (wird gespeichert)
    ChatSend {
        #[serde(default)]
        text: String,
        /// gespeicherte Nachricht erneut senden
        #[serde(default)]
        id: Option<i64>,
    },
    /// Chat-Dock: gespeicherte Nachrichten abrufen
    ChatSaved,
    /// Chat-Dock: anheften bzw. automatische Wiederholung (Minuten, 0 = aus)
    ChatUpdate {
        id: i64,
        #[serde(default)]
        pinned: Option<bool>,
        #[serde(default)]
        repeat: Option<u32>,
    },
    ChatDelete {
        id: i64,
    },
    /// Chat-Dock: Mindestzahl an Chatnachrichten zwischen zwei Wiederholungen
    ChatMinLines {
        n: u32,
    },
}

#[derive(Serialize)]
struct DockLine<'a> {
    stamp: &'a str,
    text: &'a str,
    tone: &'static str,
}

fn tone_name(t: Tone) -> &'static str {
    match t {
        Tone::Green => "green",
        Tone::Red => "red",
        Tone::Cyan => "cyan",
        Tone::Yellow => "yellow",
        Tone::Magenta => "magenta",
        Tone::Dark => "dark",
    }
}

/// EVE-Daten eines Spielers für Overlay und Programmfenster.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EveCard {
    pub name: String,
    pub id: Option<i64>,
    pub corp: Option<String>,
    pub alliance: Option<String>,
    pub portrait: Option<String>,
}

pub fn portrait_url(id: i64, size: u32) -> String {
    format!("https://images.evetech.net/characters/{id}/portrait?size={size}")
}

impl EveCard {
    fn from_user(u: &crate::db::User) -> Option<Self> {
        let name = u
            .eve_character_name
            .clone()
            .filter(|n| !n.trim().is_empty())?;
        Some(Self {
            name,
            id: u.eve_character_id,
            corp: u.corp_ticker.clone(),
            alliance: u.alliance_ticker.clone(),
            portrait: u.eve_character_id.map(|id| portrait_url(id, 256)),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub stamp: String,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Open,
    Running,
    Won,
}

struct ActiveRun {
    frames: Vec<Frame>,
    started: Instant,
    next: usize,
    log_seen: usize,
    winners: Vec<String>,
}

/// Spielname aus einem anderen Spiel als EVE (gleiche Felder wie `EveCard`,
/// damit Overlay und Dock beide gleich anzeigen können).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IgnCard {
    pub game: String,
    pub label: String,
    pub name: String,
    /// Zusatzzeile, z. B. „Stufe 80 Magier · <Gilde>“
    pub sub: Option<String>,
    pub portrait: Option<String>,
}

impl IgnCard {
    fn from_db(g: &crate::db::GameName, label: &str) -> Self {
        let info = g.info.as_ref();
        let s = |k: &str| {
            info.and_then(|i| i.get(k))
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|x| !x.is_empty())
        };
        Self {
            game: g.game.clone(),
            label: label.to_string(),
            name: g.name.clone(),
            sub: s("summary"),
            portrait: s("image"),
        }
    }
}

/// Ein Gewinner mit Anzeigenamen, EVE-Daten und zugeordnetem Preis.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Winner {
    pub login: String,
    /// „login // EVE-Name“ für Programmfenster, Dock und Verlauf
    #[serde(skip)]
    pub display: String,
    pub eve: Option<EveCard>,
    /// Spielname passend zum Design (wenn nicht EVE)
    pub ign: Option<IgnCard>,
    pub prize: Option<PrizeView>,
}

struct GiveawayTimer {
    deadline: Instant,
    warned: HashSet<u64>,
}

#[derive(Serialize)]
struct OverlayState<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    visible: bool,
    mode: &'static str,
    keyword: &'a str,
    count: usize,
    recent: Vec<&'a str>,
    timer_active: bool,
    timer_remaining: f64,
    timer_total: u64,
    run: Option<&'a RunView>,
    /// erster Gewinner (für einfache Anzeigen)
    winner: &'a str,
    winners: Vec<Winner>,
    winners_count: u32,
    prize: String,
    status: &'a str,
    auto: AutoState,
    /// Design-ID; `theme_v` ändert sich, wenn Designs neu geladen wurden.
    theme: String,
    theme_v: u64,
}

/// Stand des Auto-Timers für das Overlay und die Timer-Anzeige.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AutoState {
    pub enabled: bool,
    /// "wartet" (Countdown bis zum nächsten Giveaway) | "giveaway" | "aus"
    pub state: &'static str,
    pub remaining: f64,
    pub total: u64,
}

pub struct Engine {
    pub cfg: Config,
    pub db: Db,
    pub phase: Phase,
    pub participants: Vec<String>,
    participant_set: HashSet<String>,
    pub active_keyword: String,
    pub winners: Vec<Winner>,
    pub status: String,
    pub status_tone: Tone,
    pub countdown_text: String,
    pub countdown_tone: Tone,
    pub log: VecDeque<LogEntry>,
    pub overlay_url: String,
    pub db_version: u64,
    pub run_view: Option<RunView>,
    timer: Option<GiveawayTimer>,
    timer_total: u64,
    run: Option<ActiveRun>,
    win_sound_at: Option<Instant>,
    auto_deadline: Option<Instant>,
    auto_total: u64,
    won_at: Option<Instant>,
    auto_last_sec: u64,
    sso_pending: Option<sso::Pending>,
    /// Geladene EVE-Assets für die Preisauswahl
    pub assets: Vec<AssetRow>,
    pub assets_status: String,
    pub assets_loading: bool,
    pub assets_version: u64,
    tx: Sender<Inbound>,
    rx: Receiver<Inbound>,
    reader: TwitchReader,
    sender: TwitchSender,
    overlay: Option<OverlayServer>,
    igns: Option<regex::Regex>,
    eve_cache: HashMap<String, String>,
    dirty: bool,
    last_publish: Instant,
    repaint: Option<Box<dyn Fn() + Send>>,
    /// Neuere Version auf GitHub (falls gefunden)
    pub update: Option<update::Release>,
    pub update_status: String,
    pub update_busy: bool,
    /// Update ist eingespielt: Programm neu starten (GUI bzw. Hauptschleife)
    pub restart_to: Option<std::path::PathBuf>,
    /// Chat-Dock: gespeicherte Nachrichten (Kopie der Datenbank)
    chat_saved: Vec<crate::db::SavedMsg>,
    /// Anzahl Chatnachrichten anderer (nicht vom Bot) seit Programmstart
    chat_seen: u64,
    /// je Nachricht mit Wiederholung: zuletzt gesendet und Stand von `chat_seen`
    chat_repeat: HashMap<i64, (Instant, u64)>,
    chat_check: Instant,
}

fn stamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

impl Engine {
    pub fn new(cfg: Config, db: Db) -> Self {
        let (tx, rx) = mpsc::channel();
        let reader = TwitchReader::start(cfg.channel_clean(), cfg.channel_configured(), tx.clone());
        let sender = TwitchSender::start(
            cfg.channel_clean(),
            cfg.bot_username.clone(),
            cfg.oauth_token.clone(),
            cfg.channel_configured(),
            tx.clone(),
        );
        let keyword = cfg.keyword_or_default();
        let mut e = Self {
            cfg,
            db,
            phase: Phase::Ready,
            participants: vec![],
            participant_set: HashSet::new(),
            active_keyword: keyword,
            winners: Vec::new(),
            status: t!("BEREIT"),
            status_tone: Tone::Cyan,
            countdown_text: "—".into(),
            countdown_tone: Tone::Yellow,
            log: VecDeque::new(),
            overlay_url: String::new(),
            db_version: 0,
            run_view: None,
            timer: None,
            timer_total: 0,
            run: None,
            win_sound_at: None,
            auto_deadline: None,
            auto_total: 0,
            won_at: None,
            auto_last_sec: u64::MAX,
            sso_pending: None,
            assets: Vec::new(),
            assets_status: t!("Noch nicht geladen").into(),
            assets_loading: false,
            assets_version: 0,
            tx,
            rx,
            reader,
            sender,
            overlay: None,
            igns: None,
            eve_cache: HashMap::new(),
            dirty: true,
            last_publish: Instant::now(),
            repaint: None,
            update: None,
            update_status: String::new(),
            update_busy: false,
            restart_to: None,
            chat_saved: Vec::new(),
            chat_seen: 0,
            chat_repeat: HashMap::new(),
            chat_check: Instant::now(),
        };
        e.chat_saved = e.db.chat_saved();
        e.refresh_igns();
        e.log(
            t!("System initialisiert; Twitch-Reader wird gestartet."),
            Tone::Green,
        );
        e.log(
            t!("Keyword: {a1} | Ausschlüsse: {a2}", a1 = e.active_keyword, a2 = EXCLUDE_USERS.join(", ")),
            Tone::Dark,
        );
        e.start_overlay();
        if e.cfg.update_check && !cfg!(test) {
            e.check_update(false);
        }
        e
    }

    // ------------------------------------------------------------ Update

    pub fn check_update(&mut self, manual: bool) {
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.update_status = t!("Suche nach Updates …").into();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Inbound::UpdateChecked {
                result: update::check(),
                manual,
            });
        });
        self.dirty = true;
    }

    pub fn install_update(&mut self) -> Result<(), String> {
        let Some(rel) = self.update.clone() else {
            return Err(t!("Kein Update verfügbar").into());
        };
        if matches!(self.phase, Phase::Running | Phase::Open) {
            return Err(t!("Bitte erst das laufende Giveaway abschließen").into());
        }
        if self.update_busy {
            return Err(t!("Update läuft bereits").into());
        }
        self.update_busy = true;
        self.update_status = t!("Lade v{a1} herunter …", a1 = rel.version);
        self.log(
            t!("Update auf v{a1} wird geladen …", a1 = rel.version),
            Tone::Cyan,
        );
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Inbound::UpdateInstalled(update::install(&rel)));
        });
        self.dirty = true;
        Ok(())
    }

    // ------------------------------------------------------------ Spielnamen

    /// Kennung des Spiels, dessen Namen das aktive Design zeigt ("eve" = EVE-Daten).
    pub fn ign_game(&self) -> String {
        themes::registry()
            .get(&self.cfg.theme)
            .info
            .ign_game
            .clone()
    }

    fn game_label(&self, game: &str) -> String {
        self.cfg
            .games
            .iter()
            .find(|g| g.game == game)
            .map(|g| g.label.clone())
            .unwrap_or_else(|| format!("{game}-Name"))
    }

    /// Spielname des Zuschauers passend zum Design (nur für Nicht-EVE-Designs).
    pub fn ign_card(&self, login: &str) -> Option<IgnCard> {
        let game = self.ign_game();
        if game == "eve" {
            return None;
        }
        self.db
            .get_game_name(login, &game)
            .map(|g| IgnCard::from_db(&g, &self.game_label(&game)))
    }

    fn handle_game_link(&mut self, login: String, game: config::GameCmd, arg: String) {
        let arg = esi::clean_name(&arg);
        if arg.is_empty() {
            let msg = match self.db.get_game_name(&login, &game.game) {
                Some(g) => t!("@{login} Dein {a1}: {a2} – ändern mit {a3} Name", login = login, a1 = game.label, a2 = g.name, a3 = game.command),
                None if game.game == "wow" => {
                    t!("@{login} Bitte so eintragen: {a1} Name-Realm", login = login, a1 = game.command)
                }
                None => t!("@{login} Bitte so eintragen: {a1} Name", login = login, a1 = game.command),
            };
            self.sender.send(msg);
            return;
        }
        let db = self.db.clone();
        let tx = self.tx.clone();
        if game.game == "wow" {
            let Some((name, realm)) = wow::parse_command(&arg, &self.cfg.wow_realm) else {
                self.sender.send(t!("@{login} Bitte so eintragen: {a1} Name-Realm", login = login, a1 = game.command));
                return;
            };
            let creds = self.cfg.wow_ready().then(|| {
                (
                    self.cfg.wow_client_id.clone(),
                    self.cfg.wow_client_secret.clone(),
                    self.cfg.wow_region.clone(),
                )
            });
            std::thread::spawn(move || {
                let res = match creds {
                    // Ohne Blizzard-Zugang: Name ungeprüft speichern
                    None => db
                        .set_game_name(&login, "wow", &format!("{name}-{realm}"), Some(&serde_json::json!({"realm": realm})))
                        .map(|_| (format!("{name}-{realm}"), String::new()))
                        .map_err(|e| e.to_string()),
                    Some((id, secret, region)) => wow::lookup(&id, &secret, &region, &name, &realm).and_then(|c| {
                        let sum = c.summary();
                        let info = serde_json::json!({"realm": c.realm, "level": c.level, "class": c.class, "race": c.race,
                            "guild": c.guild, "faction": c.faction, "image": c.image, "region": c.region, "summary": sum});
                        let shown = format!("{}-{}", c.name, c.realm);
                        db.set_game_name(&login, "wow", &shown, Some(&info)).map(|_| (shown, sum)).map_err(|e| e.to_string())
                    }),
                };
                let msg = match res {
                    Ok((shown, sum)) => Inbound::GameDone {
                        reply: if sum.is_empty() {
                            format!("@{login} {a1} gespeichert: {shown}", login = login, a1 = game.label, shown = shown)
                        } else {
                            format!("@{login} {a1} gespeichert: {shown} ({sum})", login = login, a1 = game.label, shown = shown, sum = sum)
                        },
                        log: format!("{a1}: @{login} // {shown}", a1 = game.label, login = login, shown = shown),
                        ok: true,
                    },
                    Err(e) => Inbound::GameDone {
                        reply: format!("@{login} {e}"),
                        log: t!("{a1} für @{login} abgelehnt: {e}", a1 = game.label, login = login, e = e),
                        ok: false,
                    },
                };
                let _ = tx.send(msg);
            });
            return;
        }
        let name: String = arg.chars().filter(|c| !c.is_control()).take(40).collect();
        let msg = match db.set_game_name(&login, &game.game, &name, None) {
            Ok(()) => Inbound::GameDone {
                reply: format!("@{login} {a1} gespeichert: {name}", login = login, a1 = game.label, name = name),
                log: format!("{a1}: @{login} // {name}", a1 = game.label, login = login, name = name),
                ok: true,
            },
            Err(e) => Inbound::GameDone {
                reply: format!("@{login} {e}"),
                log: t!("{a1} für @{login} abgelehnt: {e}", a1 = game.label, login = login, e = e),
                ok: false,
            },
        };
        let _ = tx.send(msg);
    }

    pub fn set_repaint(&mut self, f: Box<dyn Fn() + Send>) {
        self.repaint = Some(f);
    }

    fn start_overlay(&mut self) {
        let initial = self.overlay_json();
        let srv = OverlayServer::start(
            self.cfg.web_port,
            initial,
            self.dock_json(),
            self.cfg.decker(),
            self.tx.clone(),
        );
        match (&srv.port, &srv.error) {
            (Some(p), _) => {
                self.overlay_url = format!("http://localhost:{p}");
                self.log(
                    t!("OVERLAY: {a1} | DEMO: {a1}/test", a1 = self.overlay_url),
                    Tone::Cyan,
                );
                if *p != self.cfg.web_port {
                    self.log(
                        t!("Port {a1} belegt – Overlay läuft auf {p}.", a1 = self.cfg.web_port, p = p),
                        Tone::Yellow,
                    );
                }
            }
            (None, Some(e)) => {
                self.overlay_url.clear();
                self.log(format!("Overlay-Fehler: {e}", e = e), Tone::Red);
            }
            _ => {}
        }
        self.overlay = Some(srv);
    }

    pub fn log(&mut self, text: impl Into<String>, tone: Tone) {
        self.log.push_back(LogEntry {
            stamp: stamp(),
            text: text.into(),
            tone,
        });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
        self.dirty = true;
        self.request_repaint();
    }

    fn request_repaint(&self) {
        if let Some(f) = &self.repaint {
            f();
        }
    }

    fn sfx(&self, s: Sfx) {
        sound::play(s, self.cfg.sound);
    }

    fn set_status(&mut self, s: impl AsRef<str>, tone: Tone) {
        self.status = s.as_ref().to_string();
        self.status_tone = tone;
        self.dirty = true;
    }

    pub fn display_name(&self, login: &str) -> String {
        if let Some(c) = self.ign_card(login) {
            return format!("{login} // {}", c.name);
        }
        if self.ign_game() != "eve" {
            return login.to_string();
        }
        match self.db.get_user(login).and_then(|u| u.eve_character_name) {
            Some(n) if !n.is_empty() => format!("{login} // {n}"),
            _ => login.to_string(),
        }
    }

    pub fn refresh_igns(&mut self) {
        let mut set: HashSet<String> = self
            .db
            .list_users()
            .into_iter()
            .filter_map(|u| u.eve_character_name)
            .collect();
        set.extend(self.eve_cache.values().cloned());
        set.extend(self.db.list_game_names().into_iter().map(|g| g.name));
        let mut v: Vec<String> = set
            .into_iter()
            .filter(|s| s.trim().chars().count() >= 3)
            .collect();
        // Längste zuerst, damit Teilnamen nicht vorher ersetzt werden
        v.sort_by_key(|s| std::cmp::Reverse(s.len()));
        self.igns = build_scrubber(&v);
        self.db_version += 1;
        self.dirty = true;
    }

    // ------------------------------------------------------------ Aktionen

    /// START bzw. STOP & AUSLOSEN
    pub fn toggle_entries(&mut self, keyword: &str, duration_text: &str) {
        let kw = keyword.trim();
        self.active_keyword = if kw.is_empty() {
            "!join".into()
        } else {
            kw.into()
        };
        self.sfx(Sfx::Click);
        if self.phase == Phase::Open {
            self.stop_entries();
            return;
        }
        if self.phase == Phase::Running {
            self.log(t!("BREACH läuft bereits."), Tone::Yellow);
            return;
        }
        let secs = match config::parse_duration(duration_text) {
            Ok(s) => s,
            Err(e) => {
                self.sfx(Sfx::Error);
                self.log(t!("Ungültige Giveaway-Dauer: {e}", e = e), Tone::Red);
                return;
            }
        };
        self.cancel_timer(true);
        self.participants.clear();
        self.participant_set.clear();
        self.winners.clear();
        self.run_view = None;
        self.phase = Phase::Open;
        self.set_status(t!("TEILNAHME OFFEN"), Tone::Green);
        self.log(
            t!("EINGÄNGE OFFEN — Chat: {a1}", a1 = self.active_keyword),
            Tone::Green,
        );
        if !self.prize_summary().is_empty() {
            let p = self.prize_summary();
            self.log(t!("Zu gewinnen: {p}", p = p), Tone::Cyan);
        }
        let prize = self.prize_summary();
        let mut open = self
            .cfg
            .open_message
            .replace("{keyword}", &self.active_keyword)
            .replace("{prize}", &prize);
        if !prize.is_empty() && !self.cfg.open_message.contains("{prize}") {
            open.push_str(&t!(" Zu gewinnen: {prize}.", prize = prize));
        }
        let n = self.cfg.winners.max(1);
        if n > 1 {
            open.push_str(&t!(" {n} Gewinner.", n = n));
        }
        if secs > 0 {
            self.sender.send(format!("{open} Dauer: {a1}.", open = open, a1 = config::format_clock(secs as f64)));
        } else {
            self.sender.send(t!("{open} Manuelles Ende.", open = open));
        }
        self.start_timer(secs);
    }

    fn stop_entries(&mut self) {
        self.cancel_timer(true);
        if self.participants.is_empty() {
            self.phase = Phase::Ready;
            self.set_status("GESTOPPT", Tone::Cyan);
            self.sender.send(self.cfg.close_message.clone());
            self.log(
                t!("Teilnahme gestoppt; keine Teilnehmer vorhanden."),
                Tone::Yellow,
            );
            return;
        }
        self.sender.send(self.cfg.close_message.clone());
        self.log(
            t!("Teilnahme gestoppt; die Auslosung startet."),
            Tone::Yellow,
        );
        self.start_breach_inner();
    }

    /// BREACH-Knopf: Run sofort starten (schließt die Teilnahme).
    pub fn start_breach(&mut self, keyword: &str, hack_text: &str) {
        let kw = keyword.trim();
        if !kw.is_empty() {
            self.active_keyword = kw.into();
        }
        match hack_text.trim().replace(',', ".").parse::<f64>() {
            Ok(v) if v.is_finite() && v >= 8.0 => self.cfg.hack_duration = v.min(600.0),
            _ => self.log(
                t!("Ungültige Run-Dauer (mind. 8 s); bisheriger Wert bleibt."),
                Tone::Yellow,
            ),
        }
        self.sfx(Sfx::Click);
        if self.phase == Phase::Running {
            self.log(t!("BREACH läuft bereits."), Tone::Yellow);
            return;
        }
        if self.participants.is_empty() {
            self.sfx(Sfx::Error);
            self.log(
                t!("FEHLER: Keine Personas im Pool. Erst Teilnehmer erfassen."),
                Tone::Red,
            );
            return;
        }
        if self.phase == Phase::Open {
            self.sender.send(self.cfg.close_message.clone());
        }
        self.cancel_timer(true);
        self.start_breach_inner();
    }

    fn start_breach_inner(&mut self) {
        let mut rng = rand::thread_rng();
        let wanted = self.cfg.winners.max(1) as usize;
        if wanted > self.participants.len() {
            self.log(
                t!("{wanted} Gewinner eingestellt, aber nur {a1} Teilnehmer – alle gewinnen.", wanted = wanted, a1 = self.participants.len()),
                Tone::Yellow,
            );
        }
        // Gleichverteilt ohne Zurücklegen; Reihenfolge = Preis 1, 2, …
        let mut winners: Vec<String> = self
            .participants
            .choose_multiple(&mut rng, wanted)
            .cloned()
            .collect();
        winners.shuffle(&mut rng);
        let duration = self.cfg.hack_duration.clamp(8.0, 600.0);
        let theme = themes::registry().get(&self.cfg.theme);
        let frames = crate::games::generate(
            &theme.info.game,
            &mut rng,
            RunSetup {
                participants: &self.participants,
                winners: &winners,
                decker: &self.cfg.decker(),
                duration,
                lexicon: &theme.lexicon,
            },
        );
        self.phase = Phase::Running;
        self.winners.clear();
        self.set_status(t!("AUSLOSUNG LÄUFT"), Tone::Red);
        self.log(
            t!("BREACH INITIALISIERT // {a1} Personas im Pool; Run-Dauer {duration}s", a1 = self.participants.len(), duration = format!("{:.1}", duration)),
            Tone::Red,
        );
        self.run = Some(ActiveRun {
            frames,
            started: Instant::now(),
            next: 0,
            log_seen: 0,
            winners,
        });
        self.dirty = true;
    }

    fn finish_breach(&mut self, logins: Vec<String>) {
        let mut winners = Vec::with_capacity(logins.len());
        for (i, login) in logins.iter().enumerate() {
            let eve = self
                .db
                .get_user(login)
                .as_ref()
                .and_then(EveCard::from_user);
            if let Some(card) = &eve {
                if card.id.is_none() {
                    self.resolve_id(login.clone(), card.name.clone());
                }
            }
            let ign = self.ign_card(login);
            // Bei Designs mit eigenem Spiel (WoW, Palia, …) keine EVE-Daten zeigen
            let eve = if self.ign_game() == "eve" { eve } else { None };
            winners.push(Winner {
                login: login.clone(),
                display: self.display_name(login),
                eve,
                ign,
                prize: prize::for_winner(&self.cfg.prizes, &self.cfg.prize_text, i),
            });
        }
        self.winners = winners;
        self.phase = Phase::Won;
        self.set_status(t!("BREACH ERFOLGREICH"), Tone::Green);
        let count = self.participants.len();
        let many = self.winners.len() > 1;
        for (i, w) in self.winners.clone().iter().enumerate() {
            let prize = w.prize.as_ref().map(|p| p.label.clone());
            let head = if many {
                t!("Gewinner {n}: ", n = i + 1)
            } else {
                t!("BREACH ERFOLGREICH! Gewinner: ").into()
            };
            let tail = prize
                .as_ref()
                .map(|p| t!(" – Preis: {p}", p = p))
                .unwrap_or_default();
            self.log(format!("{head}@{}{tail}", w.display), Tone::Yellow);
            if let Err(e) = self.db.add_history(&w.display, count, prize.as_deref()) {
                self.log(
                    t!("Verlauf konnte nicht gespeichert werden: {e}", e = e),
                    Tone::Red,
                );
            }
        }
        self.win_sound_at = Some(Instant::now() + Duration::from_millis(380));
        self.won_at = Some(Instant::now());
        self.send_win_messages();
        self.dirty = true;
    }

    /// Chat: je Gewinner eine Nachricht; bei sehr vielen eine Sammelnachricht.
    fn send_win_messages(&mut self) {
        let tpl = self.cfg.win_message.clone();
        let line = |w: &Winner| {
            let prize = w
                .prize
                .as_ref()
                .map(|p| p.label.clone())
                .unwrap_or_default();
            let mut m = tpl.replace("{winner}", &w.login).replace("{prize}", &prize);
            if !prize.is_empty() && !tpl.contains("{prize}") {
                m.push_str(&t!(" Preis: {prize}", prize = prize));
            }
            m
        };
        if self.winners.len() <= 10 {
            for w in self.winners.clone() {
                self.sender.send(line(&w));
            }
        } else {
            let names: Vec<String> = self
                .winners
                .iter()
                .map(|w| format!("@{}", w.login))
                .collect();
            for chunk in names.chunks(15) {
                self.sender
                    .send(t!("🎉 Gewinner: {list}", list = chunk.join(", ")));
            }
        }
    }

    /// Kurzfassung, was es zu gewinnen gibt.
    pub fn prize_summary(&self) -> String {
        prize::summary(&self.cfg.prizes, &self.cfg.prize_text)
    }

    /// Gewinnerzahl ändern (Programmfenster/Dock) und speichern.
    pub fn set_winners(&mut self, n: u32) {
        let n = n.clamp(1, 100);
        if n == self.cfg.winners {
            return;
        }
        self.cfg.winners = n;
        if let Err(e) = config::save(&config::config_path(), &self.cfg) {
            self.log(t!("Gewinnerzahl nicht gespeichert: {e}", e = e), Tone::Red);
        }
        self.dirty = true;
    }

    /// Preisliste/Beschreibung übernehmen und speichern.
    pub fn set_prizes(&mut self, prizes: Vec<Prize>, text: String) {
        self.cfg.prizes = prizes;
        self.cfg.prize_text = text;
        if let Err(e) = config::save(&config::config_path(), &self.cfg) {
            self.log(t!("Preise nicht gespeichert: {e}", e = e), Tone::Red);
        }
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.cancel_timer(true);
        self.sfx(Sfx::Click);
        self.phase = Phase::Ready;
        self.participants.clear();
        self.participant_set.clear();
        self.winners.clear();
        self.run = None;
        self.run_view = None;
        self.win_sound_at = None;
        self.won_at = None;
        self.auto_deadline = None;
        self.log.clear();
        self.set_status(t!("BEREIT / TEILNAHME GESCHLOSSEN"), Tone::Cyan);
        self.log(
            t!("System zurückgesetzt. Pool geleert; Entries geschlossen."),
            Tone::Yellow,
        );
    }

    // ------------------------------------------------------------ Timer

    fn start_timer(&mut self, secs: u64) {
        self.timer_total = secs;
        if secs == 0 {
            self.timer = None;
            self.countdown_text = "MANUELL".into();
            self.countdown_tone = Tone::Cyan;
        } else {
            self.timer = Some(GiveawayTimer {
                deadline: Instant::now() + Duration::from_secs(secs),
                warned: HashSet::new(),
            });
        }
        self.dirty = true;
    }

    fn cancel_timer(&mut self, reset: bool) {
        self.timer = None;
        if reset {
            self.timer_total = 0;
            self.countdown_text = "—".into();
            self.countdown_tone = Tone::Yellow;
        }
        self.dirty = true;
    }

    fn timer_remaining(&self) -> f64 {
        self.timer
            .as_ref()
            .map(|t| {
                t.deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64()
            })
            .unwrap_or(0.0)
    }

    fn tick_timer(&mut self) {
        if self.phase != Phase::Open {
            return;
        }
        let Some(t) = &self.timer else { return };
        let remaining = t
            .deadline
            .saturating_duration_since(Instant::now())
            .as_secs_f64();
        let shown = remaining.ceil() as u64;
        let text = config::format_clock(remaining);
        if text != self.countdown_text {
            self.countdown_text = text;
            self.countdown_tone = if shown <= 10 { Tone::Red } else { Tone::Yellow };
            self.dirty = true;
            self.request_repaint();
        }
        let mut warn = None;
        if let Some(t) = &mut self.timer {
            for th in [60u64, 10] {
                if self.timer_total > th && shown <= th && t.warned.insert(th) {
                    warn = Some(th);
                }
            }
        }
        if let Some(th) = warn {
            self.sender.send(t!("⏱ Noch {th} Sekunden! Teilnahme mit {a1}.", th = th, a1 = self.active_keyword));
        }
        if remaining <= 0.0 {
            self.cancel_timer(true);
            if self.participants.is_empty() {
                self.sender.send(t!("⏱ Giveaway beendet: keine Teilnehmer. Schreibe beim nächsten Mal {a1}.", a1 = self.active_keyword));
                self.log(
                    t!("Countdown abgelaufen; keine Teilnehmer. Giveaway sauber beendet."),
                    Tone::Yellow,
                );
                self.phase = Phase::Ready;
                self.set_status("GESTOPPT", Tone::Cyan);
                return;
            }
            self.sender.send(self.cfg.close_message.clone());
            self.log(
                t!("Countdown abgelaufen; die Auslosung startet automatisch."),
                Tone::Red,
            );
            self.start_breach_inner();
        }
    }

    // ------------------------------------------------------------ Run

    fn tick_run(&mut self) {
        let Some(run) = &mut self.run else { return };
        let elapsed = run.started.elapsed().as_secs_f64();
        let mut changed = false;
        let mut sfx = None;
        let mut new_logs = vec![];
        let mut finished = None;
        while run.next < run.frames.len() && run.frames[run.next].t <= elapsed {
            let f = &run.frames[run.next];
            if f.sfx.is_some() {
                sfx = f.sfx;
            }
            // Neue Logzeilen des Runs auch ins Programm-Terminal spiegeln
            let total_logs = f.view.log.len();
            for l in &f.view.log[run.log_seen.min(total_logs)..] {
                let tone = match l.tone.as_str() {
                    "good" => Tone::Green,
                    "bad" => Tone::Red,
                    "warn" => Tone::Yellow,
                    "edge" => Tone::Magenta,
                    "dim" => Tone::Dark,
                    _ => Tone::Cyan,
                };
                new_logs.push((l.text.clone(), tone));
            }
            run.log_seen = total_logs;
            self.run_view = Some(f.view.clone());
            if f.view.done {
                finished = Some(run.winners.clone());
            }
            run.next += 1;
            changed = true;
        }
        for (t, tone) in new_logs {
            self.log(t, tone);
        }
        if let Some(s) = sfx {
            self.sfx(s);
        }
        if let Some(w) = finished {
            self.run = None;
            self.finish_breach(w);
        }
        if changed {
            self.dirty = true;
            self.request_repaint();
        }
    }

    // ------------------------------------------------------------ Chat

    fn add_participant(&mut self, name: &str) {
        let name = clean_login(name);
        if name.is_empty()
            || EXCLUDE_USERS.contains(&name.as_str())
            || self.participant_set.contains(&name)
            || self.phase != Phase::Open
        {
            return;
        }
        self.participant_set.insert(name.clone());
        self.participants.push(name.clone());
        let display = self.display_name(&name);
        self.sfx(Sfx::Join);
        self.log(
            t!("PERSONA ERFASST: @{display} [{a1}]", display = display, a1 = self.participants.len()),
            Tone::Cyan,
        );
        if display != name {
            self.sender.send(t!("@{name} ist dabei.", name = name));
        }
        self.dirty = true;
    }

    fn handle_chat(&mut self, user: String, msg: String) {
        if !user.eq_ignore_ascii_case(self.cfg.bot_username.trim()) {
            self.chat_seen += 1;
        }
        let msg = msg.trim().to_string();
        let lower = msg.to_lowercase();
        if lower == self.active_keyword.to_lowercase() {
            if self.phase != Phase::Open {
                return;
            }
            self.add_participant(&user);
            return;
        }
        if let Some(g) = self.cfg.games.iter().find(|g| {
            let c = g.command.to_lowercase();
            lower == c || lower.starts_with(&format!("{c} "))
        }) {
            let g = g.clone();
            let arg: String = msg.chars().skip(g.command.chars().count()).collect();
            self.handle_game_link(user, g, arg.trim().to_string());
            return;
        }
        let cmd = self.cfg.eve_command_or_default().to_lowercase();
        if lower == cmd {
            self.handle_eve_link(user, String::new());
        } else if lower.starts_with(&format!("{cmd} ")) {
            let name: String = msg.chars().skip(cmd.chars().count()).collect();
            self.handle_eve_link(user, name.trim().to_string());
        }
    }

    /// Schlägt Charakter-ID, Corp und Allianz zu einem IGN über ESI nach und
    /// speichert sie (Quelle bleibt erhalten).
    pub fn resolve_id(&mut self, login: String, name: String) {
        let tx = self.tx.clone();
        let db = self.db.clone();
        std::thread::spawn(move || {
            let msg = match esi::validate_character(&name) {
                Ok(info) => {
                    let old = db.get_user(&login).unwrap_or_default();
                    match db.upsert_user(&NewUser {
                        twitch_login: login.clone(),
                        eve_character_name: Some(info.name.clone()),
                        eve_character_id: Some(info.id),
                        twitch_id: old.twitch_id,
                        corp_ticker: info.corp.clone(),
                        alliance_ticker: info.alliance.clone(),
                        source: old.source.or(Some("esi".into())),
                    }) {
                        Ok(()) => Inbound::IdResolved {
                            login,
                            info: Some(info),
                            error: None,
                        },
                        Err(e) => Inbound::IdResolved {
                            login,
                            info: None,
                            error: Some(e.to_string()),
                        },
                    }
                }
                Err(e) => Inbound::IdResolved {
                    login,
                    info: None,
                    error: Some(e),
                },
            };
            let _ = tx.send(msg);
        });
    }

    /// Ergänzt für alle Zuordnungen ohne Charakter-ID die ID über ESI.
    pub fn resolve_missing_ids(&mut self) -> usize {
        let todo: Vec<(String, String)> = self
            .db
            .list_users()
            .into_iter()
            .filter(|u| u.eve_character_id.is_none())
            .filter_map(|u| {
                u.eve_character_name
                    .filter(|n| !n.trim().is_empty())
                    .map(|n| (u.twitch_login, n))
            })
            .collect();
        let n = todo.len();
        for (login, name) in todo {
            self.resolve_id(login, name);
        }
        if n > 0 {
            self.log(
                t!("Suche Charakter-IDs für {n} Zuordnung(en) …", n = n),
                Tone::Cyan,
            );
        }
        n
    }

    fn handle_eve_link(&mut self, login: String, name: String) {
        if !self.cfg.eve_enabled {
            self.sender
                .send(t!("@{login} EVE-Verknüpfung ist derzeit deaktiviert.", login = login));
            return;
        }
        if esi::clean_name(&name).is_empty() {
            self.sender
                .send(t!("@{login} Bitte einen EVE-Charakternamen angeben.", login = login));
            return;
        }
        let tx = self.tx.clone();
        let db = self.db.clone();
        std::thread::spawn(move || {
            let msg = match esi::validate_character(&name) {
                Ok(info) => match db.upsert_user(&NewUser {
                    twitch_login: login.clone(),
                    eve_character_name: Some(info.name.clone()),
                    eve_character_id: Some(info.id),
                    corp_ticker: info.corp.clone(),
                    alliance_ticker: info.alliance.clone(),
                    source: Some("esi-chat".into()),
                    ..Default::default()
                }) {
                    Ok(()) => Inbound::EveDone {
                        login,
                        ign: Some(info.name),
                        error: None,
                    },
                    Err(e) => Inbound::EveDone {
                        login,
                        ign: None,
                        error: Some(e.to_string()),
                    },
                },
                Err(e) => Inbound::EveDone {
                    login,
                    ign: None,
                    error: Some(e),
                },
            };
            let _ = tx.send(msg);
        });
    }

    fn drain_inbox(&mut self) {
        while let Ok(item) = self.rx.try_recv() {
            match item {
                Inbound::Chat { user, msg } => self.handle_chat(user, msg),
                Inbound::Log(t, tone) => self.log(t, tone),
                Inbound::Dock(DockCmd::Settings { settings }, reply) => {
                    // Erst prüfen und antworten, dann übernehmen: ein Port-Wechsel
                    // startet den Webserver neu, über den die Antwort gerade läuft.
                    let result = settings
                        .apply_to(&self.cfg)
                        .and_then(|c| self.validate_config(&c).map(|_| c));
                    match result {
                        Ok(cfg) => {
                            if let Some(r) = reply {
                                let _ = r.send(Ok(None));
                            }
                            self.log(t!("Dock: Einstellungen gespeichert"), Tone::Dark);
                            if let Err(e) = self.apply_config(cfg) {
                                self.log(t!("Einstellungen nicht übernommen: {e}", e = e), Tone::Red);
                            }
                        }
                        Err(e) => {
                            if let Some(r) = reply {
                                let _ = r.send(Err(e));
                            }
                        }
                    }
                }
                Inbound::Dock(cmd, reply) => {
                    let result = self.handle_dock(cmd);
                    if let Some(r) = reply {
                        let _ = r.send(result);
                    } else if let Err(e) = result {
                        self.log(e, Tone::Red);
                    }
                }
                Inbound::SsoCode { code, state } => self.sso_callback(code, state),
                Inbound::SsoLogin(result) => match result {
                    Ok(login) => {
                        self.cfg.eve_sso_refresh_token = login.refresh_token.clone();
                        self.cfg.eve_sso_character_id = Some(login.character_id);
                        self.cfg.eve_sso_character_name = login.character_name.clone();
                        if let Err(e) = config::save(&config::config_path(), &self.cfg) {
                            self.log(t!("EVE-Anmeldung nicht gespeichert: {e}", e = e), Tone::Red);
                        }
                        self.log(
                            t!("Mit EVE verbunden: {a1}", a1 = login.character_name),
                            Tone::Green,
                        );
                        self.assets_version += 1;
                        self.start_asset_load(Some(login));
                    }
                    Err(e) => {
                        self.assets_status = t!("Anmeldung fehlgeschlagen: {e}", e = e);
                        self.assets_version += 1;
                        self.log(t!("EVE-Anmeldung fehlgeschlagen: {e}", e = e), Tone::Red);
                    }
                },
                Inbound::Assets { result, refresh } => {
                    self.assets_loading = false;
                    if let Some(r) = refresh.filter(|r| *r != self.cfg.eve_sso_refresh_token) {
                        self.cfg.eve_sso_refresh_token = r;
                        let _ = config::save(&config::config_path(), &self.cfg);
                    }
                    match result {
                        Ok(rows) => {
                            self.assets_status = t!("{a1} Einträge geladen ({a2})", a1 = prize::fmt_qty(rows.len() as u64), a2 = chrono::Local::now().format("%H:%M"));
                            self.log(
                                t!("EVE-Assets geladen: {a1} Einträge.", a1 = rows.len()),
                                Tone::Cyan,
                            );
                            self.assets = rows;
                        }
                        Err(e) => {
                            self.assets_status = t!("Laden fehlgeschlagen: {e}", e = e);
                            self.log(t!("EVE-Assets nicht geladen: {e}", e = e), Tone::Red);
                        }
                    }
                    self.assets_version += 1;
                    self.request_repaint();
                }
                Inbound::IdResolved { login, info, error } => match info {
                    Some(info) => {
                        self.log(
                            t!("EVE-Charakter-ID für @{login} ergänzt ({a1}).", login = login, a1 = info.id),
                            Tone::Dark,
                        );
                        let card = self
                            .db
                            .get_user(&login)
                            .as_ref()
                            .and_then(EveCard::from_user);
                        for w in self.winners.iter_mut().filter(|w| w.login == login) {
                            w.eve = card.clone();
                        }
                        self.refresh_igns();
                    }
                    None => self.log(
                        t!("Charakter-ID für @{login} nicht gefunden: {a1}", login = login, a1 = error.unwrap_or_default()),
                        Tone::Yellow,
                    ),
                },
                Inbound::GameDone { ok, reply, log } => {
                    self.log(log, if ok { Tone::Cyan } else { Tone::Yellow });
                    self.sender.send(reply);
                    self.refresh_igns();
                }
                Inbound::UpdateChecked { result, manual } => {
                    self.update_busy = false;
                    match result {
                        Ok(Some(rel)) => {
                            self.update_status = t!("Version {a1} verfügbar", a1 = rel.version);
                            if rel.asset.is_none() {
                                self.update_status
                                    .push_str(&t!(" (für dieses System kein Download)"));
                            }
                            self.log(
                                t!("Update verfügbar: v{a1} (installiert: v{a2}) – Einstellungen oder Dock → Aktualisieren", a1 = rel.version, a2 = update::VERSION),
                                Tone::Magenta,
                            );
                            self.update = Some(rel);
                        }
                        Ok(None) => {
                            self.update = None;
                            self.update_status = format!("Aktuell (v{})", update::VERSION);
                            if manual {
                                self.log(
                                    t!("Keine neuere Version – v{a1} ist aktuell.", a1 = update::VERSION),
                                    Tone::Green,
                                );
                            }
                        }
                        Err(e) => {
                            self.update_status = t!("Prüfung fehlgeschlagen: {e}", e = e);
                            if manual {
                                self.log(
                                    t!("Update-Prüfung fehlgeschlagen: {e}", e = e),
                                    Tone::Yellow,
                                );
                            }
                        }
                    }
                    self.dirty = true;
                }
                Inbound::UpdateInstalled(res) => {
                    self.update_busy = false;
                    match res {
                        Ok(path) => {
                            self.update_status = t!("Update installiert – Neustart …").into();
                            self.log(
                                t!("Update installiert – das Programm startet neu."),
                                Tone::Green,
                            );
                            self.restart_to = Some(path);
                        }
                        Err(e) => {
                            self.update_status = t!("Update fehlgeschlagen: {e}", e = e);
                            self.log(t!("Update fehlgeschlagen: {e}", e = e), Tone::Red);
                        }
                    }
                    self.dirty = true;
                }
                Inbound::EveDone { login, ign, error } => {
                    if let Some(i) = ign {
                        self.eve_cache.insert(login.clone(), i.clone());
                        self.refresh_igns();
                        self.log(t!("EVE-Daten für @{login} übernommen.", login = login), Tone::Cyan);
                        self.sender
                            .send(t!("@{login} EVE-Verknüpfung gespeichert.", login = login));
                    } else {
                        self.log(
                            t!("EVE-Verknüpfung @{login} fehlgeschlagen: {a1}", login = login, a1 = error.unwrap_or_default()),
                            Tone::Yellow,
                        );
                        self.sender.send(t!("@{login} EVE-Verknüpfung fehlgeschlagen. Bitte Eingabe prüfen und erneut versuchen.", login = login));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ Overlay

    /// Design wechseln und speichern.
    pub fn set_theme(&mut self, id: &str) -> Result<(), String> {
        if !themes::registry().list().iter().any(|(t, ..)| t == id) {
            return Err(t!("Unbekanntes Design: {id}", id = id));
        }
        let mut c = self.cfg.clone();
        c.theme = id.to_string();
        self.apply_config(c)?;
        let name = themes::registry().get(id).info.name.clone();
        self.log(format!("Design: {name}", name = name), Tone::Dark);
        Ok(())
    }

    /// Eigene Designs neu einlesen; Fehler landen im Log.
    pub fn reload_themes(&mut self) {
        let msgs = themes::registry().reload();
        let n = themes::registry().list().len();
        for m in &msgs {
            self.log(m.clone(), Tone::Yellow);
        }
        self.log(t!("Designs neu geladen: {n} verfügbar", n = n), Tone::Dark);
        self.dirty = true;
    }

    /// ID des aktiven Designs (unbekannte → Standard).
    pub fn theme_id(&self) -> String {
        themes::registry().get(&self.cfg.theme).info.id.clone()
    }

    fn overlay_json(&self) -> String {
        let (mode, visible) = match self.phase {
            Phase::Ready => ("idle", false),
            // Große Teilnahme-Anzeige nur, wenn gewünscht – die Timer-Anzeige
            // (/timer) bekommt den Zustand trotzdem über `mode`.
            Phase::Open => ("open", self.cfg.overlay_show_open),
            Phase::Running => ("run", true),
            Phase::Won => ("winner", true),
        };
        let recent: Vec<&str> = self
            .participants
            .iter()
            .rev()
            .take(10)
            .map(String::as_str)
            .collect();
        let st = OverlayState {
            kind: "state",
            visible,
            mode,
            keyword: &self.active_keyword,
            count: self.participants.len(),
            recent,
            timer_active: self.timer.is_some() && self.phase == Phase::Open,
            timer_remaining: self.timer_remaining(),
            timer_total: self.timer_total,
            run: if matches!(self.phase, Phase::Running | Phase::Won) {
                self.run_view.as_ref()
            } else {
                None
            },
            winner: self.winners.first().map_or("", |w| w.login.as_str()),
            winners: self
                .winners
                .iter()
                .map(|w| Winner {
                    eve: if self.cfg.hide_eve_names {
                        None
                    } else {
                        w.eve.clone()
                    },
                    ign: if self.cfg.hide_eve_names {
                        None
                    } else {
                        w.ign.clone()
                    },
                    ..w.clone()
                })
                .collect(),
            winners_count: self.cfg.winners.max(1),
            prize: self.prize_summary(),
            status: &self.status,
            auto: self.auto_state(),
            theme: self.theme_id(),
            theme_v: themes::registry().version(),
        };
        let mut v = serde_json::to_value(&st).unwrap_or(Value::Null);
        if self.cfg.hide_eve_names {
            scrub(&mut v, self.igns.as_ref());
        }
        v.to_string()
    }

    fn publish(&mut self) {
        let counting =
            (self.phase == Phase::Open && self.timer.is_some()) || self.auto_deadline.is_some();
        let due = counting && self.last_publish.elapsed() >= Duration::from_millis(500);
        if !(self.dirty || due) {
            return;
        }
        self.dirty = false;
        self.last_publish = Instant::now();
        let json = self.overlay_json();
        let dock = self.dock_json();
        if let Some(o) = &self.overlay {
            o.publish(json);
            o.publish_dock(dock);
        }
    }

    // ------------------------------------------------------------ OBS-Dock

    fn handle_dock(&mut self, cmd: DockCmd) -> DockReply {
        match cmd {
            DockCmd::Toggle {
                keyword,
                duration,
                winners,
            } => {
                if let Some(n) = winners {
                    self.set_winners(n);
                }
                self.log(t!("Dock: START/STOP"), Tone::Dark);
                self.toggle_entries(&keyword, &duration);
            }
            DockCmd::Breach {
                keyword,
                hack,
                winners,
            } => {
                if let Some(n) = winners {
                    self.set_winners(n);
                }
                self.log(t!("Dock: BREACH"), Tone::Dark);
                self.start_breach(&keyword, &hack);
            }
            DockCmd::Reset => self.reset(),
            DockCmd::Auto { enabled } => {
                let mut c = self.cfg.clone();
                c.auto_enabled = enabled;
                if let Err(e) = self.apply_config(c) {
                    self.log(t!("Auto-Giveaway nicht umgeschaltet: {e}", e = e), Tone::Red);
                    return Err(e);
                }
            }
            DockCmd::Settings { settings } => {
                let cfg = settings.apply_to(&self.cfg)?;
                self.apply_config(cfg)?;
            }
            DockCmd::Winners { n } => self.set_winners(n),
            DockCmd::Prizes { prizes, text } => {
                let prizes = prizes
                    .into_iter()
                    .filter(|p| !p.name.trim().is_empty())
                    .map(|mut p| {
                        p.quantity = p.quantity.max(1);
                        p
                    })
                    .collect();
                self.set_prizes(prizes, text.trim().to_string());
            }
            DockCmd::Assets => {
                return Ok(Some(
                    serde_json::to_value(&self.assets).unwrap_or(Value::Null),
                ));
            }
            DockCmd::LoadAssets => self.load_assets(),
            DockCmd::SsoConnect => {
                let url = self.sso_begin()?;
                // Im Systembrowser öffnen (das Dock selbst läuft in OBS)
                let u = url.clone();
                std::thread::spawn(move || {
                    let _ = open::that(u);
                });
                return Ok(Some(Value::String(url)));
            }
            DockCmd::SsoLogout => self.sso_logout(),
            DockCmd::SsoClient { id } => self.set_sso_client_id(&id),
            DockCmd::SsoStructures { enabled } => self.set_sso_structures(enabled),
            DockCmd::Theme { id } => self.set_theme(&id)?,
            DockCmd::ReloadThemes => self.reload_themes(),
            DockCmd::UpdateCheck => self.check_update(true),
            DockCmd::UpdateInstall => self.install_update()?,
            DockCmd::ChatSend { text, id } => {
                let text = match id {
                    Some(id) => self
                        .chat_saved
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.text.clone())
                        .ok_or_else(|| t!("Nachricht nicht gefunden"))?,
                    None => text,
                };
                self.chat_send(&text)?;
                return Ok(None);
            }
            DockCmd::ChatSaved => return Ok(Some(self.chat_view())),
            DockCmd::ChatUpdate {
                id,
                pinned,
                repeat,
            } => {
                self.db
                    .chat_update(id, pinned, repeat)
                    .map_err(|e| e.to_string())?;
                if repeat.is_some() {
                    // Wiederholung zählt ab jetzt
                    self.chat_repeat.insert(id, (Instant::now(), self.chat_seen));
                }
                self.chat_reload();
                return Ok(None);
            }
            DockCmd::ChatDelete { id } => {
                self.db.chat_delete(id).map_err(|e| e.to_string())?;
                self.chat_repeat.remove(&id);
                self.chat_reload();
                return Ok(None);
            }
            DockCmd::ChatMinLines { n } => {
                let mut c = self.cfg.clone();
                c.chat_repeat_min_lines = n.min(1000);
                config::save(&config::config_path(), &c).map_err(|e| t!("Speichern fehlgeschlagen: {e}", e = e))?;
                self.cfg = c;
                crate::chat::saved_changed();
                return Ok(None);
            }
            DockCmd::GameNameDelete { login, game } => {
                self.db
                    .delete_game_name(&login, &game)
                    .map_err(|e| e.to_string())?;
                self.refresh_igns();
            }
        }
        self.dirty = true;
        Ok(None)
    }

    /// Einstellungen für das Dock – ohne Tokens und Schlüssel.
    fn settings_view(&self) -> Value {
        let c = &self.cfg;
        let mut v = serde_json::json!({
            "channel": c.channel, "bot_username": c.bot_username,
            "oauth_token_set": !c.oauth_token.trim().is_empty(),
            "keyword": c.keyword, "web_port": c.web_port, "timer_duration": c.timer_duration,
            "hack_duration": c.hack_duration, "decker_name": c.decker_name, "sound": c.sound,
            "auto_enabled": c.auto_enabled, "auto_interval": c.auto_interval, "auto_winner_hold": c.auto_winner_hold,
            "open_message": c.open_message, "close_message": c.close_message, "win_message": c.win_message,
            "eve_enabled": c.eve_enabled, "eve_command": c.eve_command,
            "hide_eve_names": c.hide_eve_names, "start_in_tray": c.start_in_tray, "close_to_tray": c.close_to_tray,
            "overlay_show_open": c.overlay_show_open,
            "language": c.language,
            "theme": self.theme_id(),
            "themes": themes::registry().list().into_iter()
                .map(|(id, name, builtin)| serde_json::json!({"id": id, "name": name, "builtin": builtin}))
                .collect::<Vec<_>>(),
            "winners": c.winners.max(1), "prize_text": c.prize_text, "prizes": c.prizes,
            "eve_sso_client_id": c.eve_sso_client_id, "eve_sso_structures": c.eve_sso_structures,
            "eve_sso_connected": self.sso_connected(), "eve_sso_character_name": c.eve_sso_character_name,
            "callback_url": sso::callback_url(self.callback_port()),
            "scopes": sso::scopes(c.eve_sso_structures),
            "assets_status": self.assets_status, "assets_loading": self.assets_loading,
            "assets_count": self.assets.len(), "assets_version": self.assets_version,
        });
        let extra = serde_json::json!({
            "ign_game": self.ign_game(),
            "games_text": c.games_text(),
            "wow_client_id": c.wow_client_id, "wow_client_secret_set": !c.wow_client_secret.trim().is_empty(),
            "wow_region": c.wow_region, "wow_realm": c.wow_realm,
            "update_check": c.update_check, "version": update::VERSION,
            "update": self.update, "update_status": self.update_status, "update_busy": self.update_busy,
        });
        if let (Some(m), Value::Object(x)) = (v.as_object_mut(), extra) {
            m.extend(x);
        }
        v
    }

    /// Statusdaten für das Bedien-Dock (enthält im Gegensatz zum Overlay
    /// auch Log, Einstellungen und EVE-Daten – nur lokal abrufbar).
    pub fn dock_json(&self) -> String {
        let phase = match self.phase {
            Phase::Ready => "ready",
            Phase::Open => "open",
            Phase::Running => "run",
            Phase::Won => "winner",
        };
        let log: Vec<DockLine> = self
            .log
            .iter()
            .rev()
            .take(14)
            .rev()
            .map(|l| DockLine {
                stamp: &l.stamp,
                text: &l.text,
                tone: tone_name(l.tone),
            })
            .collect();
        let recent: Vec<&str> = self
            .participants
            .iter()
            .rev()
            .take(8)
            .map(String::as_str)
            .collect();
        let run = self.run_view.as_ref().filter(|_| self.phase == Phase::Running).map(|r| {
            serde_json::json!({ "phase": r.phase_text, "os": r.os, "os_max": r.os_max, "marks": r.marks, "overall": r.overall })
        });
        serde_json::json!({
            "phase": phase,
            "status": self.status,
            "status_tone": tone_name(self.status_tone),
            "countdown": self.countdown_text,
            "countdown_tone": tone_name(self.countdown_tone),
            "timer_remaining": if self.phase == Phase::Open { self.timer_remaining() } else { 0.0 },
            "timer_active": self.phase == Phase::Open && self.timer.is_some(),
            "count": self.participants.len(),
            "recent": recent,
            "keyword": self.active_keyword,
            "duration": self.cfg.timer_duration,
            "hack": self.cfg.hack_duration,
            "winners": self.winners.iter().map(|w| serde_json::json!({
                "login": w.login, "display": w.display, "eve": w.eve, "ign": w.ign, "prize": w.prize
            })).collect::<Vec<_>>(),
            "winners_count": self.cfg.winners.max(1),
            "prize": self.prize_summary(),
            "auto": self.auto_state(),
            "auto_interval": self.cfg.auto_interval,
            "run": run,
            "log": log,
            "overlay_url": self.overlay_url,
            "settings": self.settings_view(),
        })
        .to_string()
    }

    // ------------------------------------------------------------ Konfiguration

    /// Prüft eine neue Konfiguration, ohne sie zu übernehmen.
    pub fn validate_config(&self, new: &Config) -> Result<(), String> {
        let giveaway = config::parse_duration(&new.timer_duration)?;
        if new.auto_enabled {
            let pause = config::parse_duration(&new.auto_interval)
                .map_err(|e| format!("Auto-Giveaway-Pause: {e}", e = e))?;
            if pause == 0 {
                return Err(t!("Auto-Giveaway: Pause muss größer als 0 sein").into());
            }
            if giveaway == 0 {
                return Err(
                    t!("Auto-Giveaway braucht eine Giveaway-Dauer größer als 0 (sonst endet es nie)")
                        .into(),
                );
            }
        }
        if new.web_port == 0 {
            return Err(t!("Port muss 1–65535 sein").into());
        }
        Ok(())
    }

    pub fn apply_config(&mut self, mut new: Config) -> Result<(), String> {
        if new.language != self.cfg.language {
            // unveränderte Standard-Chattexte wechseln mit der Sprache
            let target = new.language.clone();
            new.language = self.cfg.language.clone();
            new.switch_language(&target);
        }
        self.validate_config(&new)?;
        let auto_changed = new.auto_enabled != self.cfg.auto_enabled
            || new.auto_interval != self.cfg.auto_interval;
        let lang_changed = new.language != self.cfg.language;
        config::save(&config::config_path(), &new)
            .map_err(|e| t!("Speichern fehlgeschlagen: {e}", e = e))?;
        let restart_irc = new.channel != self.cfg.channel
            || new.bot_username != self.cfg.bot_username
            || new.oauth_token != self.cfg.oauth_token;
        let restart_web = new.web_port != self.cfg.web_port
            || new.decker() != self.cfg.decker()
            || self.overlay_url.is_empty();
        self.cfg = new;
        if lang_changed {
            // Programmtexte, Designs und Dock wechseln sofort die Sprache
            crate::i18n::set_lang(&self.cfg.language);
            for m in themes::registry().reload() {
                self.log(m, Tone::Yellow);
            }
            if self.phase == Phase::Ready && self.auto_text().is_none() {
                self.set_status(t!("BEREIT"), Tone::Cyan);
            }
        }
        if self.phase != Phase::Open && self.phase != Phase::Running {
            self.active_keyword = self.cfg.keyword_or_default();
        }
        if restart_irc {
            self.reader.stop();
            self.sender.stop();
            self.reader = TwitchReader::start(
                self.cfg.channel_clean(),
                self.cfg.channel_configured(),
                self.tx.clone(),
            );
            self.sender = TwitchSender::start(
                self.cfg.channel_clean(),
                self.cfg.bot_username.clone(),
                self.cfg.oauth_token.clone(),
                self.cfg.channel_configured(),
                self.tx.clone(),
            );
        }
        if restart_web {
            if let Some(mut o) = self.overlay.take() {
                o.stop();
            }
            self.start_overlay();
        }
        if auto_changed {
            self.auto_deadline = None;
            if self.cfg.auto_enabled {
                self.log(
                    t!("Auto-Giveaway aktiv: alle {a1} ein neues Giveaway.", a1 = self.cfg.auto_interval.trim()),
                    Tone::Cyan,
                );
            } else {
                self.log(t!("Auto-Giveaway ausgeschaltet."), Tone::Yellow);
            }
        }
        self.log(t!("Konfiguration gespeichert und übernommen."), Tone::Green);
        self.dirty = true;
        Ok(())
    }

    pub fn tick(&mut self) {
        self.drain_inbox();
        self.tick_timer();
        self.tick_run();
        self.tick_auto();
        self.tick_chat_repeat();
        if let Some(at) = self.win_sound_at {
            if Instant::now() >= at {
                self.win_sound_at = None;
                if !self.winners.is_empty() {
                    self.sfx(Sfx::Win2);
                }
            }
        }
        self.publish();
    }

    // ------------------------------------------------------------ Chat-Dock

    fn chat_reload(&mut self) {
        self.chat_saved = self.db.chat_saved();
        let ids: HashSet<i64> = self.chat_saved.iter().filter(|m| m.repeat_min > 0).map(|m| m.id).collect();
        self.chat_repeat.retain(|id, _| ids.contains(id));
        crate::chat::saved_changed();
    }

    /// Text über den Bot senden und merken.
    fn chat_send(&mut self, text: &str) -> Result<(), String> {
        if !self.sender.enabled() {
            return Err(t!("Zum Senden in den Einstellungen Bot-Account und OAuth-Token eintragen."));
        }
        let out = crate::chat::outgoing(text)?;
        self.sender.send(out);
        let text = text.replace(['\r', '\n'], " ").trim().to_string();
        self.db.chat_remember(&text).map_err(|e| e.to_string())?;
        if let Some(m) = self.db.chat_saved().into_iter().find(|m| m.text == text) {
            if m.repeat_min > 0 {
                self.chat_repeat.insert(m.id, (Instant::now(), self.chat_seen));
            }
        }
        self.chat_reload();
        Ok(())
    }

    /// Sekunden bis zur nächsten automatischen Wiederholung
    fn chat_next_in(&self, m: &crate::db::SavedMsg) -> Option<f64> {
        if m.repeat_min == 0 {
            return None;
        }
        let since = self
            .chat_repeat
            .get(&m.id)
            .map_or(0.0, |(at, _)| at.elapsed().as_secs_f64());
        Some((m.repeat_min as f64 * 60.0 - since).max(0.0))
    }

    fn chat_view(&self) -> Value {
        let items: Vec<Value> = self
            .chat_saved
            .iter()
            .map(|m| {
                let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
                v["next_in"] = self.chat_next_in(m).map_or(Value::Null, |s| serde_json::json!(s));
                v
            })
            .collect();
        serde_json::json!({
            "items": items,
            "bot_ok": self.sender.enabled(),
            "bot": self.cfg.bot_username.trim(),
            "channel": self.cfg.channel_clean(),
            "min_lines": self.cfg.chat_repeat_min_lines,
            "language": self.cfg.language,
        })
    }

    /// Gespeicherte Nachrichten mit Wiederholung automatisch senden.
    fn tick_chat_repeat(&mut self) {
        if self.chat_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.chat_check = Instant::now();
        if !self.sender.enabled() {
            return;
        }
        let need = self.cfg.chat_repeat_min_lines as u64;
        let seen = self.chat_seen;
        let mut due = Vec::new();
        for m in self.chat_saved.iter().filter(|m| m.repeat_min > 0) {
            let (at, seen_then) = *self
                .chat_repeat
                .entry(m.id)
                .or_insert((Instant::now(), seen));
            if at.elapsed().as_secs() >= m.repeat_min as u64 * 60 && seen - seen_then >= need {
                due.push(m.clone());
            }
        }
        let sent = !due.is_empty();
        for m in due {
            if let Ok(out) = crate::chat::outgoing(&m.text) {
                self.sender.send(out);
                self.log(t!("Chat-Wiederholung: {text}", text = m.text), Tone::Dark);
            }
            self.chat_repeat.insert(m.id, (Instant::now(), seen));
            let _ = self.db.chat_remember(&m.text);
        }
        if sent {
            // Liste aktualisieren (Zähler, „zuletzt“)
            self.chat_saved = self.db.chat_saved();
            crate::chat::saved_changed();
        }
    }

    // ------------------------------------------------------------ Auto-Giveaway

    pub fn auto_state(&self) -> AutoState {
        if !self.cfg.auto_enabled {
            return AutoState {
                enabled: false,
                state: "aus",
                remaining: 0.0,
                total: 0,
            };
        }
        match self.auto_deadline {
            Some(d) => AutoState {
                enabled: true,
                state: "wartet",
                remaining: d.saturating_duration_since(Instant::now()).as_secs_f64(),
                total: self.auto_total,
            },
            None => AutoState {
                enabled: true,
                state: "giveaway",
                remaining: 0.0,
                total: self.auto_total,
            },
        }
    }

    /// Text für das Programmfenster, z. B. „nächstes Giveaway in 12:34“.
    pub fn auto_text(&self) -> Option<String> {
        let a = self.auto_state();
        match a.state {
            "aus" => None,
            "wartet" => Some(t!("nächstes Giveaway in {a1}", a1 = config::format_clock(a.remaining))),
            _ => Some(t!("Giveaway läuft – danach beginnt der Countdown neu").into()),
        }
    }

    fn tick_auto(&mut self) {
        if !self.cfg.auto_enabled {
            if self.auto_deadline.take().is_some() {
                self.dirty = true;
            }
            return;
        }
        match self.phase {
            Phase::Open | Phase::Running => {
                if self.auto_deadline.take().is_some() {
                    self.dirty = true;
                }
            }
            Phase::Won => {
                // Gewinner eine Weile stehen lassen, dann leise zurück auf „bereit“
                let hold = Duration::from_secs_f64(self.cfg.auto_winner_hold.clamp(3.0, 3600.0));
                if self.won_at.is_some_and(|t| t.elapsed() >= hold) {
                    self.won_at = None;
                    self.phase = Phase::Ready;
                    self.participants.clear();
                    self.participant_set.clear();
                    self.winners.clear();
                    self.run_view = None;
                    self.set_status(t!("AUTO-GIVEAWAY: WARTET"), Tone::Cyan);
                }
            }
            Phase::Ready => match self.auto_deadline {
                None => {
                    let secs = config::parse_duration(&self.cfg.auto_interval)
                        .unwrap_or(1800)
                        .max(1);
                    self.auto_total = secs;
                    self.auto_deadline = Some(Instant::now() + Duration::from_secs(secs));
                    self.set_status(t!("AUTO-GIVEAWAY: WARTET"), Tone::Cyan);
                    self.log(
                        t!("Auto-Giveaway: nächstes Giveaway in {a1}.", a1 = config::format_clock(secs as f64)),
                        Tone::Cyan,
                    );
                    self.dirty = true;
                    self.request_repaint();
                }
                Some(d) if Instant::now() >= d => {
                    self.auto_deadline = None;
                    let duration = match config::parse_duration(&self.cfg.timer_duration) {
                        Ok(s) if s > 0 => self.cfg.timer_duration.clone(),
                        _ => "5:00".to_string(),
                    };
                    let kw = self.active_keyword.clone();
                    self.log(t!("Auto-Giveaway startet."), Tone::Green);
                    self.toggle_entries(&kw, &duration);
                }
                Some(d) => {
                    // Anzeige im Programmfenster einmal pro Sekunde aktualisieren
                    let sec = d.saturating_duration_since(Instant::now()).as_secs();
                    if sec != self.auto_last_sec {
                        self.auto_last_sec = sec;
                        self.request_repaint();
                    }
                }
            },
        }
    }

    // ------------------------------------------------------------ EVE-Assets

    pub fn sso_connected(&self) -> bool {
        !self.cfg.eve_sso_refresh_token.is_empty() && self.cfg.eve_sso_character_id.is_some()
    }

    /// Port, unter dem der Callback erreichbar ist (tatsächlicher Overlay-Port).
    pub fn callback_port(&self) -> u16 {
        self.overlay
            .as_ref()
            .and_then(|o| o.port)
            .unwrap_or(self.cfg.web_port)
    }

    /// Startet die EVE-Anmeldung; liefert die URL, die im Browser geöffnet wird.
    pub fn sso_begin(&mut self) -> Result<String, String> {
        if self.cfg.eve_sso_client_id.trim().is_empty() {
            return Err(
                t!("Bitte zuerst die Client-ID deiner EVE-App eintragen und speichern.").into(),
            );
        }
        if self.overlay.as_ref().and_then(|o| o.port).is_none() {
            return Err(
                t!("Der Webserver läuft nicht – ohne ihn kommt die Anmeldung nicht zurück.").into(),
            );
        }
        let (url, pending) = sso::begin(
            &self.cfg.eve_sso_client_id,
            self.callback_port(),
            self.cfg.eve_sso_structures,
        );
        self.sso_pending = Some(pending);
        self.assets_status = t!("Warte auf die Anmeldung im Browser …").into();
        self.assets_version += 1;
        self.log(t!("EVE-Anmeldung im Browser gestartet."), Tone::Cyan);
        Ok(url)
    }

    fn sso_callback(&mut self, code: String, state: String) {
        // Nur die zur letzten Anmeldung passende Rückkehr annehmen; eine alte
        // (z. B. aus einem früheren Browser-Tab) verbraucht die laufende nicht.
        match &self.sso_pending {
            None => {
                self.log(
                    t!("EVE-Anmeldung: Rückruf ohne laufende Anmeldung – bitte im Tool „Mit EVE verbinden“ klicken."),
                    Tone::Yellow,
                );
                return;
            }
            Some(p) if p.state != state => {
                self.log(
                    t!("EVE-Anmeldung: Rückruf aus einem älteren Anmeldeversuch ignoriert – bitte den zuletzt geöffneten Browser-Tab verwenden."),
                    Tone::Yellow,
                );
                return;
            }
            _ => {}
        }
        let Some(pending) = self.sso_pending.take() else {
            return;
        };
        let client = self.cfg.eve_sso_client_id.clone();
        let tx = self.tx.clone();
        self.assets_status = t!("Anmeldung wird abgeschlossen …").into();
        self.assets_version += 1;
        std::thread::spawn(move || {
            let _ = tx.send(Inbound::SsoLogin(sso::exchange(&client, &code, &pending)));
        });
    }

    /// Strukturnamen-Scope an/aus (gilt ab der nächsten Anmeldung).
    pub fn set_sso_structures(&mut self, on: bool) {
        if on == self.cfg.eve_sso_structures {
            return;
        }
        self.cfg.eve_sso_structures = on;
        if let Err(e) = config::save(&config::config_path(), &self.cfg) {
            self.log(t!("Einstellung nicht gespeichert: {e}", e = e), Tone::Red);
        }
    }

    pub fn set_sso_client_id(&mut self, id: &str) {
        let id = id.trim().to_string();
        if id == self.cfg.eve_sso_client_id {
            return;
        }
        self.cfg.eve_sso_client_id = id;
        // Neue App → alte Anmeldung gehört zur alten App
        self.cfg.eve_sso_refresh_token.clear();
        self.cfg.eve_sso_character_id = None;
        self.cfg.eve_sso_character_name.clear();
        if let Err(e) = config::save(&config::config_path(), &self.cfg) {
            self.log(t!("Client-ID nicht gespeichert: {e}", e = e), Tone::Red);
        }
        self.assets_version += 1;
    }

    pub fn sso_logout(&mut self) {
        self.cfg.eve_sso_refresh_token.clear();
        self.cfg.eve_sso_character_id = None;
        self.cfg.eve_sso_character_name.clear();
        let _ = config::save(&config::config_path(), &self.cfg);
        self.assets.clear();
        self.assets_status = "Abgemeldet".into();
        self.assets_version += 1;
        self.log(t!("EVE-Anmeldung entfernt."), Tone::Yellow);
    }

    /// Lädt die Assets neu (mit frischem Access-Token).
    pub fn load_assets(&mut self) {
        self.start_asset_load(None);
    }

    fn start_asset_load(&mut self, login: Option<sso::Login>) {
        if self.assets_loading {
            return;
        }
        if login.is_none() && !self.sso_connected() {
            self.assets_status = t!("Nicht mit EVE verbunden").into();
            self.assets_version += 1;
            return;
        }
        self.assets_loading = true;
        self.assets_status = t!("Assets werden geladen …").into();
        self.assets_version += 1;
        let client = self.cfg.eve_sso_client_id.clone();
        let refresh_token = self.cfg.eve_sso_refresh_token.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let (login, refresh) = match login {
                Some(l) => (Ok(l), None),
                None => match sso::refresh(&client, &refresh_token) {
                    Ok(l) => {
                        let r = Some(l.refresh_token.clone());
                        (Ok(l), r)
                    }
                    Err(e) => (Err(t!("{e} – ggf. neu verbinden", e = e)), None),
                },
            };
            let result = login.and_then(|l| sso::load_assets(&l));
            let _ = tx.send(Inbound::Assets { result, refresh });
        });
    }

    pub fn shutdown(&mut self) {
        self.cancel_timer(true);
        self.reader.stop();
        self.sender.stop();
        if let Some(mut o) = self.overlay.take() {
            o.stop();
        }
    }
}

/// Ersetzt EVE-Charakternamen in allen Texten durch „[EVE-NAME]“ – die IGNs
/// sollen nicht im Stream auftauchen.
pub fn build_scrubber(igns: &[String]) -> Option<regex::Regex> {
    if igns.is_empty() {
        return None;
    }
    let alt = igns
        .iter()
        .map(|i| regex::escape(i.trim()))
        .collect::<Vec<_>>()
        .join("|");
    regex::RegexBuilder::new(&format!("(?:{alt})"))
        .case_insensitive(true)
        .build()
        .ok()
}

pub fn scrub(v: &mut Value, re: Option<&regex::Regex>) {
    match v {
        Value::String(s) => {
            if let Some(re) = re {
                if re.is_match(s) {
                    *s = re.replace_all(s, "[EVE-NAME]").into_owned();
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| scrub(x, re)),
        Value::Object(m) => {
            m.retain(|k, _| {
                !matches!(
                    k.to_lowercase().as_str(),
                    "ign" | "eve_name" | "eve_character_name"
                )
            });
            m.values_mut().for_each(|x| scrub(x, re));
        }
        _ => {}
    }
}

/// Startet den Engine-Thread.
pub fn spawn(engine: Arc<Mutex<Engine>>) {
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || loop {
            let restart = {
                let Ok(mut e) = engine.lock() else { return };
                e.tick();
                e.restart_to.clone()
            };
            // Update eingespielt: auch im Tray ohne sichtbares Fenster neu starten
            if let Some(exe) = restart {
                crate::restart_after_update(&engine, &exe);
            }
            std::thread::sleep(Duration::from_millis(50));
        })
        .expect("engine thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_giveaway_flow() {
        let dir = std::env::temp_dir().join(format!("cg-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        db.upsert_user(&NewUser {
            twitch_login: "bob".into(),
            eve_character_name: Some("Bobby Pilot".into()),
            ..Default::default()
        })
        .unwrap();
        let cfg = Config {
            web_port: 18766,
            keyword: "!hack".into(),
            sound: false,
            hack_duration: 8.0,
            hide_eve_names: true,
            ..Default::default()
        };
        let mut e = Engine::new(cfg, db);
        assert!(e.overlay_url.contains("1876"));
        e.toggle_entries("!hack", "0");
        assert_eq!(e.phase, Phase::Open);
        for (u, m) in [
            ("alice", "!hack"),
            ("bob", "!HACK"),
            ("alice", "!hack"),
            ("nightbot", "!hack"),
            ("carol", "hallo"),
            ("dave", " !hack "),
        ] {
            e.tx.send(Inbound::Chat {
                user: u.into(),
                msg: m.into(),
            })
            .unwrap();
        }
        e.tick();
        assert_eq!(e.participants, vec!["alice", "bob", "dave"]);
        // Overlay-JSON darf den EVE-Namen nicht enthalten
        e.dirty = true;
        assert!(!e.overlay_json().to_lowercase().contains("bobby pilot"));
        e.start_breach("!hack", "8");
        assert_eq!(e.phase, Phase::Running);
        let start = Instant::now();
        while e.phase == Phase::Running && start.elapsed() < Duration::from_secs(15) {
            e.tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(e.phase, Phase::Won);
        assert_eq!(e.winners.len(), 1);
        assert!(e.participants.contains(&e.winners[0].login));
        assert_eq!(e.db.history(5).len(), 1);
        let json = e.overlay_json();
        assert!(json.contains("\"mode\":\"winner\""));
        assert!(!json.to_lowercase().contains("bobby pilot"));
        e.reset();
        assert_eq!(e.phase, Phase::Ready);
        e.shutdown();
    }

    #[test]
    fn winner_portrait_in_overlay() {
        let dir = std::env::temp_dir().join(format!("cg-eve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        db.upsert_user(&NewUser {
            twitch_login: "areadox".into(),
            eve_character_name: Some("daulin".into()),
            eve_character_id: Some(2112625428),
            corp_ticker: Some("ABC".into()),
            ..Default::default()
        })
        .unwrap();
        let cfg = Config {
            web_port: 18866,
            sound: false,
            ..Default::default()
        };
        let mut e = Engine::new(cfg, db);
        e.toggle_entries("!join", "0");
        e.tx.send(Inbound::Chat {
            user: "areadox".into(),
            msg: "!join".into(),
        })
        .unwrap();
        e.tick();
        e.finish_breach(vec!["areadox".into()]);
        let json = e.overlay_json();
        assert!(json.contains("images.evetech.net/characters/2112625428/portrait"));
        assert!(json.contains("\"name\":\"daulin\""));
        assert_eq!(
            e.winners[0].eve.as_ref().unwrap().corp.as_deref(),
            Some("ABC")
        );
        e.cfg.hide_eve_names = true;
        e.refresh_igns();
        let hidden = e.overlay_json();
        assert!(!hidden.contains("portrait"));
        assert!(!hidden.to_lowercase().contains("daulin"));
        e.shutdown();
    }

    #[test]
    fn auto_giveaway_cycle() {
        let dir = std::env::temp_dir().join(format!("cg-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let cfg = Config {
            web_port: 18966,
            sound: false,
            keyword: "!hack".into(),
            hack_duration: 8.0,
            timer_duration: "0:02".into(),
            auto_enabled: true,
            auto_interval: "0:01".into(),
            auto_winner_hold: 3.0,
            ..Default::default()
        };
        let mut e = Engine::new(cfg, db);
        let run_until = |e: &mut Engine, secs: u64, cond: &dyn Fn(&Engine) -> bool| {
            let start = Instant::now();
            while !cond(e) && start.elapsed() < Duration::from_secs(secs) {
                e.tick();
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(cond(e), "Bedingung nicht erreicht");
        };
        e.tick();
        assert_eq!(e.auto_state().state, "wartet");
        assert!(e.overlay_json().contains("\"state\":\"wartet\""));
        // Countdown läuft ab → Giveaway öffnet sich selbst
        run_until(&mut e, 5, &|e| e.phase == Phase::Open);
        assert_eq!(e.auto_state().state, "giveaway");
        e.tx.send(Inbound::Chat {
            user: "alice".into(),
            msg: "!hack".into(),
        })
        .unwrap();
        // Giveaway-Countdown endet → Run → Gewinner
        run_until(&mut e, 20, &|e| e.phase == Phase::Won);
        assert_eq!(e.winners[0].login, "alice");
        // nach der Gewinner-Anzeige beginnt der Countdown von vorn
        run_until(&mut e, 8, &|e| {
            e.phase == Phase::Ready && e.auto_state().state == "wartet"
        });
        assert!(e.participants.is_empty() && e.winners.is_empty());
        // Ohne Teilnehmer endet das Giveaway und der Kreislauf geht weiter
        run_until(&mut e, 5, &|e| e.phase == Phase::Open);
        run_until(&mut e, 5, &|e| e.phase == Phase::Ready);
        run_until(&mut e, 3, &|e| e.auto_state().state == "wartet");
        // Ausschalten stoppt den Kreislauf
        e.cfg.auto_enabled = false;
        e.tick();
        assert_eq!(e.auto_state().state, "aus");
        assert!(e.auto_text().is_none());
        e.shutdown();
    }

    #[test]
    fn auto_config_validation() {
        let dir = std::env::temp_dir().join(format!("cg-autov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let mut e = Engine::new(
            Config {
                web_port: 19066,
                sound: false,
                ..Default::default()
            },
            db,
        );
        let bad = Config {
            auto_enabled: true,
            timer_duration: "0".into(),
            ..e.cfg.clone()
        };
        assert!(e.apply_config(bad).is_err());
        let bad = Config {
            auto_enabled: true,
            auto_interval: "0".into(),
            ..e.cfg.clone()
        };
        assert!(e.apply_config(bad).is_err());
        e.shutdown();
    }

    #[test]
    fn several_winners_with_prizes() {
        let dir = std::env::temp_dir().join(format!("cg-multi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let prizes = vec![
            Prize {
                name: "Gila".into(),
                quantity: 1,
                type_id: Some(17715),
                ..Default::default()
            },
            Prize {
                name: "PLEX".into(),
                quantity: 500,
                type_id: Some(44992),
                ..Default::default()
            },
        ];
        let cfg = Config {
            web_port: 19166,
            sound: false,
            keyword: "!hack".into(),
            hack_duration: 8.0,
            winners: 3,
            prizes,
            ..Default::default()
        };
        let mut e = Engine::new(cfg, db);
        assert_eq!(e.prize_summary(), "Gila, 500× PLEX");
        e.toggle_entries("!hack", "0");
        for u in ["a1", "a2", "a3", "a4", "a5"] {
            e.tx.send(Inbound::Chat {
                user: u.into(),
                msg: "!hack".into(),
            })
            .unwrap();
        }
        e.tick();
        let json = e.overlay_json();
        assert!(json.contains("\"prize\":\"Gila, 500× PLEX\""));
        assert!(json.contains("\"winners_count\":3"));
        e.start_breach("!hack", "8");
        let start = Instant::now();
        while e.phase == Phase::Running && start.elapsed() < Duration::from_secs(15) {
            e.tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(e.phase, Phase::Won);
        assert_eq!(e.winners.len(), 3);
        let mut logins: Vec<&str> = e.winners.iter().map(|w| w.login.as_str()).collect();
        logins.sort();
        logins.dedup();
        assert_eq!(logins.len(), 3, "keine doppelten Gewinner");
        assert_eq!(e.winners[0].prize.as_ref().unwrap().label, "Gila");
        assert!(e.winners[0]
            .prize
            .as_ref()
            .unwrap()
            .icon
            .as_ref()
            .unwrap()
            .contains("/types/17715/"));
        assert_eq!(e.winners[1].prize.as_ref().unwrap().label, "500× PLEX");
        assert!(e.winners[2].prize.is_none());
        let hist = e.db.history(10);
        assert_eq!(hist.len(), 3);
        assert_eq!(hist.iter().filter(|h| h.prize.is_some()).count(), 2);
        let json = e.overlay_json();
        assert_eq!(json.matches("\"login\"").count(), 3);
        // Mehr Gewinner als Teilnehmer → alle gewinnen
        e.reset();
        e.set_winners(10);
        e.toggle_entries("!hack", "0");
        e.tx.send(Inbound::Chat {
            user: "solo".into(),
            msg: "!hack".into(),
        })
        .unwrap();
        e.tick();
        e.start_breach("!hack", "8");
        let start = Instant::now();
        while e.phase == Phase::Running && start.elapsed() < Duration::from_secs(15) {
            e.tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(e.winners.len(), 1);
        assert_eq!(e.winners[0].login, "solo");
        e.shutdown();
    }

    #[test]
    fn game_names_per_theme() {
        let dir = std::env::temp_dir().join(format!("cg-ign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("g.db"));
        let db = Db::open(&dir.join("g.db")).unwrap();
        db.upsert_user(&NewUser {
            twitch_login: "ali".into(),
            eve_character_name: Some("Ali Pilot".into()),
            ..Default::default()
        })
        .unwrap();
        let mut e = Engine::new(
            Config {
                web_port: 19368,
                sound: false,
                theme: "palia".into(),
                ..Default::default()
            },
            db,
        );
        let chat = |e: &mut Engine, user: &str, msg: &str| {
            e.tx.send(Inbound::Chat {
                user: user.into(),
                msg: msg.into(),
            })
            .unwrap();
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(400) {
                e.tick();
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        chat(&mut e, "ali", "!palia Ali Sternenstaub");
        chat(&mut e, "ali", "!wow Thrall-Blackrock");
        assert_eq!(
            e.db.get_game_name("ali", "palia").unwrap().name,
            "Ali Sternenstaub"
        );
        assert_eq!(
            e.db.get_game_name("ali", "wow").unwrap().name,
            "Thrall-Blackrock",
            "ohne Blizzard-Zugang ungeprüft"
        );
        // Palia-Design zeigt den Palia-Namen, keine EVE-Daten
        let c = e.ign_card("ali").unwrap();
        assert_eq!(
            (c.game.as_str(), c.name.as_str()),
            ("palia", "Ali Sternenstaub")
        );
        assert_eq!(e.display_name("ali"), "ali // Ali Sternenstaub");
        // EVE-Design: wie bisher
        e.cfg.theme = "eve".into();
        assert!(e.ign_card("ali").is_none());
        assert_eq!(e.display_name("ali"), "ali // Ali Pilot");
        // Design ohne Spielname des Zuschauers → nur Login
        e.cfg.theme = "wow".into();
        assert_eq!(e.ign_card("ali").unwrap().name, "Thrall-Blackrock");
        e.db.delete_game_name("ali", "wow").unwrap();
        assert_eq!(e.display_name("ali"), "ali");
        // Gewinner bekommt die Karte
        e.cfg.theme = "palia".into();
        e.toggle_entries("!hack", "0");
        chat(&mut e, "ali", "!hack");
        e.start_breach("!hack", "8");
        let t = Instant::now();
        while e.phase == Phase::Running && t.elapsed() < Duration::from_secs(15) {
            e.tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(e.winners[0].ign.as_ref().unwrap().name, "Ali Sternenstaub");
        assert!(e.winners[0].eve.is_none());
        assert!(e.overlay_json().contains("Ali Sternenstaub"));
        // Secret nie im Dock
        e.cfg.wow_client_secret = "supergeheim".into();
        assert!(!e.dock_json().contains("supergeheim"));
        e.shutdown();
    }

    #[test]
    fn dock_settings_and_secrets() {
        let base = Config {
            oauth_token: "geheim123".into(),
            ..Default::default()
        };
        // Leeres Token-Feld lässt das Token unverändert
        let p = SettingsPatch {
            oauth_token: Some(String::new()),
            keyword: Some("!hack".into()),
            web_port: Some(9000),
            ..Default::default()
        };
        let c = p.apply_to(&base).unwrap();
        assert_eq!(c.oauth_token, "geheim123");
        assert_eq!(c.keyword, "!hack");
        assert_eq!(c.web_port, 9000);
        let p = SettingsPatch {
            oauth_token: Some(" neu ".into()),
            ..Default::default()
        };
        assert_eq!(p.apply_to(&base).unwrap().oauth_token, "neu");
        assert!(SettingsPatch {
            web_port: Some(70000),
            ..Default::default()
        }
        .apply_to(&base)
        .is_err());
        assert!(SettingsPatch {
            hack_duration: Some(2.0),
            ..Default::default()
        }
        .apply_to(&base)
        .is_err());
        // JSON aus dem Dock wird korrekt gelesen
        let cmd: DockCmd = serde_json::from_str(r#"{"cmd":"settings","settings":{"channel":"areadox","sound":false,"auto_winner_hold":15}}"#).unwrap();
        let DockCmd::Settings { settings } = cmd else {
            panic!("falscher Befehl")
        };
        let c = settings.apply_to(&base).unwrap();
        assert_eq!(
            (c.channel.as_str(), c.sound, c.auto_winner_hold),
            ("areadox", false, 15.0)
        );
        let cmd: DockCmd = serde_json::from_str(
            r#"{"cmd":"prizes","prizes":[{"name":"Gila","quantity":2,"type_id":17715}],"text":""}"#,
        )
        .unwrap();
        assert!(matches!(cmd, DockCmd::Prizes { .. }));

        let dir = std::env::temp_dir().join(format!("cg-docks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let mut e = Engine::new(
            Config {
                web_port: 19266,
                sound: false,
                oauth_token: "geheim123".into(),
                    eve_sso_refresh_token: "refresh789".into(),
                eve_sso_character_id: Some(1),
                ..Default::default()
            },
            db,
        );
        let dock = e.dock_json();
        for secret in ["geheim123", "key456", "refresh789"] {
            assert!(!dock.contains(secret), "{secret} darf nicht ins Dock");
        }
        assert!(dock.contains("\"oauth_token_set\":true"));
        assert!(dock.contains("\"eve_sso_connected\":true"));
        assert!(e.handle_dock(DockCmd::Winners { n: 4 }).is_ok());
        assert_eq!(e.cfg.winners, 4);
        e.shutdown();
    }

    #[test]
    fn open_panel_can_be_hidden() {
        let dir = std::env::temp_dir().join(format!("cg-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let mut e = Engine::new(
            Config {
                web_port: 19366,
                sound: false,
                overlay_show_open: false,
                ..Default::default()
            },
            db,
        );
        e.toggle_entries("!hack", "1:00");
        let json = e.overlay_json();
        // Großes Overlay unsichtbar, Timer-Anzeige kennt die offene Phase trotzdem
        assert!(json.contains("\"visible\":false"));
        assert!(json.contains("\"mode\":\"open\""));
        assert!(json.contains("\"timer_active\":true"));
        e.cfg.overlay_show_open = true;
        assert!(e.overlay_json().contains("\"visible\":true"));
        e.shutdown();
    }

    #[test]
    fn theme_switch() {
        let dir = std::env::temp_dir().join(format!("cg-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        let mut e = Engine::new(
            Config {
                web_port: 19367,
                sound: false,
                ..Default::default()
            },
            db,
        );
        assert!(e.overlay_json().contains("\"theme\":\"shadowrun\""));
        assert!(e
            .handle_dock(DockCmd::Theme {
                id: "gibtsnicht".into()
            })
            .is_err());
        assert!(e.handle_dock(DockCmd::Theme { id: "eve".into() }).is_ok());
        assert_eq!(e.cfg.theme, "eve");
        assert!(e.overlay_json().contains("\"theme\":\"eve\""));
        let dock = e.dock_json();
        assert!(dock.contains("\"themes\":[") && dock.contains("\"anime\""));
        // Unbekanntes Design in der Config → Standard, statt kaputter Anzeige
        e.cfg.theme = "weg".into();
        assert_eq!(e.theme_id(), "shadowrun");
        // Run nutzt die Begriffe des Designs
        e.cfg.theme = "wow".into();
        for n in ["a", "b", "c"] {
            e.participants.push(n.into());
        }
        e.phase = Phase::Open;
        e.start_breach("!hack", "8");
        let frames = &e.run.as_ref().unwrap().frames;
        assert_eq!(frames.last().unwrap().view.phase, "LOOT");
        // Patch aus dem Dock
        let patch = SettingsPatch {
            theme: Some("palia".into()),
            ..Default::default()
        };
        assert_eq!(patch.apply_to(&e.cfg).unwrap().theme, "palia");
        e.shutdown();
    }

    #[test]
    fn scrub_names() {
        let mut v =
            serde_json::json!({"winner": "areadox", "log": ["Gewinner: Daulin!"], "ign": "daulin"});
        scrub(&mut v, build_scrubber(&["daulin".to_string()]).as_ref());
        assert_eq!(v["log"][0], "Gewinner: [EVE-NAME]!");
        assert!(v.get("ign").is_none());
        assert_eq!(v["winner"], "areadox");
    }

    #[test]
    fn chat_dock_merkt_und_wiederholt() {
        let cmd: DockCmd = serde_json::from_str(r#"{"cmd":"chatsend","id":3}"#).unwrap();
        assert_eq!(cmd, DockCmd::ChatSend { text: String::new(), id: Some(3) });
        let dir = std::env::temp_dir().join(format!("cg-chatdock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("g.db")).unwrap();
        // ohne Bot: Senden wird abgelehnt
        let mut e = Engine::new(
            Config { web_port: 19466, sound: false, channel: "areadox".into(), ..Default::default() },
            db.clone(),
        );
        assert!(e.handle_dock(DockCmd::ChatSend { text: "hi".into(), id: None }).is_err());
        e.shutdown();
        let mut e = Engine::new(
            Config {
                web_port: 19476,
                sound: false,
                channel: "areadox".into(),
                bot_username: "areadoxbot".into(),
                oauth_token: "x".into(),
                chat_repeat_min_lines: 2,
                ..Default::default()
            },
            db,
        );
        e.handle_dock(DockCmd::ChatSend { text: "Discord: x".into(), id: None }).unwrap();
        e.handle_dock(DockCmd::ChatSend { text: "/me winkt".into(), id: None }).unwrap();
        assert!(e.handle_dock(DockCmd::ChatSend { text: "/ban a".into(), id: None }).is_err());
        let id = e.chat_saved.iter().find(|m| m.text == "Discord: x").unwrap().id;
        e.handle_dock(DockCmd::ChatSend { text: String::new(), id: Some(id) }).unwrap();
        assert_eq!(e.chat_saved.iter().find(|m| m.id == id).unwrap().uses, 2);
        e.handle_dock(DockCmd::ChatUpdate { id, pinned: Some(true), repeat: Some(5) }).unwrap();
        let view = e.chat_view();
        assert_eq!(view["items"][0]["id"], id);
        assert!(view["items"][0]["next_in"].as_f64().unwrap() > 290.0);
        // Zeit abgelaufen, aber noch zu wenig Chat: keine Wiederholung
        let past = Instant::now() - Duration::from_secs(301);
        e.chat_repeat.insert(id, (past, e.chat_seen));
        e.chat_check = past;
        e.tick();
        assert_eq!(e.chat_saved.iter().find(|m| m.id == id).unwrap().uses, 2);
        for u in ["a", "areadoxbot", "b"] {
            e.tx.send(Inbound::Chat { user: u.into(), msg: "hallo".into() }).unwrap();
        }
        e.chat_check = past;
        e.tick();
        assert_eq!(e.chat_saved.iter().find(|m| m.id == id).unwrap().uses, 3);
        e.handle_dock(DockCmd::ChatDelete { id }).unwrap();
        assert!(e.chat_saved.iter().all(|m| m.id != id));
        e.shutdown();
    }
}