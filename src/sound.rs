//! Kurze, synthetisch erzeugte Soundeffekte.
//!
//! Ausgabe wahlweise über das OBS-Overlay (Browserquelle spielt sie per
//! WebAudio ab – Lautstärke dann im OBS-Mixer) oder über das System
//! (Windows: PlaySound aus dem Speicher).

use std::sync::OnceLock;
use tokio::sync::broadcast;

/// Soundereignisse für das Overlay (`/ws`)
pub fn obs_events() -> &'static broadcast::Sender<String> {
    static TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    TX.get_or_init(|| broadcast::channel(64).0)
}

impl Sfx {
    pub fn name(self) -> &'static str {
        match self {
            Sfx::Click => "click",
            Sfx::Join => "join",
            Sfx::Error => "error",
            Sfx::Dice => "dice",
            Sfx::Ice => "ice",
            Sfx::Glitch => "glitch",
            Sfx::Win1 => "win1",
            Sfx::Win2 => "win2",
        }
    }

    /// Nur Rückmeldung für die Bedienung (Klick, Fehler) – nicht in den Stream
    pub fn ui_only(self) -> bool {
        matches!(self, Sfx::Click | Sfx::Error)
    }
}

/// Sound ausgeben: `output` ist "obs" oder "system".
pub fn emit(s: Sfx, enabled: bool, output: &str) {
    if !enabled {
        return;
    }
    if output == "system" {
        play(s, true);
    } else if !s.ui_only() {
        let _ = obs_events().send(format!(r#"{{"kind":"sfx","sfx":"{}"}}"#, s.name()));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    Click,
    Join,
    Error,
    Dice,
    Ice,
    Glitch,
    Win1,
    Win2,
}

/// (Frequenzfolge in Hz, Dauer je Ton in s, Lautstärke)
#[cfg_attr(not(windows), allow(dead_code))]
fn spec(s: Sfx) -> (&'static [f32], f32, f32) {
    match s {
        Sfx::Click => (&[880.0], 0.045, 0.22),
        Sfx::Join => (&[660.0], 0.11, 0.22),
        Sfx::Error => (&[190.0], 0.18, 0.25),
        Sfx::Dice => (&[1400.0, 900.0, 1700.0, 1100.0], 0.022, 0.12),
        Sfx::Ice => (&[220.0, 180.0, 150.0], 0.07, 0.25),
        Sfx::Glitch => (&[150.0, 1900.0, 120.0], 0.05, 0.2),
        Sfx::Win1 => (&[660.0], 0.18, 0.22),
        Sfx::Win2 => (&[990.0], 0.24, 0.22),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn wav(s: Sfx) -> Vec<u8> {
    let (freqs, dur, vol) = spec(s);
    let rate = 22050u32;
    let mut frames: Vec<i16> = Vec::new();
    for &f in freqs {
        let count = ((rate as f32 * dur) as usize).max(1);
        let attack = (rate as f32 * 0.008).max(1.0);
        let release = (rate as f32 * 0.025).max(1.0);
        for i in 0..count {
            let env = (i as f32 / attack)
                .min(1.0)
                .min((count - i) as f32 / release);
            let v = (32767.0
                * vol
                * env
                * (2.0 * std::f32::consts::PI * f * i as f32 / rate as f32).sin())
                as i16;
            frames.push(v);
        }
    }
    let data_len = (frames.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for v in frames {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(windows)]
pub fn play(s: Sfx, enabled: bool) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    if !enabled {
        return;
    }
    // Die Puffer müssen während der asynchronen Wiedergabe gültig bleiben → statisch.
    static CACHE: OnceLock<Vec<(Sfx, Vec<u8>)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        [
            Sfx::Click,
            Sfx::Join,
            Sfx::Error,
            Sfx::Dice,
            Sfx::Ice,
            Sfx::Glitch,
            Sfx::Win1,
            Sfx::Win2,
        ]
        .into_iter()
        .map(|x| (x, wav(x)))
        .collect()
    });
    if let Some((_, data)) = cache.iter().find(|(x, _)| *x == s) {
        unsafe {
            PlaySoundW(
                data.as_ptr() as *const u16,
                std::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn play(_s: Sfx, _enabled: bool) {}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header() {
        let w = super::wav(super::Sfx::Dice);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize,
            w.len() - 44
        );
    }
}
