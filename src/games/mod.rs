//! Spiel-Abläufe für die Designs. Jeder Ablauf erzeugt – wie der Matrix-Run –
//! vorab eine Folge von Bildern; die Gewinner stehen vorher fair fest.
//!
//! - `dice`     Matrix-Run mit W6-Pools (Shadowrun) → `crate::matrix`
//! - `datasite` Data-Site hacken wie in EVE Online (Virus gegen Subsysteme)
//! - `treasure` Schatzsuche mit Hinweisen, Graben und Heiß/Kalt (Palia-Stil)
//! - `riddle`   Rätsel, Runenschloss, Feuerschalen, dann Lootwurf (WoW-Stil)
//! - `battle`   Bosskampf mit Spezialattacken und Freundschaftskraft (Anime)

mod battle;
mod datasite;
mod riddle;
mod treasure;

use crate::matrix::{self, Candidate, Frame, LogLine, RunSetup, RunView};
use crate::sound::Sfx;
use crate::themes::Lexicon;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

pub const GAMES: [&str; 5] = ["dice", "datasite", "treasure", "riddle", "battle"];

/// Erzeugt den Run für den Ablauf `game` (unbekannt → Würfel-Run).
pub fn generate<R: Rng>(game: &str, rng: &mut R, s: RunSetup<'_>) -> Vec<Frame> {
    match game {
        "datasite" => datasite::run(rng, &s),
        "treasure" => treasure::run(rng, &s),
        "riddle" => riddle::run(rng, &s),
        "battle" => battle::run(rng, &s),
        _ => matrix::generate(rng, s),
    }
}

/// Gemeinsames Werkzeug der Abläufe: Texte, Log, Phasen, Bilder, Finale.
pub(crate) struct Stage<'a, R: Rng> {
    pub rng: &'a mut R,
    pub lx: &'a Lexicon,
    pub v: RunView,
    frames: Vec<(f64, RunView, Option<Sfx>)>,
}

impl<'a, R: Rng> Stage<'a, R> {
    pub fn new(rng: &'a mut R, s: &RunSetup<'a>, game: &str) -> Self {
        Stage {
            rng,
            lx: s.lexicon,
            v: RunView::blank(game, s.decker, s.participants.len()),
            frames: vec![],
        }
    }

    /// Text aus dem Wörterbuch; `{decker}` und `{host}` gehen immer.
    pub fn tx(&self, key: &str, args: &[(&str, String)]) -> String {
        let mut all = args.to_vec();
        all.push(("decker", self.v.decker.clone()));
        all.push(("host", self.v.host.clone()));
        self.lx.t(key, &all)
    }

    /// Zufälliger Eintrag einer Liste (leer, wenn die Liste fehlt).
    pub fn pick(&mut self, list: &str) -> String {
        self.lx
            .list(list)
            .choose(self.rng)
            .cloned()
            .unwrap_or_default()
    }

    pub fn log(&mut self, key: &str, args: &[(&str, String)], tone: &str) {
        let text = self.tx(key, args);
        self.v.log.push(LogLine {
            text,
            tone: tone.into(),
        });
    }

    pub fn phase(&mut self, key: &str, text_key: &str, args: &[(&str, String)]) {
        self.v.phase = self.tx(&format!("phase.{key}"), &[]);
        self.v.phase_key = key.into();
        self.v.phase_text = self.tx(text_key, args);
    }

    pub fn push<S: Serialize>(&mut self, weight: f64, sfx: Option<Sfx>, scene: &S) {
        self.v.scene = serde_json::to_value(scene).ok();
        self.frames.push((weight, self.v.clone(), sfx));
    }

    /// Auslosung: Teilnehmerliste, aus der nach und nach alle außer den Gewinnern fallen.
    /// Mit `values` bekommt jeder einen Wurf 1–100, die Gewinner die höchsten.
    pub fn finale<S: Serialize>(&mut self, s: &RunSetup<'_>, scene: &S, values: bool) {
        let is_winner = |n: &str| s.winners.iter().any(|w| w == n);
        let shown: Vec<String> = s.winners.iter().take(24).cloned().collect();
        let fill = (shown.len() + 4).clamp(12, 24) - shown.len();
        let mut others: Vec<String> = s
            .participants
            .iter()
            .filter(|p| !is_winner(p))
            .cloned()
            .collect();
        others.shuffle(self.rng);
        others.truncate(fill);
        let mut names = others;
        names.extend(shown);
        names.shuffle(self.rng);
        // Würfe: Gewinner oben, alle verschieden
        let mut pool: Vec<u32> = (1..=100).collect();
        pool.shuffle(self.rng);
        let mut rolls: Vec<u32> = pool.into_iter().take(names.len()).collect();
        rolls.sort_unstable_by(|a, b| b.cmp(a));
        let n_win = names.iter().filter(|n| is_winner(n)).count();
        let (top, low) = rolls.split_at(n_win);
        let (mut top, mut low) = (top.to_vec(), low.to_vec());
        top.shuffle(self.rng);
        low.shuffle(self.rng);
        self.v.candidates = names
            .iter()
            .map(|n| Candidate {
                name: n.clone(),
                state: "offen".into(),
                value: values.then(|| {
                    if is_winner(n) {
                        top.pop().unwrap_or(100)
                    } else {
                        low.pop().unwrap_or(1)
                    }
                }),
            })
            .collect();
        self.phase("fin", "fin.start", &[]);
        self.log(
            "log.register",
            &[("n", s.participants.len().to_string())],
            "info",
        );
        self.push(1.4, Some(Sfx::Dice), scene);

        let mut losers: Vec<usize> = (0..self.v.candidates.len())
            .filter(|&i| !is_winner(&self.v.candidates[i].name))
            .collect();
        if values {
            // niedrigste Würfe fliegen zuerst raus
            losers.sort_by_key(|&i| self.v.candidates[i].value);
        } else {
            losers.shuffle(self.rng);
        }
        let steps = losers.len().clamp(1, 4);
        let chunk = losers.len().div_ceil(steps).max(1);
        for part in losers.chunks(chunk) {
            for &i in part {
                self.v.candidates[i].state = "raus".into();
            }
            let left = self
                .v
                .candidates
                .iter()
                .filter(|c| c.state == "offen")
                .count();
            if left == 1 {
                self.phase("fin", "fin.left1", &[]);
            } else {
                self.phase("fin", "fin.left", &[("n", left.to_string())]);
            }
            self.push(0.9, Some(Sfx::Click), scene);
        }
        for c in self.v.candidates.iter_mut() {
            if is_winner(&c.name) {
                c.state = "gewinner".into();
            }
        }
    }

    /// Abschlussbild mit Gewinnern und Zeitachse.
    pub fn finish<S: Serialize>(mut self, s: &RunSetup<'_>, scene: &S) -> Vec<Frame> {
        self.v.winners = s.winners.to_vec();
        self.v.done = true;
        self.phase("done", "pt.done", &[]);
        let names: Vec<String> = s.winners.iter().take(6).map(|w| format!("@{w}")).collect();
        let more = s.winners.len().saturating_sub(6);
        let list = if more > 0 {
            self.tx(
                "log.and_more",
                &[("list", names.join(", ")), ("n", more.to_string())],
            )
        } else {
            names.join(", ")
        };
        if s.winners.len() == 1 {
            self.log("log.win1", &[("list", list)], "good");
        } else {
            self.log(
                "log.winN",
                &[("list", list), ("n", s.winners.len().to_string())],
                "good",
            );
        }
        self.push(0.0, Some(Sfx::Win1), scene);
        matrix::timeline(self.frames, s.duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// Alle Abläufe enden mit den Gewinnern, ohne offene Platzhalter, in sinnvoller Länge.
    #[test]
    fn all_games_finish_cleanly() {
        let reg = crate::themes::registry();
        let people: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
        for (id, ..) in reg.list() {
            let theme = reg.get(&id);
            for seed in 0..80u64 {
                let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                let n = [1usize, 1, 3, 12, 40][(seed % 5) as usize];
                let winners: Vec<String> = people
                    .iter()
                    .skip(seed as usize % 7)
                    .take(n)
                    .cloned()
                    .collect();
                let parts = if seed % 11 == 0 {
                    &people[..n.max(1)]
                } else {
                    &people[..]
                };
                let winners: Vec<String> =
                    winners.into_iter().filter(|w| parts.contains(w)).collect();
                let winners = if winners.is_empty() {
                    vec![parts[0].clone()]
                } else {
                    winners
                };
                let dur = [10.0, 30.0, 60.0][(seed % 3) as usize];
                let frames = generate(
                    &theme.info.game,
                    &mut rng,
                    RunSetup {
                        participants: parts,
                        winners: &winners,
                        decker: "streamer",
                        duration: dur,
                        lexicon: &theme.lexicon,
                    },
                );
                let last = frames.last().unwrap();
                assert!(last.view.done, "{id}");
                assert_eq!(last.view.winners, winners, "{id}");
                assert_eq!(last.view.game, theme.info.game, "{id}");
                assert!((last.t - dur).abs() < 1e-9);
                assert!(
                    frames.windows(2).all(|w| w[0].t <= w[1].t),
                    "{id}: Zeit monoton"
                );
                assert!(
                    frames.len() > 8 && frames.len() < 160,
                    "{id}: {} Bilder",
                    frames.len()
                );
                let won = last
                    .view
                    .candidates
                    .iter()
                    .filter(|c| c.state == "gewinner")
                    .count();
                assert_eq!(won, winners.len().min(24), "{id}");
                for f in &frames {
                    let mut texts: Vec<&str> = vec![&f.view.phase, &f.view.phase_text];
                    texts.extend(f.view.log.iter().map(|l| l.text.as_str()));
                    let scene = f
                        .view
                        .scene
                        .as_ref()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    for t in texts.iter().copied().chain([scene.as_str()]) {
                        assert!(
                            !t.contains("{decker}") && !t.contains("{n}") && !t.contains("{host}"),
                            "{id}: offener Platzhalter: {t}"
                        );
                    }
                    for t in texts {
                        assert!(
                            !t.is_empty() || f.view.phase_key == "init",
                            "{id}: leerer Text"
                        );
                        assert!(
                            !(t.contains('.')
                                && !t.contains(' ')
                                && t.chars().next().is_some_and(|c| c.is_ascii_lowercase())),
                            "{id}: Schlüssel statt Text: {t}"
                        );
                    }
                }
                if theme.info.game == "riddle" {
                    let c = &last.view.candidates;
                    let min_win = c
                        .iter()
                        .filter(|c| c.state == "gewinner")
                        .filter_map(|c| c.value)
                        .min()
                        .unwrap();
                    assert!(
                        c.iter()
                            .filter(|c| c.state == "raus")
                            .all(|c| c.value.unwrap() < min_win),
                        "{id}: Gewinner haben die höchsten Würfe"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod print {
    use super::*;
    use rand::SeedableRng;

    /// `GAME=datasite SEED=3 cargo test print_game -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_game() {
        let reg = crate::themes::registry();
        let id = std::env::var("THEME").unwrap_or_else(|_| "eve".into());
        let theme = reg.get(&id);
        let seed: u64 = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let people: Vec<String> = (0..14).map(|i| format!("user{i}")).collect();
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let frames = generate(
            &theme.info.game,
            &mut rng,
            RunSetup {
                participants: &people,
                winners: &["user3".to_string()],
                decker: "areadox",
                duration: 40.0,
                lexicon: &theme.lexicon,
            },
        );
        for f in &frames {
            println!("{:5.1}s [{}] {}", f.t, f.view.phase, f.view.phase_text);
        }
        println!("--- LOG ({} Bilder)", frames.len());
        for l in &frames.last().unwrap().view.log {
            println!("[{}] {}", l.tone, l.text);
        }
    }
}
