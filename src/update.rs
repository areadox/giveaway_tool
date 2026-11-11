//! Update über GitHub-Releases: neueste Version abfragen, passendes Programm
//! herunterladen, die laufende Datei ersetzen und neu starten.
//!
//! Die alte Datei wird nur umbenannt (`*.old`, unter Windows ist eine laufende
//! .exe nicht löschbar) und beim nächsten Start aufgeräumt.

use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "areadox/giveaway_tool";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Release {
    pub version: String,
    /// Seite des Releases auf GitHub
    pub url: String,
    /// Download des passenden Programms (fehlt, wenn es für dieses System keins gibt)
    pub asset: Option<String>,
    pub notes: String,
}

pub fn asset_name() -> &'static str {
    if cfg!(windows) {
        "CyberGiveaway-windows-x64.exe"
    } else {
        "CyberGiveaway-linux-x64"
    }
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(['.', '-', '+']).map(|p| p.parse::<u32>());
    Some((
        it.next()?.ok()?,
        it.next()?.ok()?,
        it.next().unwrap_or(Ok(0)).ok()?,
    ))
}

/// Ist `latest` neuer als `current`?
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("CyberGiveaway/{VERSION}"))
        .build()
}

/// Liest ein Release-JSON der GitHub-API.
pub fn release_from_json(v: &Value) -> Option<Release> {
    let tag = v.get("tag_name")?.as_str()?;
    let asset = v
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a.get("name").and_then(Value::as_str) == Some(asset_name()))
        .and_then(|a| a.get("browser_download_url").and_then(Value::as_str))
        .map(str::to_string);
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        url: v
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        asset,
        notes: v
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(4000)
            .collect(),
    })
}

/// Fragt das neueste Release ab; `Ok(None)` = schon aktuell.
pub fn check() -> Result<Option<Release>, String> {
    let v: Value = agent()
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(c, _) => t!("GitHub antwortet mit HTTP {c}", c = c),
            other => t!("GitHub nicht erreichbar: {other}", other = other),
        })?
        .into_json()
        .map_err(|e| e.to_string())?;
    let r = release_from_json(&v).ok_or(t!("Antwort von GitHub unverständlich"))?;
    Ok(is_newer(&r.version, VERSION).then_some(r))
}

fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

fn new_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".new");
    PathBuf::from(s)
}

/// Prüft, ob die Datei ein Programm für dieses System ist.
pub fn looks_like_program(bytes: &[u8]) -> bool {
    if bytes.len() < 1_000_000 {
        return false;
    }
    if cfg!(windows) {
        bytes.starts_with(b"MZ")
    } else {
        bytes.starts_with(b"\x7fELF")
    }
}

/// Lädt das Release herunter und tauscht die Programmdatei aus.
/// Gibt den Pfad des neuen Programms zurück (zum Neustart).
pub fn install(r: &Release) -> Result<PathBuf, String> {
    let url = r
        .asset
        .as_ref()
        .ok_or_else(|| t!("Im Release v{a1} gibt es kein {a2}", a1 = r.version, a2 = asset_name()))?;
    let exe = std::env::current_exe().map_err(|e| t!("Programmpfad unbekannt: {e}", e = e))?;
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| t!("Download fehlgeschlagen: {e}", e = e))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(300 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| t!("Download abgebrochen: {e}", e = e))?;
    if !looks_like_program(&bytes) {
        return Err(
            t!("Die heruntergeladene Datei ist kein gültiges Programm – Update abgebrochen").into(),
        );
    }
    replace(&exe, &bytes)?;
    Ok(exe)
}

/// Ersetzt `exe` durch `bytes`; bei Fehlern bleibt die alte Datei erhalten.
pub fn replace(exe: &Path, bytes: &[u8]) -> Result<(), String> {
    let new = new_path(exe);
    let old = old_path(exe);
    std::fs::write(&new, bytes)
        .map_err(|e| t!("Neue Version nicht speicherbar ({a1}): {e}", a1 = new.display(), e = e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        t!("Programmdatei nicht austauschbar: {e}", e = e)
    })?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(t!("Neue Version nicht einsetzbar: {e}", e = e));
    }
    Ok(())
}

/// Reste eines früheren Updates entfernen (beim Start aufrufen).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(new_path(&exe));
    }
}

/// Neues Programm starten; der Aufrufer beendet danach den eigenen Prozess.
pub fn spawn(exe: &Path) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| t!("Neustart fehlgeschlagen: {e}", e = e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.0.13", "0.0.12"));
        assert!(is_newer("0.1.0", "0.0.99"));
        assert!(!is_newer("v0.0.12", "0.0.12"));
        assert!(!is_newer("0.0.9", "0.0.12"));
        assert!(!is_newer("kaputt", "0.0.1"));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
    }

    #[test]
    fn release_json() {
        let v = serde_json::json!({"tag_name":"v0.0.20","html_url":"https://github.com/x","body":"Neu",
            "assets":[{"name":"CyberGiveaway-windows-x64.exe","browser_download_url":"https://dl/win"},
                      {"name":"CyberGiveaway-linux-x64","browser_download_url":"https://dl/linux"}]});
        let r = release_from_json(&v).unwrap();
        assert_eq!(r.version, "0.0.20");
        assert_eq!(
            r.asset.as_deref(),
            Some(if cfg!(windows) {
                "https://dl/win"
            } else {
                "https://dl/linux"
            })
        );
    }

    #[test]
    fn replace_file() {
        let dir = std::env::temp_dir().join(format!("cg-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("prog");
        std::fs::write(&exe, b"alt").unwrap();
        replace(&exe, b"neu").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"neu");
        assert_eq!(std::fs::read(old_path(&exe)).unwrap(), b"alt");
        assert!(!looks_like_program(b"MZ"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
