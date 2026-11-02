# Cyber Giveaway – Matrix-Run

Twitch-Giveaway-Tool für **Areadox Gaming** mit OBS-Overlay. Zuschauer treten per Chat-Befehl bei. Die Auslosung wird als Matrix-Run im Stil von *Shadowrun* (Pen & Paper) inszeniert: Ein Decker hackt sich in einen Host, setzt Marken, kämpft gegen IC, würfelt sichtbar mit W6-Pools und zieht am Ende die Gewinnerdatei heraus.

Rust-Neuimplementierung der früheren Python-Version (`CyberGiveaway.exe`). `config.json` und `giveaway.db` der alten Version können direkt weiterverwendet werden.

Aktuelle Version: **0.0.1**

## Funktionen

- **Teilnahme per Chat:** Keyword (z. B. `!hack`) während der offenen Phase. Bots (StreamElements, Moobot, Nightbot) werden ignoriert, doppelte Teilnahmen zählen nicht.
- **Countdown:** als `mm:ss` oder in Minuten (`0` = manuelles Ende), mit Chat-Warnungen bei 60 und 10 Sekunden. Läuft er ab, startet der Run automatisch.
- **Matrix-Run (Shadowrun-Stil):**
  - W6-Würfelpools, Erfolg bei 5 oder 6, Patzer und kritische Patzer, Limits
  - Matrixwahrnehmung, Eiliges Hacken oder Brute Force für 3 Marken
  - IC-Kampfrunden (Patrouillen-IC, Killer-IC, Schwarzes IC, Blaster, Teerbaby, Säure, Aufspür-IC, Absturz-IC, selten eine Spinne) mit Datenspikes
  - Overwatch-Wert bis zur GOD-Konvergenz bei 40, Matrix- und geistiger Zustandsmonitor, Edge (Sechser explodieren), Datenbomben, Link-Sperre beim Ausstöpseln
  - Heiß-Sim oder Kalt-Sim, zufälliger Host mit Stufe und ASDF-Attributen
  - Teilnehmerregister wird dekodiert, bis nur der Gewinner übrig ist
- **Fairness:** Der Gewinner wird vor dem Run gleichverteilt per Zufall gezogen. Der Run ist nur Inszenierung und endet immer erfolgreich.
- **OBS-Overlay** (Browserquelle, transparent):
  - `http://localhost:8766` live
  - `http://localhost:8766/test` Demo-Schleife zum Einrichten
  - `?view=graph` nur Host und Würfel, `?view=log` nur Charakterbogen und Protokoll, `?hide=1` blendet den Verbindungshinweis aus
- **Chat-Bot:** Start-, End- und Gewinnernachricht über einen Bot-Account (OAuth-Token).
- **EVE-Online-Modus (optional):**
  - `!eve Charaktername` verknüpft einen Twitch-Account mit einem EVE-Charakter (Prüfung über ESI)
  - Import über eve2twitch-API und Eve2Twitch-Bot-Nachrichten
  - EVE-Namen erscheinen nie im Overlay, sondern als `[EVE-NAME]`
- **Datenbank:** Zuordnungen anlegen, bearbeiten, löschen und als CSV exportieren; Verlauf aller Gewinner.

## Installation (Windows)

1. Unter [Releases](https://github.com/areadox/giveaway_tool/releases) `CyberGiveaway-windows-x64.zip` herunterladen und in einen eigenen Ordner entpacken.
2. Optional `config.json` und `giveaway.db` der alten Version in denselben Ordner kopieren.
3. `CyberGiveaway.exe` starten. Beim ersten Start öffnen sich die Einstellungen.
4. In OBS eine **Browserquelle** mit `http://localhost:8766` anlegen (1920 × 1080).

Konfiguration und Datenbank liegen immer neben der `.exe`.

## Bedienung

| Knopf | Wirkung |
|---|---|
| **START** | öffnet die Teilnahme und startet den Countdown |
| **STOP & AUSLOSEN** | schließt die Teilnahme und startet den Matrix-Run |
| **BREACH** | startet den Run sofort mit dem aktuellen Pool |
| **RESET** | leert Pool, Protokoll und Overlay |

Das Feld **RUN (s)** legt fest, wie lange der Run im Overlay dauert (mindestens 8 Sekunden, Standard 30).

## Konfiguration (`config.json`)

| Schlüssel | Bedeutung |
|---|---|
| `channel` | Twitch-Kanal, dessen Chat gelesen wird (anonym) |
| `bot_username`, `oauth_token` | Account, der Chatnachrichten sendet |
| `keyword` | Teilnahmebefehl |
| `web_port` | Port des Overlays; ist er belegt, werden die nächsten 9 Ports versucht |
| `timer_duration` | Standard-Giveaway-Dauer |
| `hack_duration` | Dauer des Matrix-Runs in Sekunden *(neu)* |
| `decker_name` | Name des Deckers im Overlay, leer = Kanalname *(neu)* |
| `open_message`, `close_message`, `win_message` | Chattexte (`{keyword}`, `{winner}`) |
| `sound` | Soundeffekte im Programm |
| `eve_enabled`, `eve_command`, `eve_api_url`, `eve_api_key`, `eve2twitch_bot_name` | EVE-Modus |

> Das OAuth-Token steht im Klartext in `config.json`. Die Datei deshalb nicht weitergeben oder ins Repository einchecken (sie steht in `.gitignore`).

## Bauen

```sh
cargo build --release      # Ergebnis: target/release/CyberGiveaway(.exe)
cargo test
```

GitHub Actions baut bei jedem Push eine Windows- und eine Linux-Version. Ein Tag `v*` erzeugt ein Release mit beiden Paketen.

Ohne Fenster nur den Overlay-Server starten (zum Testen): `CyberGiveaway --headless`

## Aufbau

| Datei | Inhalt |
|---|---|
| `src/matrix.rs` | Generator für den Matrix-Run (Würfelregeln, Phasen, IC) |
| `src/engine.rs` | Spielablauf, Countdown, Chat-Verarbeitung, Overlay-Status |
| `src/overlay.rs` + `assets/overlay.html` | Webserver (axum/WebSocket) und OBS-Overlay |
| `src/gui.rs` | Programmfenster (egui) |
| `src/twitch.rs` | IRC-Leser (anonym) und Bot-Sender |
| `src/esi.rs` | EVE-ESI und eve2twitch |
| `src/db.rs`, `src/config.rs`, `src/sound.rs` | Datenbank, Konfiguration, Soundeffekte |

*Shadowrun* ist eine Marke der Topps Company, Inc. Dieses Tool ist ein inoffizielles Fanprojekt und nutzt nur allgemeine Spielbegriffe; Hosts, Decks und Texte sind frei erfunden.

## Versionen

| Version | Änderungen |
|---|---|
| 0.0.1 | Erste Rust-Version: Matrix-Run im Shadowrun-Stil, neues Overlay, Verlauf-Tab, einstellbare Run-Dauer und Decker-Name |
