//! OBS-Overlay: kleiner Webserver (axum) mit WebSocket-Statusstrom.
//!
//! Routen: `/` Overlay, `/test` Demo-Modus, `/timer` Timer-Anzeige,
//! `/ws` Live-Status, `/api/demo` Demo-Run, `/dock` Bedien-Dock für OBS
//! (`/ws/dock`, `POST /api/dock`) – das Dock nur vom selben PC aus.

use crate::engine::{DockCmd, Inbound};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::{oneshot, watch};

pub const OVERLAY_HTML: &str = include_str!("../assets/overlay.html");
pub const TIMER_HTML: &str = include_str!("../assets/timer.html");
pub const DOCK_HTML: &str = include_str!("../assets/dock.html");

#[derive(Clone)]
struct AppState {
    rx: watch::Receiver<String>,
    dock_rx: watch::Receiver<String>,
    cmd: mpsc::Sender<Inbound>,
    decker: String,
}

pub struct OverlayServer {
    pub port: Option<u16>,
    pub error: Option<String>,
    tx: Option<watch::Sender<String>>,
    dock_tx: Option<watch::Sender<String>>,
    shutdown: Option<oneshot::Sender<()>>,
    done: Option<mpsc::Receiver<()>>,
}

impl OverlayServer {
    pub fn start(
        requested: u16,
        initial: String,
        initial_dock: String,
        decker: String,
        cmd: mpsc::Sender<Inbound>,
    ) -> Self {
        let (tx, rx) = watch::channel(initial);
        let (dock_tx, dock_rx) = watch::channel(initial_dock);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u16, String>>();
        let (sd_tx, sd_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("overlay-web".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                rt.block_on(async move {
                    let mut bound = None;
                    let mut last_err = String::new();
                    let end = requested.saturating_add(9);
                    for p in requested..=end {
                        match tokio::net::TcpListener::bind(("0.0.0.0", p)).await {
                            Ok(l) => {
                                bound = Some((l, p));
                                break;
                            }
                            Err(e) => last_err = e.to_string(),
                        }
                    }
                    let Some((listener, port)) = bound else {
                        let _ = ready_tx.send(Err(format!(
                            "Kein freier Port in {requested}..{end}: {last_err}"
                        )));
                        return;
                    };
                    let _ = ready_tx.send(Ok(port));
                    let app = Router::new()
                        .route("/", get(index))
                        .route("/test", get(index))
                        .route("/timer", get(timer))
                        .route("/ws", get(ws_handler))
                        .route("/api/demo", get(demo))
                        .route("/dock", get(dock_page))
                        .route("/ws/dock", get(dock_ws))
                        .route("/api/dock", axum::routing::post(dock_cmd))
                        .route("/eve/callback", get(eve_callback))
                        .with_state(AppState {
                            rx,
                            dock_rx,
                            cmd,
                            decker,
                        });
                    let _ = axum::serve(
                        listener,
                        app.into_make_service_with_connect_info::<SocketAddr>(),
                    )
                    .with_graceful_shutdown(async {
                        let _ = sd_rx.await;
                    })
                    .await;
                });
                rt.shutdown_timeout(Duration::from_millis(500));
                let _ = done_tx.send(());
            });
        let (port, error) = match spawned {
            Err(e) => (None, Some(e.to_string())),
            Ok(_) => match ready_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(p)) => (Some(p), None),
                Ok(Err(e)) => (None, Some(e)),
                Err(_) => (None, Some("Webserver startet nicht (Timeout)".into())),
            },
        };
        Self {
            port,
            error,
            tx: Some(tx),
            dock_tx: Some(dock_tx),
            shutdown: Some(sd_tx),
            done: Some(done_rx),
        }
    }

    pub fn publish(&self, json: String) {
        if let Some(tx) = &self.tx {
            tx.send_replace(json);
        }
    }

    pub fn publish_dock(&self, json: String) {
        if let Some(tx) = &self.dock_tx {
            tx.send_replace(json);
        }
    }

    /// Stoppt den Server und wartet kurz, bis der Port frei ist.
    pub fn stop(&mut self) {
        // Sender fallen lassen → alle WebSocket-Schleifen enden
        self.tx = None;
        self.dock_tx = None;
        if let Some(sd) = self.shutdown.take() {
            let _ = sd.send(());
        }
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(Duration::from_secs(3));
        }
    }
}

impl Drop for OverlayServer {
    fn drop(&mut self) {
        self.tx = None;
        self.dock_tx = None;
        if let Some(sd) = self.shutdown.take() {
            let _ = sd.send(());
        }
    }
}

async fn index() -> impl IntoResponse {
    ([("Cache-Control", "no-store")], Html(OVERLAY_HTML))
}

async fn timer() -> impl IntoResponse {
    ([("Cache-Control", "no-store")], Html(TIMER_HTML))
}

#[derive(serde::Deserialize)]
struct DemoQuery {
    winners: Option<usize>,
}

/// Demo-Run mit Fantasienamen; `?winners=3` zeigt mehrere Gewinner mit Beispielpreisen.
async fn demo(
    State(st): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<DemoQuery>,
) -> impl IntoResponse {
    use rand::seq::SliceRandom;
    let names: Vec<String> = [
        "netghost",
        "chromefox",
        "datawitch",
        "ratbyte",
        "zeroday",
        "neonsamurai",
        "glitchkid",
        "trollrigger",
        "streetsam",
        "wageslave",
        "elfdecker",
        "dwarfrigger",
        "ghostwire",
        "redsamurai",
        "smoothop",
        "jackpoint",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let n = q.winners.unwrap_or(1).clamp(1, names.len());
    let mut rng = rand::thread_rng();
    let winners: Vec<String> = names.choose_multiple(&mut rng, n).cloned().collect();
    let decker = if st.decker.trim().is_empty() {
        "DEMO-DECKER".to_string()
    } else {
        st.decker.clone()
    };
    let frames = crate::matrix::generate(
        &mut rng,
        crate::matrix::RunSetup {
            participants: &names,
            winners: &winners,
            decker: &decker,
            duration: 30.0,
        },
    );
    // Beispielpreise mit echten EVE-Typ-IDs (für die Item-Icons)
    let samples = [
        ("Gila", 1u64, 17715i64),
        ("Ishtar", 1, 12005),
        ("PLEX", 500, 44992),
        ("Nanite Repair Paste", 1500, 28668),
        ("Rifter", 3, 587),
    ];
    let prizes: Vec<crate::prize::Prize> = samples
        .iter()
        .cycle()
        .take(n)
        .map(|(name, q, t)| crate::prize::Prize {
            name: (*name).into(),
            quantity: *q,
            type_id: Some(*t),
            ..Default::default()
        })
        .collect();
    let list: Vec<serde_json::Value> = winners
        .iter()
        .enumerate()
        .map(|(i, w)| {
            serde_json::json!({
                "login": w,
                "eve": null,
                "prize": crate::prize::for_winner(&prizes, "", i),
            })
        })
        .collect();
    Json(serde_json::json!({
        "participants": names,
        "frames": frames,
        "winners": list,
        "prize": crate::prize::summary(&prizes, ""),
    }))
}

// ------------------------------------------------------------ Dock

const FORBIDDEN: &str =
    "Das Bedien-Dock ist nur auf dem PC erreichbar, auf dem das Giveaway-Tool läuft.";

/// Fremde Webseiten dürfen keine Befehle schicken: Origin muss lokal sein.
fn local_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = origin.split("://").nth(1).unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

async fn dock_page(ConnectInfo(addr): ConnectInfo<SocketAddr>) -> Response {
    if !addr.ip().is_loopback() {
        return (StatusCode::FORBIDDEN, FORBIDDEN).into_response();
    }
    ([("Cache-Control", "no-store")], Html(DOCK_HTML)).into_response()
}

async fn dock_ws(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(st): State<AppState>,
) -> Response {
    if !addr.ip().is_loopback() || !local_origin(&headers) {
        return (StatusCode::FORBIDDEN, FORBIDDEN).into_response();
    }
    ws.on_upgrade(move |socket| client(socket, st.dock_rx))
        .into_response()
}

async fn dock_cmd(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(st): State<AppState>,
    Json(cmd): Json<DockCmd>,
) -> Response {
    if !addr.ip().is_loopback() || !local_origin(&headers) {
        return (StatusCode::FORBIDDEN, FORBIDDEN).into_response();
    }
    match st.cmd.send(Inbound::Dock(cmd)) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "Giveaway-Tool beendet").into_response(),
    }
}

#[derive(serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn callback_page(title: &str, text: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html><html lang="de"><meta charset="utf-8"><title>{title}</title>
<body style="font:16px/1.5 system-ui,sans-serif;background:#14112e;color:#e8e3ff;display:grid;place-items:center;height:100vh;margin:0">
<div style="max-width:460px;padding:24px;border:1px solid #3a3170;border-left:3px solid #2ee6c5;background:#1b1740">
<h1 style="font-size:20px;margin:0 0 8px">{title}</h1><p style="margin:0;color:#9a93c9">{text}</p></div></body></html>"#
    ))
}

/// Rückkehr von der EVE-Anmeldung (nur vom selben PC).
async fn eve_callback(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(st): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<CallbackQuery>,
) -> Response {
    if !addr.ip().is_loopback() {
        return (StatusCode::FORBIDDEN, FORBIDDEN).into_response();
    }
    if let Some(err) = q.error {
        if err == "invalid_scope" {
            let _ = st.cmd.send(Inbound::Log(
                "EVE-Anmeldung: Scope fehlt in der EVE-App (siehe Browserfenster).".into(),
                crate::engine::Tone::Red,
            ));
            return callback_page(
                "Scope fehlt in deiner EVE-App",
                "Das Tool hat einen Scope angefragt, der in deiner App auf developers.eveonline.com nicht eingetragen ist. \
                 Entweder dort <b>esi-universe.read_structures.v1</b> ergänzen oder im Tool unter \
                 Einstellungen → Preise den Haken „Strukturnamen“ entfernen. Danach erneut „Mit EVE verbinden“.",
            )
            .into_response();
        }
        let detail = q.error_description.unwrap_or(err);
        return callback_page(
            "Anmeldung abgebrochen",
            &format!("EVE meldet: {}", html_escape(&detail)),
        )
        .into_response();
    }
    match (q.code, q.state) {
        (Some(code), Some(state)) => {
            let _ = st.cmd.send(Inbound::SsoCode { code, state });
            callback_page(
                "Anmeldung empfangen",
                "Das Giveaway-Tool lädt jetzt deine Assets. Du kannst dieses Fenster schließen.",
            )
            .into_response()
        }
        _ => callback_page(
            "Anmeldung unvollständig",
            "Es kam kein Code von EVE zurück. Bitte im Tool erneut verbinden.",
        )
        .into_response(),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn ws_handler(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client(socket, st.rx))
}

async fn client(mut socket: WebSocket, mut rx: watch::Receiver<String>) {
    let first = rx.borrow_and_update().clone();
    if socket.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() { break; }
                let msg = rx.borrow_and_update().clone();
                if socket.send(Message::Text(msg.into())).await.is_err() { break; }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    _ => {}
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
            }
        }
    }
}
