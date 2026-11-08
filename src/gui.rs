//! Programmfenster (egui): Steuerung, Terminal, Einstellungen, Datenbank.

use crate::config::Config;
use crate::db::{HistoryRow, NewUser, User};
use crate::engine::{Engine, Phase, Tone};
use crate::prize::{fmt_qty, type_icon_url, Prize};
use eframe::egui::{self, Color32, RichText, Stroke};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BG: Color32 = Color32::from_rgb(5, 7, 9);
const PANEL: Color32 = Color32::from_rgb(8, 17, 14);
const GREEN: Color32 = Color32::from_rgb(57, 255, 136);
const RED: Color32 = Color32::from_rgb(255, 49, 88);
const CYAN: Color32 = Color32::from_rgb(34, 227, 255);
const MAGENTA: Color32 = Color32::from_rgb(255, 47, 179);
const YELLOW: Color32 = Color32::from_rgb(255, 196, 0);
const DIM: Color32 = Color32::from_rgb(85, 112, 91);

fn tone(t: Tone) -> Color32 {
    match t {
        Tone::Green => GREEN,
        Tone::Red => RED,
        Tone::Cyan => CYAN,
        Tone::Yellow => YELLOW,
        Tone::Magenta => MAGENTA,
        Tone::Dark => DIM,
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    General,
    Messages,
    Prizes,
    Database,
    History,
}

struct EditState {
    original: Option<String>,
    login: String,
    eve_name: String,
    eve_id: String,
    twitch_id: String,
    corp: String,
    alliance: String,
    source: String,
    error: Option<String>,
}

impl EditState {
    fn from_user(u: Option<&User>) -> Self {
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        match u {
            Some(u) => Self {
                original: Some(u.twitch_login.clone()),
                login: u.twitch_login.clone(),
                eve_name: o(&u.eve_character_name),
                eve_id: u
                    .eve_character_id
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                twitch_id: o(&u.twitch_id),
                corp: o(&u.corp_ticker),
                alliance: o(&u.alliance_ticker),
                source: u.source.clone().unwrap_or_else(|| "manuell".into()),
                error: None,
            },
            None => Self {
                original: None,
                login: String::new(),
                eve_name: String::new(),
                eve_id: String::new(),
                twitch_id: String::new(),
                corp: String::new(),
                alliance: String::new(),
                source: "manuell".into(),
                error: None,
            },
        }
    }
}

struct Settings {
    tab: Tab,
    draft: Config,
    port: String,
    hack: String,
    error: Option<String>,
    rows: Vec<User>,
    history: Vec<HistoryRow>,
    prizes: Vec<Prize>,
    prize_text: String,
    client_id: String,
    asset_search: String,
    assets_seen: u64,
    manual_name: String,
    manual_qty: u64,
    prize_info: Option<String>,
    selected: Option<usize>,
    edit: Option<EditState>,
    confirm_delete: bool,
    db_seen: u64,
    info: Option<String>,
    show_token: bool,
}

pub struct GuiApp {
    engine: Arc<Mutex<Engine>>,
    keyword: String,
    hack: String,
    duration: String,
    settings: Option<Settings>,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, engine: Arc<Mutex<Engine>>) -> Self {
        style(&cc.egui_ctx);
        // Lädt EVE-Porträts direkt von images.evetech.net
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let (keyword, hack, duration, needs_setup) = {
            let mut e = engine.lock().expect("engine");
            e.set_repaint(Box::new(move || ctx.request_repaint()));
            (
                e.cfg.keyword_or_default(),
                format!("{}", e.cfg.hack_duration),
                e.cfg.timer_duration.clone(),
                e.cfg.needs_setup(),
            )
        };
        let mut app = Self {
            engine,
            keyword,
            hack,
            duration,
            settings: None,
        };
        if needs_setup {
            app.open_settings();
        }
        app
    }

    fn open_settings(&mut self) {
        let e = self.engine.lock().expect("engine");
        self.settings = Some(Settings {
            tab: Tab::General,
            draft: e.cfg.clone(),
            port: e.cfg.web_port.to_string(),
            hack: format!("{}", e.cfg.hack_duration),
            error: None,
            rows: e.db.list_users(),
            history: e.db.history(100),
            selected: None,
            edit: None,
            confirm_delete: false,
            db_seen: e.db_version,
            info: None,
            show_token: false,
            prizes: e.cfg.prizes.clone(),
            prize_text: e.cfg.prize_text.clone(),
            client_id: e.cfg.eve_sso_client_id.clone(),
            asset_search: String::new(),
            assets_seen: e.assets_version,
            manual_name: String::new(),
            manual_qty: 1,
            prize_info: None,
        });
    }
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(13, 30, 22);
    v.faint_bg_color = Color32::from_rgb(10, 22, 17);
    v.selection.bg_fill = Color32::from_rgb(8, 90, 50);
    v.selection.stroke = Stroke::new(1.0_f32, GREEN);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 70, 45));
    v.widgets.inactive.bg_fill = Color32::from_rgb(10, 24, 18);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(34, 96, 64));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, GREEN);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(10, 24, 18);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(16, 40, 30);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(20, 55, 40);
    v.override_text_color = Some(Color32::from_rgb(210, 255, 225));
    ctx.set_visuals(v);
    let mut s = (*ctx.style()).clone();
    for (_, font) in s.text_styles.iter_mut() {
        font.family = egui::FontFamily::Monospace;
    }
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(s);
}

fn big_button(ui: &mut egui::Ui, text: &str, color: Color32, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 36.0],
        egui::Button::new(RichText::new(text).color(color).strong().size(15.0))
            .stroke(Stroke::new(1.0_f32, color))
            .fill(Color32::from_rgb(6, 14, 11)),
    )
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut open_settings = false;
        let mut action: Option<&'static str> = None;
        let mut new_winners: Option<u32> = None;
        {
            let e = self.engine.lock().expect("engine");
            egui::TopBottomPanel::top("header")
                .frame(egui::Frame::NONE.fill(BG).inner_margin(10.0))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .stroke(Stroke::new(1.0_f32, GREEN))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("// MATRIX-RUN :: GIVEAWAY")
                                        .color(GREEN)
                                        .size(17.0)
                                        .strong(),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("⚙ EINSTELLUNGEN").color(MAGENTA),
                                                )
                                                .stroke(Stroke::new(1.0_f32, MAGENTA)),
                                            )
                                            .clicked()
                                        {
                                            open_settings = true;
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("KEYWORD").color(CYAN));
                        ui.add(egui::TextEdit::singleline(&mut self.keyword).desired_width(110.0));
                        ui.add_space(10.0);
                        ui.label(RichText::new("RUN (s)").color(CYAN));
                        ui.add(egui::TextEdit::singleline(&mut self.hack).desired_width(50.0))
                            .on_hover_text("Dauer des Matrix-Runs im Overlay (mind. 8 Sekunden)");
                        ui.add_space(10.0);
                        ui.label(RichText::new("GEWINNER").color(CYAN));
                        let mut n = e.cfg.winners.max(1);
                        if ui
                            .add(egui::DragValue::new(&mut n).range(1..=100))
                            .on_hover_text("Anzahl der Gewinner pro Giveaway")
                            .changed()
                        {
                            new_winners = Some(n);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("GIVEAWAY-DAUER").color(CYAN));
                        ui.add(egui::TextEdit::singleline(&mut self.duration).desired_width(70.0));
                        ui.label(
                            RichText::new("Minuten oder mm:ss (0 = manuell)")
                                .color(DIM)
                                .size(11.0),
                        );
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("STATUS: {}", e.status)).color(tone(e.status_tone)),
                    );
                    ui.label(
                        RichText::new(format!("COUNTDOWN: {}", e.countdown_text))
                            .color(tone(e.countdown_tone))
                            .size(16.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("TEILNEHMER: {}", e.participants.len())).color(GREEN),
                    );
                    let prize = e.prize_summary();
                    if !prize.is_empty() {
                        ui.label(RichText::new(format!("PREIS: {prize}")).color(YELLOW));
                    }
                    if let Some(t) = e.auto_text() {
                        ui.label(RichText::new(format!("AUTO-GIVEAWAY: {t}")).color(CYAN));
                    }
                    if let Some(rv) = e.run_view.as_ref().filter(|_| e.phase == Phase::Running) {
                        ui.label(
                            RichText::new(format!(
                                "MATRIX: {} // OW {}/{} // Marken {}/3",
                                rv.phase_text, rv.os, rv.os_max, rv.marks
                            ))
                            .color(MAGENTA),
                        );
                    }
                    let blink = !e.winners.is_empty()
                        && (ctx.input(|i| i.time) * 1000.0 / 450.0) as i64 % 2 == 0;
                    if e.winners.is_empty() {
                        ui.label(
                            RichText::new("GEWINNER: —")
                                .color(YELLOW)
                                .size(16.0)
                                .strong(),
                        );
                    }
                    let many = e.winners.len() > 1;
                    for (i, w) in e.winners.iter().take(5).enumerate() {
                        ui.horizontal(|ui| {
                            if let Some(id) = w.eve.as_ref().and_then(|c| c.id) {
                                portrait(ui, id, if many { 40.0 } else { 64.0 });
                            }
                            ui.vertical(|ui| {
                                let head = if many {
                                    format!("GEWINNER {}", i + 1)
                                } else {
                                    "GEWINNER".into()
                                };
                                ui.label(
                                    RichText::new(format!("{head}: @{}", w.display.to_uppercase()))
                                        .color(if blink { RED } else { YELLOW })
                                        .size(if many { 14.0 } else { 16.0 })
                                        .strong(),
                                );
                                let mut info = vec![];
                                if let Some(c) = &w.eve {
                                    let tick = [
                                        c.corp.as_ref().map(|t| format!("[{t}]")),
                                        c.alliance.as_ref().map(|t| format!("<{t}>")),
                                    ]
                                    .into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                    info.push(format!("EVE: {} {tick}", c.name).trim().to_string());
                                }
                                if let Some(p) = &w.prize {
                                    info.push(format!("Preis: {}", p.label));
                                }
                                if !info.is_empty() {
                                    ui.label(RichText::new(info.join("  |  ")).color(CYAN));
                                }
                            });
                        });
                    }
                    if e.winners.len() > 5 {
                        ui.label(
                            RichText::new(format!(
                                "… und {} weitere (siehe Protokoll und Verlauf)",
                                e.winners.len() - 5
                            ))
                            .color(DIM),
                        );
                    }
                    if !e.winners.is_empty() {
                        ctx.request_repaint_after(Duration::from_millis(150));
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let w = ((ui.available_width() - 16.0) / 3.0).max(80.0);
                        let (label, color) = if e.phase == Phase::Open {
                            ("STOP & AUSLOSEN", RED)
                        } else {
                            ("START", GREEN)
                        };
                        if big_button(ui, label, color, w).clicked() {
                            action = Some("toggle");
                        }
                        if big_button(ui, "BREACH", RED, w)
                            .on_hover_text("Matrix-Run sofort starten")
                            .clicked()
                        {
                            action = Some("breach");
                        }
                        if big_button(ui, "RESET", YELLOW, w).clicked() {
                            action = Some("reset");
                        }
                    });
                    ui.add_space(2.0);
                    if e.overlay_url.is_empty() {
                        ui.label(RichText::new("Overlay nicht aktiv").color(RED).size(11.0));
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("OBS:").color(CYAN).size(11.0));
                            ui.hyperlink_to(
                                RichText::new(&e.overlay_url).size(11.0),
                                &e.overlay_url,
                            );
                            ui.label(RichText::new("| DEMO:").color(CYAN).size(11.0));
                            let demo = format!("{}/test", e.overlay_url);
                            ui.hyperlink_to(RichText::new(&demo).size(11.0), &demo);
                            ui.label(RichText::new("| TIMER:").color(CYAN).size(11.0));
                            let timer = format!("{}/timer", e.overlay_url);
                            ui.hyperlink_to(RichText::new(&timer).size(11.0), &timer)
                                .on_hover_text(
                                "Kleine Anzeige für OBS (z. B. 520 × 110); Vorschau mit ?demo=1",
                            );
                            ui.label(RichText::new("| DOCK:").color(CYAN).size(11.0));
                            let dock = format!("{}/dock", e.overlay_url);
                            ui.hyperlink_to(RichText::new(&dock).size(11.0), &dock)
                                .on_hover_text(
                                    "Bedien-Dock für OBS: Docks → Benutzerdefinierte Browser-Docks",
                                );
                        });
                    }
                });

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 0,
                    bottom: 10,
                }))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(8, 124, 44)))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("TERMINAL // SYSTEM-LOG").color(CYAN));
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for l in &e.log {
                                        ui.label(
                                            RichText::new(format!("[{}] {}", l.stamp, l.text))
                                                .color(tone(l.tone))
                                                .size(12.0),
                                        );
                                    }
                                });
                        });
                });
        }

        if let Some(n) = new_winners {
            self.engine.lock().expect("engine").set_winners(n);
        }
        match action {
            Some("toggle") => self
                .engine
                .lock()
                .expect("engine")
                .toggle_entries(&self.keyword, &self.duration),
            Some("breach") => self
                .engine
                .lock()
                .expect("engine")
                .start_breach(&self.keyword, &self.hack),
            Some("reset") => self.engine.lock().expect("engine").reset(),
            _ => {}
        }
        if open_settings && self.settings.is_none() {
            self.open_settings();
        }
        self.settings_window(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut e) = self.engine.lock() {
            e.shutdown();
        }
    }
}

impl GuiApp {
    fn settings_window(&mut self, ctx: &egui::Context) {
        if self.settings.is_none() {
            return;
        }
        let mut close = false;
        let engine = self.engine.clone();
        let mut saved_keyword: Option<(String, String, String)> = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings"),
            egui::ViewportBuilder::default()
                .with_title("MATRIX-RUN // EINSTELLUNGEN")
                .with_inner_size([820.0, 640.0])
                .with_min_inner_size([680.0, 520.0]),
            |ctx, _class| {
                let st = self.settings.as_mut().expect("settings");
                if ctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                // DB-Änderungen aus dem Chat übernehmen
                {
                    let e = engine.lock().expect("engine");
                    if e.db_version != st.db_seen {
                        st.db_seen = e.db_version;
                        st.rows = e.db.list_users();
                    }
                }
                egui::TopBottomPanel::top("tabs")
                    .frame(egui::Frame::NONE.fill(BG).inner_margin(8.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            for (t, label) in [
                                (Tab::General, "Einstellungen"),
                                (Tab::Messages, "Chat & EVE"),
                                (Tab::Prizes, "Preise"),
                                (Tab::Database, "Datenbank"),
                                (Tab::History, "Verlauf"),
                            ] {
                                if ui
                                    .selectable_label(
                                        st.tab == t,
                                        RichText::new(label).color(if st.tab == t {
                                            GREEN
                                        } else {
                                            CYAN
                                        }),
                                    )
                                    .clicked()
                                {
                                    st.tab = t;
                                    if t == Tab::History {
                                        st.history = engine.lock().expect("engine").db.history(100);
                                    }
                                }
                            }
                        });
                    });
                if matches!(st.tab, Tab::General | Tab::Messages) {
                    egui::TopBottomPanel::bottom("save")
                        .frame(egui::Frame::NONE.fill(BG).inner_margin(10.0))
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                if let Some(err) = &st.error {
                                    ui.label(RichText::new(err).color(RED));
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if big_button(ui, "Speichern & übernehmen", GREEN, 240.0)
                                            .clicked()
                                        {
                                            let mut cfg = st.draft.clone();
                                            let port = st.port.trim().parse::<u32>();
                                            let hack =
                                                st.hack.trim().replace(',', ".").parse::<f64>();
                                            match (port, hack) {
                                                (Ok(p), Ok(h))
                                                    if (1..=65535).contains(&p)
                                                        && h.is_finite()
                                                        && h >= 8.0 =>
                                                {
                                                    cfg.web_port = p as u16;
                                                    cfg.hack_duration = h.min(600.0);
                                                    if cfg.keyword.trim().is_empty() {
                                                        cfg.keyword = "!join".into();
                                                    }
                                                    let res = {
                                                        let mut e = engine.lock().expect("engine");
                                                        // Werte, die woanders gepflegt werden, nicht überschreiben
                                                        cfg.winners = e.cfg.winners;
                                                        cfg.prizes = e.cfg.prizes.clone();
                                                        cfg.prize_text = e.cfg.prize_text.clone();
                                                        cfg.eve_sso_client_id =
                                                            e.cfg.eve_sso_client_id.clone();
                                                        cfg.eve_sso_refresh_token =
                                                            e.cfg.eve_sso_refresh_token.clone();
                                                        cfg.eve_sso_character_id =
                                                            e.cfg.eve_sso_character_id;
                                                        cfg.eve_sso_character_name =
                                                            e.cfg.eve_sso_character_name.clone();
                                                        e.apply_config(cfg.clone())
                                                    };
                                                    match res {
                                                        Ok(()) => {
                                                            saved_keyword = Some((
                                                                cfg.keyword_or_default(),
                                                                cfg.timer_duration.clone(),
                                                                format!("{}", cfg.hack_duration),
                                                            ));
                                                            close = true;
                                                        }
                                                        Err(e) => st.error = Some(e),
                                                    }
                                                }
                                                (Ok(p), _) if !(1..=65535).contains(&p) => {
                                                    st.error = Some("Port muss 1–65535 sein".into())
                                                }
                                                (Err(_), _) => {
                                                    st.error =
                                                        Some("Port muss eine Zahl sein".into())
                                                }
                                                _ => {
                                                    st.error =
                                                        Some("Run-Dauer: Zahl ab 8 Sekunden".into())
                                                }
                                            }
                                        }
                                    },
                                );
                            });
                        });
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(BG).inner_margin(12.0))
                    .show(ctx, |ui| match st.tab {
                        Tab::General => general_tab(ui, st),
                        Tab::Messages => messages_tab(ui, st),
                        Tab::Prizes => prizes_tab(ui, st, &engine),
                        Tab::Database => database_tab(ui, st, &engine),
                        Tab::History => history_tab(ui, st),
                    });
                if let Some(edit) = st.edit.as_mut() {
                    let mut done = false;
                    let mut keep = true;
                    egui::Window::new(if edit.original.is_some() {
                        "Zuordnung bearbeiten"
                    } else {
                        "Zuordnung hinzufügen"
                    })
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut keep)
                    .show(ctx, |ui| {
                        if let Ok(id) = edit.eve_id.trim().parse::<i64>() {
                            portrait(ui, id, 128.0);
                            ui.add_space(4.0);
                        }
                        egui::Grid::new("edit")
                            .num_columns(2)
                            .spacing([10.0, 8.0])
                            .show(ui, |ui| {
                                for (label, v) in [
                                    ("Twitch-Login", &mut edit.login),
                                    ("EVE-Charaktername", &mut edit.eve_name),
                                    ("EVE-Charakter-ID", &mut edit.eve_id),
                                    ("Twitch-ID", &mut edit.twitch_id),
                                    ("Corp-Ticker", &mut edit.corp),
                                    ("Allianz-Ticker", &mut edit.alliance),
                                    ("Quelle", &mut edit.source),
                                ] {
                                    ui.label(RichText::new(label).color(CYAN));
                                    ui.add(egui::TextEdit::singleline(v).desired_width(260.0));
                                    ui.end_row();
                                }
                            });
                        if let Some(err) = &edit.error {
                            ui.label(RichText::new(err).color(RED));
                        }
                        if ui.button(RichText::new("Speichern").color(GREEN)).clicked() {
                            let id = edit.eve_id.trim();
                            let id = if id.is_empty() {
                                Ok(None)
                            } else {
                                id.parse::<i64>().map(Some)
                            };
                            match id {
                                Err(_) => {
                                    edit.error = Some("EVE-Charakter-ID muss eine Zahl sein".into())
                                }
                                Ok(id) => {
                                    let mut e = engine.lock().expect("engine");
                                    let new_login = crate::db::clean_login(&edit.login);
                                    if let Some(orig) = &edit.original {
                                        if crate::db::clean_login(orig) != new_login
                                            && !new_login.is_empty()
                                        {
                                            let _ = e.db.delete_user(orig);
                                        }
                                    }
                                    let some = |s: &str| {
                                        (!s.trim().is_empty()).then(|| s.trim().to_string())
                                    };
                                    match e.db.upsert_user(&NewUser {
                                        twitch_login: edit.login.clone(),
                                        eve_character_name: some(&edit.eve_name),
                                        eve_character_id: id,
                                        twitch_id: some(&edit.twitch_id),
                                        corp_ticker: some(&edit.corp),
                                        alliance_ticker: some(&edit.alliance),
                                        source: some(&edit.source),
                                    }) {
                                        Ok(()) => {
                                            e.refresh_igns();
                                            done = true;
                                        }
                                        Err(err) => edit.error = Some(err.to_string()),
                                    }
                                }
                            }
                        }
                    });
                    if done || !keep {
                        st.edit = None;
                    }
                }
            },
        );
        if let Some((k, d, h)) = saved_keyword {
            if self
                .engine
                .lock()
                .map(|e| e.phase != Phase::Open)
                .unwrap_or(true)
            {
                self.keyword = k;
            }
            self.duration = d;
            self.hack = h;
        }
        if close {
            if let Some(st) = &self.settings {
                let mut e = self.engine.lock().expect("engine");
                let text = st.prize_text.trim().to_string();
                if text != e.cfg.prize_text || st.prizes != e.cfg.prizes {
                    e.set_prizes(st.prizes.clone(), text);
                }
            }
            self.settings = None;
        }
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool, hint: &str) {
    ui.label(RichText::new(label).color(CYAN));
    let r = ui.add(
        egui::TextEdit::singleline(value)
            .password(password)
            .desired_width(f32::INFINITY),
    );
    if !hint.is_empty() {
        r.on_hover_text(hint);
    }
    ui.end_row();
}

fn general_tab(ui: &mut egui::Ui, st: &mut Settings) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("general").num_columns(2).spacing([14.0, 10.0]).min_col_width(220.0).show(ui, |ui| {
            let d = &mut st.draft;
            field(ui, "Twitch-Channel", &mut d.channel, false, "Kanal, dessen Chat gelesen wird");
            field(ui, "Bot-Username", &mut d.bot_username, false, "Account, der Chatnachrichten sendet");
            ui.label(RichText::new("OAuth-Token").color(CYAN));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.oauth_token).password(!st.show_token).desired_width(ui.available_width() - 90.0));
                ui.checkbox(&mut st.show_token, "zeigen");
            });
            ui.end_row();
            field(ui, "Teilnahme-Keyword", &mut d.keyword, false, "z. B. !hack");
            field(ui, "Webserver-Port", &mut st.port, false, "Port für das OBS-Overlay (Fallback: die nächsten 9 Ports)");
            field(ui, "Giveaway-Dauer (mm:ss oder Minuten)", &mut d.timer_duration, false, "0 = manuelles Ende");
            field(ui, "Matrix-Run-Dauer (Sekunden)", &mut st.hack, false, "Wie lange der simulierte Run im Overlay dauert (mind. 8)");
            field(ui, "Decker-Name im Overlay", &mut d.decker_name, false, "Leer = Twitch-Channel");
            ui.label(RichText::new("Sound").color(CYAN));
            ui.checkbox(&mut d.sound, "Soundeffekte im Programm");
            ui.end_row();
            ui.label(RichText::new("Auto-Giveaway").color(CYAN));
            ui.checkbox(&mut d.auto_enabled, "an – öffnet Giveaways selbst und beginnt danach von vorn");
            ui.end_row();
            field(ui, "Pause bis zum nächsten Giveaway", &mut d.auto_interval, false, "mm:ss oder Minuten, z. B. 30:00");
            ui.label(RichText::new("Gewinner-Anzeige im Auto-Modus").color(CYAN));
            ui.add(egui::DragValue::new(&mut d.auto_winner_hold).range(3.0..=3600.0).speed(1.0).suffix(" s"));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Im Auto-Modus gilt die Giveaway-Dauer von oben (muss größer als 0 sein). Die Timer-Anzeige für OBS liegt unter /timer.").color(DIM).size(11.0));
        ui.add_space(10.0);
        ui.label(RichText::new("Das OAuth-Token wird im Klartext in config.json neben der .exe gespeichert – die Datei nicht weitergeben.").color(DIM).size(11.0));
    });
}

fn messages_tab(ui: &mut egui::Ui, st: &mut Settings) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("messages")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .min_col_width(220.0)
            .show(ui, |ui| {
                let d = &mut st.draft;
                field(
                    ui,
                    "Chattext bei Start",
                    &mut d.open_message,
                    false,
                    "Platzhalter {keyword}",
                );
                field(ui, "Chattext bei Ende", &mut d.close_message, false, "");
                field(
                    ui,
                    "Chattext Gewinner",
                    &mut d.win_message,
                    false,
                    "Platzhalter {winner}",
                );
                ui.label(RichText::new("EVE-Modus").color(CYAN));
                ui.checkbox(&mut d.eve_enabled, "aktiv");
                ui.end_row();
                ui.label(RichText::new("EVE-Namen im Overlay").color(CYAN));
                ui.checkbox(&mut d.hide_eve_names, "verbergen (kein Porträt, Name als [EVE-NAME])");
                ui.end_row();
                field(
                    ui,
                    "EVE-Befehl",
                    &mut d.eve_command,
                    false,
                    "z. B. !eve – Zuschauer schreiben „!eve Charaktername“",
                );
                field(ui, "eve2twitch API-URL", &mut d.eve_api_url, false, "");
                field(ui, "eve2twitch API-Key", &mut d.eve_api_key, true, "");
                field(
                    ui,
                    "Eve2Twitch-Bot-Nick (leer = alle)",
                    &mut d.eve2twitch_bot_name,
                    false,
                    "",
                );
            });
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "Gewinnt ein registrierter Spieler, zeigt das Overlay sein EVE-Porträt mit Name und Tickern.",
            )
            .color(DIM)
            .size(11.0),
        );
    });
}

fn database_tab(ui: &mut egui::Ui, st: &mut Settings, engine: &Arc<Mutex<Engine>>) {
    ui.horizontal(|ui| {
        if ui.button("Hinzufügen").clicked() {
            st.edit = Some(EditState::from_user(None));
        }
        let sel = st.selected.and_then(|i| st.rows.get(i)).cloned();
        if ui
            .add_enabled(sel.is_some(), egui::Button::new("Bearbeiten"))
            .clicked()
        {
            st.edit = Some(EditState::from_user(sel.as_ref()));
        }
        if ui
            .add_enabled(sel.is_some(), egui::Button::new("Löschen"))
            .clicked()
        {
            st.confirm_delete = true;
        }
        if ui.button("CSV exportieren").clicked() {
            // Dialog ohne gehaltene Engine-Sperre öffnen
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name("eve-zuordnungen.csv")
                .save_file()
            {
                let db = engine.lock().expect("engine").db.clone();
                st.info = Some(match db.export_csv(&path) {
                    Ok(()) => format!("Zuordnungen exportiert: {}", path.display()),
                    Err(e) => format!("Export fehlgeschlagen: {e}"),
                });
            }
        }
        if ui.button("Aktualisieren").clicked() {
            st.rows = engine.lock().expect("engine").db.list_users();
            st.selected = None;
        }
        if ui
            .button("Fehlende IDs ergänzen")
            .on_hover_text(
                "Charakter-ID, Corp und Allianz über ESI nachschlagen – nötig für das Porträt",
            )
            .clicked()
        {
            let n = engine.lock().expect("engine").resolve_missing_ids();
            st.info = Some(if n == 0 {
                "Alle Zuordnungen haben bereits eine Charakter-ID.".into()
            } else {
                format!("Suche läuft für {n} Zuordnung(en) – die Tabelle aktualisiert sich selbst.")
            });
        }
    });
    if st.confirm_delete {
        if let Some(u) = st.selected.and_then(|i| st.rows.get(i)).cloned() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Zuordnung @{} löschen?", u.twitch_login)).color(YELLOW),
                );
                if ui.button(RichText::new("Ja, löschen").color(RED)).clicked() {
                    let mut e = engine.lock().expect("engine");
                    let _ = e.db.delete_user(&u.twitch_login);
                    e.refresh_igns();
                    st.confirm_delete = false;
                    st.selected = None;
                }
                if ui.button("Abbrechen").clicked() {
                    st.confirm_delete = false;
                }
            });
        } else {
            st.confirm_delete = false;
        }
    }
    if let Some(info) = &st.info {
        ui.label(RichText::new(info).color(GREEN).size(11.0));
    }
    ui.add_space(4.0);
    let rows = st.rows.clone();
    egui_extras::TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Column::exact(40.0))
        .column(egui_extras::Column::initial(150.0).resizable(true))
        .column(egui_extras::Column::initial(170.0).resizable(true))
        .column(egui_extras::Column::initial(100.0).resizable(true))
        .column(egui_extras::Column::initial(110.0).resizable(true))
        .column(egui_extras::Column::remainder())
        .header(22.0, |mut h| {
            for t in [
                "",
                "Twitch",
                "EVE-Charakter",
                "EVE-ID",
                "Corp / Allianz",
                "Quelle",
            ] {
                h.col(|ui| {
                    ui.label(RichText::new(t).color(CYAN).strong());
                });
            }
        })
        .body(|body| {
            body.rows(38.0, rows.len(), |mut row| {
                let i = row.index();
                let u = &rows[i];
                row.set_selected(st.selected == Some(i));
                let dash = |o: &Option<String>| {
                    o.clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "—".into())
                };
                row.col(|ui| {
                    if let Some(id) = u.eve_character_id {
                        portrait(ui, id, 34.0);
                    }
                });
                row.col(|ui| {
                    ui.label(&u.twitch_login);
                });
                row.col(|ui| {
                    ui.label(dash(&u.eve_character_name));
                });
                row.col(|ui| {
                    ui.label(
                        u.eve_character_id
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "—".into()),
                    );
                });
                row.col(|ui| {
                    ui.label(format!(
                        "{} / {}",
                        dash(&u.corp_ticker),
                        dash(&u.alliance_ticker)
                    ));
                });
                row.col(|ui| {
                    ui.label(dash(&u.source));
                });
                let r = row.response();
                if r.clicked() {
                    st.selected = Some(i);
                    st.confirm_delete = false;
                }
                if r.double_clicked() {
                    st.edit = Some(EditState::from_user(Some(u)));
                }
            });
        });
}

/// EVE-Porträt in passender Auflösung; bis zum Laden ein Platzhalter.
fn portrait(ui: &mut egui::Ui, id: i64, px: f32) {
    let size = if px <= 64.0 {
        64
    } else if px <= 128.0 {
        128
    } else {
        256
    };
    ui.add(
        egui::Image::new(crate::engine::portrait_url(id, size))
            .fit_to_exact_size(egui::vec2(px, px))
            .corner_radius(2.0)
            .show_loading_spinner(false),
    );
}

fn history_tab(ui: &mut egui::Ui, st: &mut Settings) {
    if st.history.is_empty() {
        ui.label(RichText::new("Noch keine Giveaways gespielt.").color(DIM));
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("hist")
            .striped(true)
            .num_columns(4)
            .spacing([20.0, 6.0])
            .show(ui, |ui| {
                for t in ["Zeitpunkt", "Gewinner", "Preis", "Teilnehmer"] {
                    ui.label(RichText::new(t).color(CYAN).strong());
                }
                ui.end_row();
                for h in &st.history {
                    ui.label(h.played_at.replace('T', " "));
                    ui.label(RichText::new(&h.winner).color(YELLOW));
                    ui.label(h.prize.as_deref().unwrap_or("—"));
                    ui.label(h.participants.to_string());
                    ui.end_row();
                }
            });
    });
}

fn type_icon(ui: &mut egui::Ui, type_id: i64, px: f32) {
    ui.add(
        egui::Image::new(type_icon_url(type_id, 64))
            .fit_to_exact_size(egui::vec2(px, px))
            .show_loading_spinner(false),
    );
}

fn prizes_tab(ui: &mut egui::Ui, st: &mut Settings, engine: &Arc<Mutex<Engine>>) {
    let mut changed = false;
    let (winners, connected, character, assets_status, loading, port) = {
        let e = engine.lock().expect("engine");
        if e.assets_version != st.assets_seen {
            st.assets_seen = e.assets_version;
        }
        (
            e.cfg.winners.max(1),
            e.sso_connected(),
            e.cfg.eve_sso_character_name.clone(),
            e.assets_status.clone(),
            e.assets_loading,
            e.callback_port(),
        )
    };

    // ---------------------------------------------------------- Gewinner & Beschreibung
    egui::Grid::new("prize_head")
        .num_columns(2)
        .spacing([14.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Anzahl Gewinner").color(CYAN));
            let mut n = winners;
            if ui
                .add(egui::DragValue::new(&mut n).range(1..=100))
                .changed()
            {
                engine.lock().expect("engine").set_winners(n);
            }
            ui.end_row();
            ui.label(RichText::new("Was gibt es zu gewinnen?").color(CYAN));
            let r = ui.add(
                egui::TextEdit::singleline(&mut st.prize_text)
                    .hint_text("leer = aus der Preisliste, z. B. „je 100 Mio. ISK“")
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() || (r.changed() && st.prize_text.is_empty()) {
                changed = true;
            }
            ui.end_row();
        });
    ui.label(
        RichText::new(
            "Mit Preisliste bekommt Gewinner 1 den ersten Preis, Gewinner 2 den zweiten usw. Ohne Liste gilt die Beschreibung für alle Gewinner.",
        )
        .color(DIM)
        .size(11.0),
    );
    if !st.prizes.is_empty() && st.prizes.len() != winners as usize {
        ui.label(
            RichText::new(format!(
                "Hinweis: {} Preise, aber {} Gewinner eingestellt.",
                st.prizes.len(),
                winners
            ))
            .color(YELLOW)
            .size(12.0),
        );
    }
    ui.add_space(6.0);

    // ---------------------------------------------------------- Preisliste
    ui.label(RichText::new("Preisliste").color(GREEN).strong());
    let mut remove = None;
    let mut swap = None;
    egui::ScrollArea::vertical().id_salt("prizelist").max_height(170.0).show(ui, |ui| {
        if st.prizes.is_empty() {
            ui.label(RichText::new("Noch keine Preise – unten aus den EVE-Assets wählen oder eigenen Preis eintragen.").color(DIM));
        }
        let len = st.prizes.len();
        for (i, p) in st.prizes.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:>2}.", i + 1)).color(DIM));
                match p.type_id {
                    Some(t) => type_icon(ui, t, 28.0),
                    None => {
                        ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    }
                }
                if ui.add(egui::DragValue::new(&mut p.quantity).range(1..=u64::MAX).prefix("× ")).changed() {
                    changed = true;
                }
                let r = ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(220.0));
                if r.lost_focus() {
                    changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Entfernen").clicked() {
                        remove = Some(i);
                    }
                    if ui
                        .add_enabled(i + 1 < len, egui::Button::new("▼").small())
                        .on_hover_text("Nach unten (späterer Gewinner)")
                        .clicked()
                    {
                        swap = Some((i, i + 1));
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("▲").small())
                        .on_hover_text("Nach oben (früherer Gewinner)")
                        .clicked()
                    {
                        swap = Some((i - 1, i));
                    }
                    // Lagerort im verbleibenden Platz, bei Bedarf gekürzt
                    if let Some(loc) = &p.location {
                        ui.add(egui::Label::new(RichText::new(loc).color(DIM).size(11.0)).truncate())
                            .on_hover_text(loc);
                    }
                });
            });
        }
    });
    if let Some(i) = remove {
        st.prizes.remove(i);
        changed = true;
    }
    if let Some((a, b)) = swap {
        st.prizes.swap(a, b);
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Eigener Preis").color(CYAN));
        ui.add(
            egui::DragValue::new(&mut st.manual_qty)
                .range(1..=u64::MAX)
                .prefix("× "),
        );
        ui.add(
            egui::TextEdit::singleline(&mut st.manual_name)
                .hint_text("z. B. 500 Mio. ISK")
                .desired_width(220.0),
        );
        if ui
            .add_enabled(
                !st.manual_name.trim().is_empty(),
                egui::Button::new("Hinzufügen"),
            )
            .clicked()
        {
            st.prizes.push(Prize {
                name: st.manual_name.trim().to_string(),
                quantity: st.manual_qty.max(1),
                ..Default::default()
            });
            st.manual_name.clear();
            st.manual_qty = 1;
            changed = true;
        }
    });
    ui.separator();

    // ---------------------------------------------------------- EVE-Assets
    ui.label(RichText::new("Aus deinen EVE-Assets").color(GREEN).strong());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Client-ID").color(CYAN));
        ui.add(egui::TextEdit::singleline(&mut st.client_id).desired_width(260.0))
            .on_hover_text("Client-ID deiner App auf developers.eveonline.com");
        if ui.button("Übernehmen").clicked() {
            engine
                .lock()
                .expect("engine")
                .set_sso_client_id(&st.client_id);
            st.prize_info = Some("Client-ID gespeichert.".into());
        }
        ui.hyperlink_to(
            "developers.eveonline.com",
            "https://developers.eveonline.com/applications",
        );
    });
    let callback = crate::sso::callback_url(port);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Callback-URL für die App:")
                .color(DIM)
                .size(11.0),
        );
        ui.label(RichText::new(&callback).size(11.0));
        if ui.small_button("kopieren").clicked() {
            ui.ctx().copy_text(callback.clone());
        }
    });
    let mut structures = engine.lock().expect("engine").cfg.eve_sso_structures;
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut structures, "Strukturnamen (Citadels) anzeigen")
            .on_hover_text(
                "Fragt zusätzlich den Scope esi-universe.read_structures.v1 an. Der muss dann auch in deiner EVE-App eingetragen sein; gilt ab der nächsten Anmeldung.",
            )
            .changed()
        {
            engine.lock().expect("engine").set_sso_structures(structures);
        }
        ui.label(
            RichText::new(format!("Scopes in der App: {}", crate::sso::scopes(structures)))
                .color(DIM)
                .size(11.0),
        );
    });
    ui.horizontal(|ui| {
        if connected {
            ui.label(RichText::new(format!("Verbunden als {character}")).color(GREEN));
            if ui
                .add_enabled(!loading, egui::Button::new("Assets laden"))
                .clicked()
            {
                engine.lock().expect("engine").load_assets();
            }
            if ui.button("Abmelden").clicked() {
                engine.lock().expect("engine").sso_logout();
            }
        } else if ui
            .button(RichText::new("Mit EVE verbinden").color(GREEN))
            .clicked()
        {
            let res = engine.lock().expect("engine").sso_begin();
            match res {
                Ok(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
                Err(e) => st.prize_info = Some(e),
            }
        }
        ui.label(RichText::new(&assets_status).color(if loading { YELLOW } else { DIM }));
        if loading {
            ui.spinner();
        }
    });
    if let Some(info) = &st.prize_info {
        ui.label(RichText::new(info).color(YELLOW).size(12.0));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Suche").color(CYAN));
        ui.add(
            egui::TextEdit::singleline(&mut st.asset_search)
                .hint_text("Name oder Ort")
                .desired_width(260.0),
        );
    });

    let rows: Vec<crate::sso::AssetRow> = {
        let e = engine.lock().expect("engine");
        let q = st.asset_search.trim().to_lowercase();
        e.assets
            .iter()
            .filter(|a| {
                q.is_empty()
                    || a.name.to_lowercase().contains(&q)
                    || a.location.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    };
    let in_list: std::collections::HashSet<i64> =
        st.prizes.iter().filter_map(|p| p.item_id).collect();
    let mut add: Option<crate::sso::AssetRow> = None;
    egui_extras::TableBuilder::new(ui)
        .id_salt("assets")
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Column::exact(34.0))
        .column(egui_extras::Column::initial(230.0).resizable(true))
        .column(egui_extras::Column::initial(80.0))
        .column(egui_extras::Column::remainder().clip(true))
        .column(egui_extras::Column::exact(96.0))
        .header(20.0, |mut h| {
            for t in ["", "Item", "Menge", "Ort", ""] {
                h.col(|ui| {
                    ui.label(RichText::new(t).color(CYAN).strong());
                });
            }
        })
        .body(|body| {
            body.rows(32.0, rows.len(), |mut row| {
                let a = &rows[row.index()];
                row.col(|ui| type_icon(ui, a.type_id, 28.0));
                row.col(|ui| {
                    ui.label(&a.name);
                });
                row.col(|ui| {
                    ui.label(fmt_qty(a.quantity));
                });
                row.col(|ui| {
                    ui.label(RichText::new(&a.location).color(DIM).size(11.0))
                        .on_hover_text(&a.location);
                });
                row.col(|ui| {
                    if in_list.contains(&a.item_id) {
                        ui.label(RichText::new("✓ Preis").color(GREEN));
                    } else if ui.button("+ als Preis").clicked() {
                        add = Some(a.clone());
                    }
                });
            });
        });
    if let Some(a) = add {
        st.prizes.push(Prize {
            name: a.name,
            quantity: a.quantity,
            type_id: Some(a.type_id),
            item_id: Some(a.item_id),
            location: Some(a.location),
        });
        changed = true;
    }
    if changed {
        engine
            .lock()
            .expect("engine")
            .set_prizes(st.prizes.clone(), st.prize_text.trim().to_string());
    }
}
