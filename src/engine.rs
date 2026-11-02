//! Spiellogik: Teilnahme, Countdown, Matrix-Run, Chat, Overlay-Status.
//!
//! Läuft in einem eigenen Thread (Tick alle 50 ms), damit Countdown und Run
//! auch weiterlaufen, wenn das Fenster minimiert ist. Die GUI greift über
//! `Arc<Mutex<Engine>>` zu.

use crate::config::{self, Config};
use crate::db::{clean_login, Db, NewUser};
use crate::esi;
use crate::matrix::{self, Frame, RunSetup, RunView};
use crate::overlay::OverlayServer;
use crate::sound::{self, Sfx};
use crate::twitch::{TwitchReader, TwitchSender};
use rand::seq::SliceRandom;
use serde::Serialize;
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
    JoinReady {
        login: String,
        ign: Option<String>,
    },
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
    winner: String,
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
    winner: &'a str,
    status: &'a str,
}

pub struct Engine {
    pub cfg: Config,
    pub db: Db,
    pub phase: Phase,
    pub participants: Vec<String>,
    participant_set: HashSet<String>,
    pub active_keyword: String,
    pub winner: String,
    pub winner_display: String,
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
            winner: String::new(),
            winner_display: String::new(),
            status: "BEREIT".into(),
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
        };
        e.refresh_igns();
        e.log(
            "System initialisiert; Twitch-Reader wird gestartet.",
            Tone::Green,
        );
        e.log(
            format!(
                "Keyword: {} | Ausschlüsse: {}",
                e.active_keyword,
                EXCLUDE_USERS.join(", ")
            ),
            Tone::Dark,
        );
        e.start_overlay();
        e
    }

    pub fn set_repaint(&mut self, f: Box<dyn Fn() + Send>) {
        self.repaint = Some(f);
    }

    fn start_overlay(&mut self) {
        let initial = self.overlay_json();
        let srv = OverlayServer::start(self.cfg.web_port, initial, self.cfg.decker());
        match (&srv.port, &srv.error) {
            (Some(p), _) => {
                self.overlay_url = format!("http://localhost:{p}");
                self.log(
                    format!("OVERLAY: {0} | DEMO: {0}/test", self.overlay_url),
                    Tone::Cyan,
                );
                if *p != self.cfg.web_port {
                    self.log(
                        format!("Port {} belegt – Overlay läuft auf {p}.", self.cfg.web_port),
                        Tone::Yellow,
                    );
                }
            }
            (None, Some(e)) => {
                self.overlay_url.clear();
                self.log(format!("Overlay-Fehler: {e}"), Tone::Red);
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

    fn set_status(&mut self, s: &str, tone: Tone) {
        self.status = s.into();
        self.status_tone = tone;
        self.dirty = true;
    }

    pub fn display_name(&self, login: &str) -> String {
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
            self.log("BREACH läuft bereits.", Tone::Yellow);
            return;
        }
        let secs = match config::parse_duration(duration_text) {
            Ok(s) => s,
            Err(e) => {
                self.sfx(Sfx::Error);
                self.log(format!("Ungültige Giveaway-Dauer: {e}"), Tone::Red);
                return;
            }
        };
        self.cancel_timer(true);
        self.participants.clear();
        self.participant_set.clear();
        self.winner.clear();
        self.winner_display.clear();
        self.run_view = None;
        self.phase = Phase::Open;
        self.set_status("TEILNAHME OFFEN", Tone::Green);
        self.log(
            format!("EINGÄNGE OFFEN — Chat: {}", self.active_keyword),
            Tone::Green,
        );
        let open = self
            .cfg
            .open_message
            .replace("{keyword}", &self.active_keyword);
        if secs > 0 {
            self.sender.send(format!(
                "{open} Dauer: {}.",
                config::format_clock(secs as f64)
            ));
        } else {
            self.sender.send(format!("{open} Manuelles Ende."));
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
                "Teilnahme gestoppt; keine Teilnehmer vorhanden.",
                Tone::Yellow,
            );
            return;
        }
        self.sender.send(self.cfg.close_message.clone());
        self.log(
            "Teilnahme gestoppt; Matrix-Run und Auslosung starten.",
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
                "Ungültige Run-Dauer (mind. 8 s); bisheriger Wert bleibt.",
                Tone::Yellow,
            ),
        }
        self.sfx(Sfx::Click);
        if self.phase == Phase::Running {
            self.log("BREACH läuft bereits.", Tone::Yellow);
            return;
        }
        if self.participants.is_empty() {
            self.sfx(Sfx::Error);
            self.log(
                "FEHLER: Keine Personas im Pool. Erst Teilnehmer erfassen.",
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
        let winner = self
            .participants
            .choose(&mut rng)
            .cloned()
            .unwrap_or_default();
        let duration = self.cfg.hack_duration.clamp(8.0, 600.0);
        let frames = matrix::generate(
            &mut rng,
            RunSetup {
                participants: &self.participants,
                winner: &winner,
                decker: &self.cfg.decker(),
                duration,
            },
        );
        self.phase = Phase::Running;
        self.winner.clear();
        self.winner_display.clear();
        self.set_status("MATRIX-RUN LÄUFT", Tone::Red);
        self.log(
            format!(
                "BREACH INITIALISIERT // {} Personas im Pool; Run-Dauer {duration:.1}s",
                self.participants.len()
            ),
            Tone::Red,
        );
        self.run = Some(ActiveRun {
            frames,
            started: Instant::now(),
            next: 0,
            log_seen: 0,
            winner,
        });
        self.dirty = true;
    }

    fn finish_breach(&mut self, winner: String) {
        let display = self.display_name(&winner);
        self.winner = winner.clone();
        self.winner_display = display.clone();
        self.phase = Phase::Won;
        self.set_status("BREACH ERFOLGREICH", Tone::Green);
        self.log(
            format!("BREACH ERFOLGREICH! Gewinner: @{display}"),
            Tone::Yellow,
        );
        self.win_sound_at = Some(Instant::now() + Duration::from_millis(380));
        if let Err(e) = self.db.add_history(&display, self.participants.len()) {
            self.log(
                format!("Verlauf konnte nicht gespeichert werden: {e}"),
                Tone::Red,
            );
        }
        self.sender
            .send(self.cfg.win_message.replace("{winner}", &winner));
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.cancel_timer(true);
        self.sfx(Sfx::Click);
        self.phase = Phase::Ready;
        self.participants.clear();
        self.participant_set.clear();
        self.winner.clear();
        self.winner_display.clear();
        self.run = None;
        self.run_view = None;
        self.win_sound_at = None;
        self.log.clear();
        self.set_status("BEREIT / TEILNAHME GESCHLOSSEN", Tone::Cyan);
        self.log(
            "System zurückgesetzt. Pool geleert; Entries geschlossen.",
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
            self.sender.send(format!(
                "⏱ Noch {th} Sekunden! Teilnahme mit {}.",
                self.active_keyword
            ));
        }
        if remaining <= 0.0 {
            self.cancel_timer(true);
            if self.participants.is_empty() {
                self.sender.send(format!(
                    "⏱ Giveaway beendet: keine Teilnehmer. Schreibe beim nächsten Mal {}.",
                    self.active_keyword
                ));
                self.log(
                    "Countdown abgelaufen; keine Teilnehmer. Giveaway sauber beendet.",
                    Tone::Yellow,
                );
                self.phase = Phase::Ready;
                self.set_status("GESTOPPT", Tone::Cyan);
                return;
            }
            self.sender.send(self.cfg.close_message.clone());
            self.log(
                "Countdown abgelaufen; automatischer Matrix-Run startet.",
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
                finished = Some(run.winner.clone());
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
            format!("PERSONA ERFASST: @{display} [{}]", self.participants.len()),
            Tone::Cyan,
        );
        if display != name {
            self.sender.send(format!("@{name} ist dabei."));
        }
        self.dirty = true;
    }

    fn handle_chat(&mut self, user: String, msg: String) {
        let msg = msg.trim().to_string();
        if let Some((twitch, ign)) = esi::parse_eve2twitch_message(&msg) {
            let bot = clean_login(&self.cfg.eve2twitch_bot_name);
            if bot.is_empty() || user == bot {
                match self.db.upsert_user(&NewUser {
                    twitch_login: twitch.clone(),
                    eve_character_name: Some(ign.clone()),
                    source: Some("eve2twitch-chat".into()),
                    ..Default::default()
                }) {
                    Ok(()) => self.log(
                        format!("EVE-Zuordnung übernommen: @{twitch} // {ign}"),
                        Tone::Cyan,
                    ),
                    Err(e) => self.log(format!("EVE-Zuordnung fehlgeschlagen: {e}"), Tone::Red),
                }
                self.refresh_igns();
                return;
            }
        }
        let lower = msg.to_lowercase();
        if lower == self.active_keyword.to_lowercase() {
            if self.phase != Phase::Open {
                return;
            }
            let api = self.cfg.eve_api_url.trim().to_string();
            let key = self.cfg.eve_api_key.trim().to_string();
            if self.cfg.eve_enabled
                && !api.is_empty()
                && !key.is_empty()
                && self.db.get_user(&user).is_none()
            {
                let tx = self.tx.clone();
                let db = self.db.clone();
                std::thread::spawn(move || {
                    let mut ign = None;
                    if let Some(info) = esi::lookup_eve2twitch(&api, &key, &user) {
                        if db
                            .upsert_user(&NewUser {
                                twitch_login: user.clone(),
                                eve_character_name: Some(info.name.clone()),
                                eve_character_id: Some(info.id),
                                corp_ticker: info.corp,
                                alliance_ticker: info.alliance,
                                source: Some("eve2twitch-api".into()),
                                ..Default::default()
                            })
                            .is_ok()
                        {
                            ign = Some(info.name);
                        }
                    }
                    let _ = tx.send(Inbound::JoinReady { login: user, ign });
                });
            } else {
                self.add_participant(&user);
            }
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

    fn handle_eve_link(&mut self, login: String, name: String) {
        if !self.cfg.eve_enabled {
            self.sender
                .send(format!("@{login} EVE-Verknüpfung ist derzeit deaktiviert."));
            return;
        }
        if esi::clean_name(&name).is_empty() {
            self.sender
                .send(format!("@{login} Bitte einen EVE-Charakternamen angeben."));
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
                Inbound::JoinReady { login, ign } => {
                    if let Some(i) = ign {
                        self.eve_cache.insert(login.clone(), i);
                        self.refresh_igns();
                    }
                    self.add_participant(&login);
                }
                Inbound::EveDone { login, ign, error } => {
                    if let Some(i) = ign {
                        self.eve_cache.insert(login.clone(), i.clone());
                        self.refresh_igns();
                        self.log(format!("EVE-Daten für @{login} übernommen."), Tone::Cyan);
                        self.sender
                            .send(format!("@{login} EVE-Verknüpfung gespeichert."));
                    } else {
                        self.log(
                            format!(
                                "EVE-Verknüpfung @{login} fehlgeschlagen: {}",
                                error.unwrap_or_default()
                            ),
                            Tone::Yellow,
                        );
                        self.sender.send(format!(
                            "@{login} EVE-Verknüpfung fehlgeschlagen. Bitte Eingabe prüfen und erneut versuchen."
                        ));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ Overlay

    fn overlay_json(&self) -> String {
        let (mode, visible) = match self.phase {
            Phase::Ready => ("idle", false),
            Phase::Open => ("open", true),
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
            winner: &self.winner,
            status: &self.status,
        };
        let mut v = serde_json::to_value(&st).unwrap_or(Value::Null);
        scrub(&mut v, self.igns.as_ref());
        v.to_string()
    }

    fn publish(&mut self) {
        let due = self.phase == Phase::Open
            && self.timer.is_some()
            && self.last_publish.elapsed() >= Duration::from_millis(500);
        if !(self.dirty || due) {
            return;
        }
        self.dirty = false;
        self.last_publish = Instant::now();
        let json = self.overlay_json();
        if let Some(o) = &self.overlay {
            o.publish(json);
        }
    }

    // ------------------------------------------------------------ Konfiguration

    pub fn apply_config(&mut self, new: Config) -> Result<(), String> {
        config::parse_duration(&new.timer_duration)?;
        if new.web_port == 0 {
            return Err("Port muss 1–65535 sein".into());
        }
        config::save(&config::config_path(), &new)
            .map_err(|e| format!("Speichern fehlgeschlagen: {e}"))?;
        let restart_irc = new.channel != self.cfg.channel
            || new.bot_username != self.cfg.bot_username
            || new.oauth_token != self.cfg.oauth_token;
        let restart_web = new.web_port != self.cfg.web_port
            || new.decker() != self.cfg.decker()
            || self.overlay_url.is_empty();
        self.cfg = new;
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
        self.log("Konfiguration gespeichert und übernommen.", Tone::Green);
        self.dirty = true;
        Ok(())
    }

    pub fn tick(&mut self) {
        self.drain_inbox();
        self.tick_timer();
        self.tick_run();
        if let Some(at) = self.win_sound_at {
            if Instant::now() >= at {
                self.win_sound_at = None;
                if !self.winner.is_empty() {
                    self.sfx(Sfx::Win2);
                }
            }
        }
        self.publish();
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
            {
                let Ok(mut e) = engine.lock() else { return };
                e.tick();
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
        assert!(e.participants.contains(&e.winner));
        assert_eq!(e.db.history(5).len(), 1);
        let json = e.overlay_json();
        assert!(json.contains("\"mode\":\"winner\""));
        assert!(!json.to_lowercase().contains("bobby pilot"));
        e.reset();
        assert_eq!(e.phase, Phase::Ready);
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
}
