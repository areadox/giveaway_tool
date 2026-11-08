//! Cyber Giveaway – Twitch-Giveaway-Tool mit OBS-Overlay und simuliertem
//! Matrix-Run im Stil von Shadowrun.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod db;
mod engine;
mod esi;
mod gui;
mod matrix;
mod overlay;
mod prize;
mod sound;
mod sso;
mod themes;
mod tray;
mod twitch;

use std::sync::{Arc, Mutex};

fn fatal(msg: &str) -> ! {
    rfd::MessageDialog::new()
        .set_title("Cyber Giveaway")
        .set_description(msg)
        .set_level(rfd::MessageLevel::Error)
        .show();
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let cfg_path = config::config_path();
    let cfg = config::load(&cfg_path);
    if !cfg_path.exists() {
        let _ = config::save(&cfg_path, &cfg);
    }
    let db = match db::Db::open(&config::db_path()) {
        Ok(db) => db,
        Err(e) => fatal(&format!("Datenbank konnte nicht geöffnet werden: {e}")),
    };

    // Ohne Fenster: nur Overlay-Server für Tests (`CyberGiveaway --headless`)
    if std::env::args().any(|a| a == "--headless") {
        let engine = Arc::new(Mutex::new(engine::Engine::new(cfg, db)));
        engine::spawn(engine.clone());
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    let engine = Arc::new(Mutex::new(engine::Engine::new(cfg, db)));
    engine::spawn(engine.clone());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("MATRIX-RUN // Twitch Giveaway")
            .with_inner_size([600.0, 620.0])
            .with_min_inner_size([520.0, 480.0])
            .with_icon(std::sync::Arc::new(eframe::egui::IconData {
                rgba: tray::icon_rgba(64),
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };
    let e2 = engine.clone();
    if let Err(e) = eframe::run_native(
        "CyberGiveaway",
        options,
        Box::new(move |cc| Ok(Box::new(gui::GuiApp::new(cc, e2)))),
    ) {
        fatal(&format!("Fenster konnte nicht gestartet werden: {e}"));
    }
    if let Ok(mut e) = engine.lock() {
        e.shutdown();
    };
}
