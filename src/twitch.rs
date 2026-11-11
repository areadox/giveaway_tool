//! Twitch-IRC: anonymer Leser (justinfan) und Bot-Sender (OAuth).

use crate::engine::{Inbound, Tone};
use rand::Rng;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;

fn connect() -> std::io::Result<TcpStream> {
    let addr = (IRC_HOST, IRC_PORT)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(ErrorKind::NotFound, t!("IRC-Host nicht auflösbar")))?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    s.set_read_timeout(Some(Duration::from_secs(1)))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    Ok(s)
}

/// Zerlegt eine IRC-Zeile in (Benutzer, Nachricht), falls PRIVMSG.
pub fn parse_privmsg(line: &str) -> Option<(String, String)> {
    let line = line
        .strip_prefix('@')
        .map_or(line, |rest| rest.split_once(' ').map_or("", |(_, r)| r));
    let (prefix, rest) = line.split_once(" PRIVMSG #")?;
    let user = prefix
        .trim_start_matches(':')
        .split('!')
        .next()?
        .to_lowercase();
    let (_, msg) = rest.split_once(" :")?;
    if user.is_empty() {
        return None;
    }
    Some((user, msg.trim().to_string()))
}

pub struct TwitchReader {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TwitchReader {
    pub fn start(channel: String, configured: bool, tx: Sender<Inbound>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let handle = std::thread::Builder::new()
            .name("twitch-reader".into())
            .spawn(move || {
                if !configured {
                    let _ = tx.send(Inbound::Log(
                        t!("CHANNEL konfigurieren, um Twitch-IRC zu verbinden.").into(),
                        Tone::Yellow,
                    ));
                    return;
                }
                while !st.load(Ordering::Relaxed) {
                    if let Err(e) = Self::session(&channel, &tx, &st) {
                        if st.load(Ordering::Relaxed) {
                            break;
                        }
                        let _ = tx.send(Inbound::Log(
                            t!("IRC getrennt: {e}. Neuer Versuch in 5 s.", e = e),
                            Tone::Red,
                        ));
                        for _ in 0..50 {
                            if st.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            })
            .ok();
        Self { stop, handle }
    }

    fn session(channel: &str, tx: &Sender<Inbound>, stop: &AtomicBool) -> std::io::Result<()> {
        let nick = format!("justinfan{}", rand::thread_rng().gen_range(10000..999999));
        let mut sock = connect()?;
        sock.write_all(t!("NICK {nick}\r\nJOIN #{channel}\r\n", nick = nick, channel = channel).as_bytes())?;
        let _ = tx.send(Inbound::Log(
            t!("Anonym mit #{channel} verbunden ({nick}).", channel = channel, nick = nick),
            Tone::Cyan,
        ));
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            match sock.read(&mut buf) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        ErrorKind::ConnectionAborted,
                        t!("IRC-Verbindung geschlossen"),
                    ))
                }
                Ok(n) => pending.extend_from_slice(&buf[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue
                }
                Err(e) => return Err(e),
            }
            while let Some(pos) = pending.windows(2).position(|w| w == b"\r\n") {
                let raw: Vec<u8> = pending.drain(..pos + 2).collect();
                let line = String::from_utf8_lossy(&raw[..raw.len() - 2]).to_string();
                if let Some(rest) = line.strip_prefix("PING ") {
                    sock.write_all(format!("PONG {rest}\r\n", rest = rest).as_bytes())?;
                    continue;
                }
                if let Some((user, msg)) = parse_privmsg(&line) {
                    let _ = tx.send(Inbound::Chat { user, msg });
                }
            }
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        // Nicht joinen: ein laufender Verbindungsaufbau würde sonst die GUI blockieren.
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take();
    }
}

impl Drop for TwitchReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sendet Chatnachrichten über den Bot-Account. Verbindet bei Bedarf.
pub struct TwitchSender {
    items: Option<Sender<String>>,
    handle: Option<JoinHandle<()>>,
    enabled: bool,
}

impl TwitchSender {
    pub fn start(
        channel: String,
        bot: String,
        token: String,
        configured: bool,
        log: Sender<Inbound>,
    ) -> Self {
        let enabled = configured && !token.trim().is_empty() && !bot.trim().is_empty();
        let (tx, rx) = mpsc::channel::<String>();
        let handle = if enabled {
            std::thread::Builder::new()
                .name("twitch-sender".into())
                .spawn(move || Self::run(rx, channel, bot, token, log))
                .ok()
        } else {
            None
        };
        Self {
            items: Some(tx),
            handle,
            enabled,
        }
    }

    pub fn send(&self, text: impl Into<String>) {
        if self.enabled {
            if let Some(tx) = &self.items {
                let _ = tx.send(text.into());
            }
        }
    }

    fn login(channel: &str, bot: &str, token: &str) -> std::io::Result<TcpStream> {
        let mut sock = connect()?;
        let token = token.trim();
        let token = if token.starts_with("oauth:") {
            token.to_string()
        } else {
            format!("oauth:{token}")
        };
        sock.write_all(
            t!("PASS {token}\r\nNICK {a1}\r\nJOIN #{channel}\r\n", token = token, a1 = bot.trim().to_lowercase(), channel = channel)
            .as_bytes(),
        )?;
        sock.set_read_timeout(Some(Duration::from_millis(50)))?;
        Ok(sock)
    }

    /// Liest anstehende Daten (PING beantworten, Login-Fehler erkennen).
    fn drain(sock: &mut TcpStream, log: &Sender<Inbound>) -> std::io::Result<()> {
        let mut buf = [0u8; 2048];
        loop {
            match sock.read(&mut buf) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        ErrorKind::ConnectionAborted,
                        "geschlossen",
                    ))
                }
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    for line in text.split("\r\n") {
                        if let Some(rest) = line.strip_prefix("PING ") {
                            sock.write_all(format!("PONG {rest}\r\n", rest = rest).as_bytes())?;
                        } else if line.contains("Login authentication failed")
                            || line.contains("Improperly formatted auth")
                        {
                            let _ = log.send(Inbound::Log(
                                t!("Bot-Login fehlgeschlagen: OAuth-Token prüfen.").into(),
                                Tone::Red,
                            ));
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(())
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn run(
        rx: Receiver<String>,
        channel: String,
        bot: String,
        token: String,
        log: Sender<Inbound>,
    ) {
        let mut sock: Option<TcpStream> = None;
        loop {
            let text = match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(t) => t,
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(s) = sock.as_mut() {
                        if Self::drain(s, &log).is_err() {
                            sock = None;
                        }
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let safe: String = text.replace(['\r', '\n'], " ").chars().take(450).collect();
            // Einmal mit bestehender Verbindung, bei Fehler einmal neu verbinden.
            for attempt in 0..2 {
                if sock.is_none() {
                    match Self::login(&channel, &bot, &token) {
                        Ok(s) => sock = Some(s),
                        Err(e) => {
                            let _ = log.send(Inbound::Log(
                                t!("Chat-Senden fehlgeschlagen: {e}", e = e),
                                Tone::Red,
                            ));
                            break;
                        }
                    }
                }
                let s = sock.as_mut().expect("socket");
                let ok = Self::drain(s, &log).is_ok()
                    && s.write_all(format!("PRIVMSG #{channel} :{safe}\r\n", channel = channel, safe = safe).as_bytes())
                        .is_ok();
                if ok {
                    break;
                }
                sock = None;
                if attempt == 1 {
                    let _ = log.send(Inbound::Log(
                        t!("Chat-Nachricht konnte nicht gesendet werden.").into(),
                        Tone::Red,
                    ));
                }
            }
            // Twitch-Ratelimit (20 Nachrichten / 30 s) großzügig einhalten
            std::thread::sleep(Duration::from_millis(350));
        }
    }

    pub fn stop(&mut self) {
        self.items = None;
        self.handle.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privmsg() {
        let l = ":areadox!areadox@areadox.tmi.twitch.tv PRIVMSG #areadox :!hack ";
        assert_eq!(parse_privmsg(l), Some(("areadox".into(), "!hack".into())));
        let tagged =
            "@badge-info=;color=#FF0000 :Foo!foo@foo.tmi.twitch.tv PRIVMSG #areadox :hallo welt";
        assert_eq!(
            parse_privmsg(tagged),
            Some(("foo".into(), "hallo welt".into()))
        );
        assert_eq!(
            parse_privmsg(":tmi.twitch.tv 001 justinfan1 :Welcome"),
            None
        );
    }
}
