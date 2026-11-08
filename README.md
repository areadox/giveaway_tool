# Cyber Giveaway – Matrix-Run

Twitch-Giveaway-Tool für **Areadox Gaming** mit OBS-Overlay. Zuschauer treten per Chat-Befehl bei. Die Auslosung wird als Matrix-Run im Stil von *Shadowrun* (Pen & Paper) inszeniert: Ein Decker hackt sich in einen Host, setzt Marken, kämpft gegen IC, würfelt sichtbar mit W6-Pools und zieht am Ende die Gewinnerdatei heraus.

Rust-Neuimplementierung der früheren Python-Version (`CyberGiveaway.exe`). `config.json` und `giveaway.db` der alten Version können direkt weiterverwendet werden.

Aktuelle Version: **0.0.8**

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
- **Mehrere Gewinner:** Die Anzahl ist im Programmfenster, im Dock und unter *Einstellungen → Preise* einstellbar. Gezogen wird ohne Doppelte. Gibt es weniger Teilnehmer als Gewinner, gewinnen alle.
- **Preise:** eine Beschreibung („Was gibt es zu gewinnen?“) und/oder eine Preisliste. Mit Liste bekommt Gewinner 1 Preis 1, Gewinner 2 Preis 2 usw. (Reihenfolge per ▲/▼); ohne Liste gilt die Beschreibung für alle. Der Preis erscheint im Overlay, in der Timer-Anzeige, im Dock, im Chat und im Verlauf.
- **Preise aus den EVE-Assets:** Nach der EVE-Anmeldung lädt das Tool deine Assets mit Icon, Menge und Ort (Station/Struktur, Container, Schiff), durchsuchbar; ein Klick markiert ein Item als Preis. Einrichtung siehe unten.
- **Fairness:** Die Gewinner werden vor dem Run gleichverteilt per Zufall gezogen. Der Run ist nur Inszenierung und endet immer erfolgreich.
- **OBS-Overlay** (Browserquelle, transparent):
  - `http://localhost:8766` live
  - `http://localhost:8766/test` Demo-Schleife zum Einrichten (`/test?winners=3` zeigt mehrere Gewinner mit Beispielpreisen)
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
- **Datenbank:** Zuordnungen anlegen, bearbeiten, löschen und als CSV exportieren; Verlauf aller Gewinner mit Preis.

## Installation

Alle Downloads liegen unter [Releases](https://github.com/areadox/giveaway_tool/releases/latest).

**Windows**
1. `CyberGiveaway-windows-x64.zip` herunterladen und in einen eigenen Ordner entpacken (oder nur `CyberGiveaway-windows-x64.exe` in einen eigenen Ordner legen).
2. Optional `config.json` und `giveaway.db` der alten Version in denselben Ordner kopieren.
3. Die `.exe` starten. Beim ersten Start öffnen sich die Einstellungen.

**Linux** (x86-64, glibc 2.35 oder neuer, z. B. Ubuntu 22.04+, Debian 12+, Fedora 36+)
1. `CyberGiveaway-linux-x64.tar.gz` herunterladen und entpacken: `tar xzf CyberGiveaway-linux-x64.tar.gz`
2. Im Ordner `CyberGiveaway` starten: `./CyberGiveaway` (beim einzelnen Programm `CyberGiveaway-linux-x64` vorher `chmod +x CyberGiveaway-linux-x64`).
3. Benötigt eine grafische Sitzung (X11 oder Wayland). Ohne Fenster nur den Overlay-Server: `./CyberGiveaway --headless`. Soundeffekte gibt es nur unter Windows.

**OBS:** eine **Browserquelle** mit `http://localhost:8766` anlegen (1920 × 1080).

Konfiguration und Datenbank liegen immer neben dem Programm.

## Tray (Infobereich)

Unter Windows startet das Programm standardmäßig nur als Symbol im Tray (Infobereich der Taskleiste) – der Giveaway läuft im Hintergrund, bedient wird über das OBS-Dock.

- **Doppelklick** (oder Linksklick) auf das Symbol öffnet das Programmfenster.
- **Rechtsklick** öffnet das Menü: Fenster anzeigen, Bedien-Dock im Browser öffnen, Overlay-Demo öffnen, Beenden.
- Das **X** des Fensters blendet es nur aus; beendet wird über „Beenden“ im Tray-Menü.

Beides lässt sich unter *Einstellungen* bzw. im Dock abschalten (`start_in_tray`, `close_to_tray`). Beim allerersten Start (noch kein Kanal/Token eingetragen) öffnet sich immer das Fenster mit den Einstellungen. Unter Linux gibt es kein Tray-Symbol; dort startet das Programm mit `start_in_tray` minimiert.

## Bedienung

| Knopf | Wirkung |
|---|---|
| **START** | öffnet die Teilnahme und startet den Countdown |
| **STOP & AUSLOSEN** | schließt die Teilnahme und startet den Matrix-Run |
| **BREACH** | startet den Run sofort mit dem aktuellen Pool |
| **RESET** | leert Pool, Protokoll und Overlay |

Das Feld **RUN (s)** legt fest, wie lange der Run im Overlay dauert (mindestens 8 Sekunden, Standard 30).

### EVE-Assets als Preise einrichten

Für den Zugriff auf deine Assets braucht das Tool eine eigene, kostenlose EVE-App (einmalig):

1. Auf [developers.eveonline.com](https://developers.eveonline.com/applications) mit deinem EVE-Account anmelden und eine neue Anwendung anlegen.
2. Verbindungstyp **Authentication & API Access** wählen und den Scope `esi-assets.read_assets.v1` hinzufügen. Optional zusätzlich `esi-universe.read_structures.v1`, wenn Items in Citadels/Strukturen mit deren Namen angezeigt werden sollen – dann im Tool den Haken „Strukturnamen (Citadels) anzeigen“ setzen. Ohne diesen Scope erscheinen Strukturen als „Struktur <ID>“.
3. Als Callback-URL `http://localhost:8766/eve/callback` eintragen (läuft das Overlay auf einem anderen Port, steht die passende Adresse unter *Einstellungen → Preise*).
4. Die **Client-ID** der App unter *Einstellungen → Preise* eintragen und „Übernehmen“ klicken. Ein Secret ist nicht nötig (Anmeldung per PKCE).
5. „Mit EVE verbinden“ klicken, im Browser den Charakter wählen und bestätigen. Danach lädt das Tool die Assets; später reicht „Assets laden“.

Die Anmeldung wird als Refresh-Token in `config.json` gespeichert (wie das Twitch-Token: Datei nicht weitergeben). „Abmelden“ entfernt sie. Gelesen wird nur; das Tool verschickt keine Items – die Übergabe (z. B. per Vertrag) machst du selbst.

### Bedien-Dock in OBS

In OBS unter **Docks → Benutzerdefinierte Browser-Docks** einen Namen (z. B. „Giveaway“) und die URL `http://localhost:8766/dock` eintragen. Das Dock zeigt Status, Countdown, Teilnehmer, Gewinner (mit EVE-Porträt) und das Protokoll. Es enthält die Knöpfe START / STOP & AUSLOSEN, BREACH und RESET (RESET verlangt einen zweiten Klick) und den Schalter für das Auto-Giveaway (wird in `config.json` gespeichert). Unter „Giveaway-Werte“ lassen sich Keyword, Dauer und Run-Dauer für den nächsten Start anpassen.

Unter **Einstellungen** stehen alle Einstellungen des Programms zur Verfügung (Twitch, Giveaway, Auto-Giveaway, Chat-Texte, EVE, Port und Tray-Verhalten), unter **Preise** die Preisbeschreibung, die Preisliste (Reihenfolge, Menge, eigene Preise) sowie EVE-Anmeldung, Asset-Suche und „+ Preis“. Die EVE-Anmeldung öffnet sich im normalen Browser. OAuth-Token und eve2twitch-Key zeigt das Dock nie an; dort eingetragen werden sie nur, wenn ein neuer Wert eingegeben wird. Die Datenbank-Zuordnungen und der Verlauf bleiben im Programmfenster.

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
| `winners` | Anzahl der Gewinner (Standard: 1) |
| `prize_text` | Was es zu gewinnen gibt (Freitext; leer = Kurzfassung der Preisliste) |
| `prizes` | Preisliste (wird im Programm gepflegt) |
| `eve_sso_client_id` | Client-ID der eigenen EVE-App für den Asset-Abruf |
| `eve_sso_structures` | zusätzlich den Scope für Strukturnamen anfragen (Standard: `false`) |
| `start_in_tray` | beim Start nur im Tray (Linux: minimiert) (Standard: `true`) |
| `close_to_tray` | Fenster-X schließt in den Tray, Windows (Standard: `true`) |
| `eve_sso_refresh_token`, `eve_sso_character_id`, `eve_sso_character_name` | EVE-Anmeldung (wird vom Programm gesetzt) |
| `open_message`, `close_message`, `win_message` | Chattexte (`{keyword}`, `{winner}`, `{prize}`). Fehlt `{prize}`, wird der Preis automatisch angehängt; bei mehreren Gewinnern geht je Gewinner eine Nachricht raus (ab 11 Gewinnern eine Sammelliste). |
| `sound` | Soundeffekte im Programm |
| `eve_enabled`, `eve_command`, `eve_api_url`, `eve_api_key`, `eve2twitch_bot_name` | EVE-Modus |

> Das OAuth-Token steht im Klartext in `config.json`. Die Datei deshalb nicht weitergeben oder ins Repository einchecken (sie steht in `.gitignore`).

## Bauen

```sh
cargo build --release      # Ergebnis: target/release/CyberGiveaway(.exe)
cargo test
```

GitHub Actions baut und testet bei jedem Push auf `main` eine Windows- und eine Linux-Version. Steht in `Cargo.toml` eine Version, zu der es noch kein Release gibt, veröffentlicht der Workflow automatisch das Release `v<Version>` mit beiden Programmen; als Release-Text dient die Zeile aus der Versionstabelle unten. Für ein neues Release also die Version in `Cargo.toml` erhöhen und die Tabelle ergänzen.

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
| `src/sso.rs` | EVE-Anmeldung (SSO/PKCE) und Asset-Abruf |
| `src/prize.rs` | Preise und Zuordnung zu den Gewinnern |
| `src/tray.rs` | Programm-Icon und Tray-Symbol (Windows) |
| `src/db.rs`, `src/config.rs`, `src/sound.rs` | Datenbank, Konfiguration, Soundeffekte |

*Shadowrun* ist eine Marke der Topps Company, Inc. Dieses Tool ist ein inoffizielles Fanprojekt und nutzt nur allgemeine Spielbegriffe; Hosts, Decks und Texte sind frei erfunden.

## Versionen

| Version | Änderungen |
|---|---|
| 0.0.8 | Alle Einstellungen und Preise (inkl. EVE-Assets) im OBS-Dock; Start minimiert im Tray mit Tray-Menü, X schließt in den Tray (Windows); Programm-Icon |
| 0.0.7 | EVE-Anmeldung: ein Rückruf aus einem älteren Anmeldeversuch bricht die laufende Anmeldung nicht mehr ab |
| 0.0.6 | EVE-Anmeldung fragt standardmäßig nur noch den Asset-Scope an; Strukturnamen optional per Haken; verständliche Meldung bei fehlendem Scope |
| 0.0.5 | Mehrere Gewinner; Preisbeschreibung und Preisliste (Overlay, Timer, Dock, Chat, Verlauf); EVE-Anmeldung und Auswahl von Preisen aus den eigenen Assets |
| 0.0.4 | Bedien-Dock für OBS unter `/dock` (Steuerung, Status, Auto-Giveaway-Schalter, Protokoll); nur lokal erreichbar |
| 0.0.3 | Auto-Giveaway mit einstellbarer Pause (in den Einstellungen ein- und ausschaltbar) und eigene Timer-Anzeige für OBS unter `/timer` |
| 0.0.2 | EVE-Porträt mit Charaktername und Tickern im Gewinner-Banner und im Programmfenster; Charakter-IDs werden über ESI ergänzt; Namensschutz im Overlay als Schalter |
| 0.0.1 | Erste Rust-Version: Matrix-Run im Shadowrun-Stil, neues Overlay, Verlauf-Tab, einstellbare Run-Dauer und Decker-Name |
