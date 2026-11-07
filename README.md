# Cyber Giveaway – Matrix-Run

Twitch-Giveaway-Tool für **Areadox Gaming** mit OBS-Overlay. Zuschauer treten per Chat-Befehl bei. Die Auslosung wird als Matrix-Run im Stil von *Shadowrun* (Pen & Paper) inszeniert: Ein Decker hackt sich in einen Host, setzt Marken, kämpft gegen IC, würfelt sichtbar mit W6-Pools und zieht am Ende die Gewinnerdatei heraus.

Rust-Neuimplementierung der früheren Python-Version (`CyberGiveaway.exe`). `config.json` und `giveaway.db` der alten Version können direkt weiterverwendet werden.

Aktuelle Version: **0.0.4**

## Funktionen

- **Teilnahme per Chat:** Keyword (z. B. `!hack`) während der offenen Phase. Bots (StreamElements, Moobot, Nightbot) werden ignoriert, doppelte Teilnahmen zählen nicht.
- **Countdown:** als `mm:ss` oder in Minuten (`0` = manuelles Ende), mit Chat-Warnungen bei 60 und 10 Sekunden. Läuft er ab, startet der Run automatisch.
- **Auto-Giveaway (abschaltbar):** Nach einer einstellbaren Pause öffnet das Tool selbst ein Giveaway. Ist es abgeschlossen, bleibt der Gewinner eine Weile stehen, dann beginnt der Countdown von vorn. Gibt es keine Teilnehmer, geht der Kreislauf einfach weiter. Ein manueller START zwischendurch ist jederzeit möglich.
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
  - `http://localhost:8766/dock` Bedien-Dock für OBS (siehe unten)
  - `http://localhost:8766/timer` kleine Timer-Anzeige (z. B. 520 × 110): Countdown bis zum nächsten Giveaway, offene Teilnahme mit Keyword, laufender Run, Gewinner; Vorschau mit `/timer?demo=1`. Bei ausgeschaltetem Auto-Giveaway ist sie nur während eines Giveaways sichtbar.
  - `?view=graph` nur Host und Würfel, `?view=log` nur Charakterbogen und Protokoll, `?hide=1` blendet den Verbindungshinweis aus
- **Chat-Bot:** Start-, End- und Gewinnernachricht über einen Bot-Account (OAuth-Token).
- **EVE-Online-Modus (optional):**
  - `!eve Charaktername` verknüpft einen Twitch-Account mit einem EVE-Charakter (Prüfung über ESI)
  - Import über eve2twitch-API und Eve2Twitch-Bot-Nachrichten
  - Gewinnt ein registrierter Spieler, zeigt das Overlay sein EVE-Porträt mit Charaktername sowie Corp- und Allianz-Ticker; das Programmfenster zeigt das Porträt beim Gewinner und in der Datenbank
  - Fehlende Charakter-IDs (z. B. aus dem Eve2Twitch-Chat) werden automatisch über ESI ergänzt, für ältere Einträge per Knopf „Fehlende IDs ergänzen“
  - Optional lassen sich EVE-Namen und Porträts im Overlay verbergen (`hide_eve_names`), dann erscheint `[EVE-NAME]`
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

### Bedien-Dock in OBS

In OBS unter **Docks → Benutzerdefinierte Browser-Docks** einen Namen (z. B. „Giveaway“) und die URL `http://localhost:8766/dock` eintragen. Das Dock zeigt Status, Countdown, Teilnehmer, Gewinner (mit EVE-Porträt) und das Protokoll. Es enthält die Knöpfe START / STOP & AUSLOSEN, BREACH und RESET (RESET verlangt einen zweiten Klick) und den Schalter für das Auto-Giveaway (wird in `config.json` gespeichert). Unter „Giveaway-Werte“ lassen sich Keyword, Dauer und Run-Dauer für den nächsten Start anpassen.

Das Dock und seine Befehle sind nur vom selben PC aus erreichbar (`localhost`). Aufrufe aus dem Netzwerk und von fremden Webseiten werden abgelehnt; das Overlay selbst bleibt im Netzwerk erreichbar.

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
| `hide_eve_names` | EVE-Namen und Porträts im Overlay verbergen (Standard: `false`) |
| `auto_enabled` | Auto-Giveaway an/aus (Standard: `false`) |
| `auto_interval` | Pause bis zum nächsten Giveaway, `mm:ss` oder Minuten (Standard: `30:00`) |
| `auto_winner_hold` | Sekunden, die der Gewinner im Auto-Modus stehen bleibt (Standard: 20) |
| `open_message`, `close_message`, `win_message` | Chattexte (`{keyword}`, `{winner}`) |
| `sound` | Soundeffekte im Programm |
| `eve_enabled`, `eve_command`, `eve_api_url`, `eve_api_key`, `eve2twitch_bot_name` | EVE-Modus |

> Das OAuth-Token steht im Klartext in `config.json`. Die Datei deshalb nicht weitergeben oder ins Repository einchecken (sie steht in `.gitignore`).

## Bauen

```sh
cargo build --release      # Ergebnis: target/release/CyberGiveaway(.exe)
cargo test
```

GitHub Actions baut bei jedem Push eine Windows- und eine Linux-Version. Wird auf GitHub ein Release (z. B. `v0.0.1`) veröffentlicht, hängt der Workflow beide Pakete daran an.

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
| 0.0.4 | Bedien-Dock für OBS unter `/dock` (Steuerung, Status, Auto-Giveaway-Schalter, Protokoll); nur lokal erreichbar |
| 0.0.3 | Auto-Giveaway mit einstellbarer Pause (in den Einstellungen ein- und ausschaltbar) und eigene Timer-Anzeige für OBS unter `/timer` |
| 0.0.2 | EVE-Porträt mit Charaktername und Tickern im Gewinner-Banner und im Programmfenster; Charakter-IDs werden über ESI ergänzt; Namensschutz im Overlay als Schalter |
| 0.0.1 | Erste Rust-Version: Matrix-Run im Shadowrun-Stil, neues Overlay, Verlauf-Tab, einstellbare Run-Dauer und Decker-Name |
