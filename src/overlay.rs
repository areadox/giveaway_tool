//! OBS-Overlay: kleiner Webserver (axum) mit WebSocket-Statusstrom.
//!
//! Routen: `/` Overlay, `/test` Demo-Modus, `/ws` Live-Status, `/api/demo` Demo-Run.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::{oneshot, watch};

pub const OVERLAY_HTML: &str = include_str!("../assets/overlay.html");

#[derive(Clone)]
struct AppState {
    rx: watch::Receiver<String>,
    decker: String,
}

pub struct OverlayServer {
    pub port: Option<u16>,
    pub error: Option<String>,
    tx: Option<watch::Sender<String>>,
    shutdown: Option<oneshot::Sender<()>>,
    done: Option<mpsc::Receiver<()>>,
}

impl OverlayServer {
    pub fn start(requested: u16, initial: String, decker: String) -> Self {
        let (tx, rx) = watch::channel(initial);
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
                        .route("/ws", get(ws_handler))
                        .route("/api/demo", get(demo))
                        .with_state(AppState { rx, decker });
                    let _ = axum::serve(listener, app)
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
            shutdown: Some(sd_tx),
            done: Some(done_rx),
        }
    }

    pub fn publish(&self, json: String) {
        if let Some(tx) = &self.tx {
            tx.send_replace(json);
        }
    }

    /// Stoppt den Server und wartet kurz, bis der Port frei ist.
    pub fn stop(&mut self) {
        // Sender fallen lassen → alle WebSocket-Schleifen enden
        self.tx = None;
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
        if let Some(sd) = self.shutdown.take() {
            let _ = sd.send(());
        }
    }
}

async fn index() -> impl IntoResponse {
    ([("Cache-Control", "no-store")], Html(OVERLAY_HTML))
}

async fn demo(State(st): State<AppState>) -> impl IntoResponse {
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let winner = names[rand::random::<usize>() % names.len()].clone();
    let decker = if st.decker.trim().is_empty() {
        "DEMO-DECKER".to_string()
    } else {
        st.decker.clone()
    };
    let mut rng = rand::thread_rng();
    let frames = crate::matrix::generate(
        &mut rng,
        crate::matrix::RunSetup {
            participants: &names,
            winner: &winner,
            decker: &decker,
            duration: 30.0,
        },
    );
    Json(serde_json::json!({ "participants": names, "frames": frames, "winner": winner }))
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
