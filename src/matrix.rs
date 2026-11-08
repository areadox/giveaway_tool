//! Simulierter Matrix-Run im Stil von Shadowrun (Pen & Paper).
//!
//! Der Run wird beim Start komplett vorab „ausgewürfelt“ und als Folge von
//! Bildern (`Frame`) mit Zeitstempel ausgegeben. Regeln angelehnt an SR5/SR6:
//! W6-Pools, Erfolg bei 5 oder 6, Patzer wenn mehr als die Hälfte Einsen,
//! Marken auf dem Host, Overwatch-Wert (OW) bis zur GOD-Konvergenz bei 40,
//! IC, Zustandsmonitore, Edge.
//!
//! Der Gewinner wird vor dem Run fair per Zufall bestimmt; der Run ist nur
//! die Inszenierung und endet immer mit der extrahierten Gewinnerdatei.

use crate::sound::Sfx;
use crate::themes::Lexicon;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

pub const OS_MAX: u32 = 40;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Pool {
    pub label: String,
    pub dice: Vec<u8>,
    pub hits: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Roll {
    pub id: u32,
    /// "decker" | "host" | "ice"
    pub actor: String,
    pub action: String,
    pub attacker: Pool,
    pub defender: Option<Pool>,
    pub threshold: Option<u32>,
    pub net: i32,
    pub glitch: bool,
    pub critical: bool,
    pub edge: bool,
    pub success: bool,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IceView {
    pub name: String,
    /// "patrol" | "killer" | "black" | "tar" | "acid" | "track" | "crash" | "blaster" | "spider"
    pub kind: String,
    /// "active" | "crashed" | "shaken"
    pub status: String,
    /// Anzeigetext des Status (aus dem Design)
    pub status_text: String,
    pub hp: u8,
    pub hp_max: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeView {
    pub label: String,
    /// "locked" | "active" | "done" | "alert" | "target"
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub text: String,
    /// "info" | "good" | "bad" | "warn" | "dim" | "edge"
    pub tone: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub name: String,
    /// "offen" | "raus" | "gewinner"
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub host: String,
    pub host_sculpt: String,
    pub host_rating: u8,
    /// Angriff, Schleicher, Datenverarbeitung, Firewall
    pub asdf: [u8; 4],
    pub decker: String,
    pub deck: String,
    /// Anzeigetext (Heiß-/Kalt-Sim o. ä. aus dem Design)
    pub sim: String,
    pub hot: bool,
    pub os: u32,
    pub os_max: u32,
    pub marks: u8,
    pub matrix_cm: u8,
    pub matrix_max: u8,
    pub stun: u8,
    pub stun_max: u8,
    pub edge: u8,
    pub edge_max: u8,
    /// "none" | "passive" | "active"
    pub alert: String,
    pub link_locked: bool,
    pub combat_turn: u32,
    pub ice: Vec<IceView>,
    pub nodes: Vec<NodeView>,
    pub node: usize,
    /// Anzeigename der Phase (aus dem Design)
    pub phase: String,
    /// Feste Kennung: jackin | grid | marks | ice | search | bomb | crack | decode | jackout | converge | done
    pub phase_key: String,
    pub phase_text: String,
    pub roll: Option<Roll>,
    pub log: Vec<LogLine>,
    pub candidates: Vec<Candidate>,
    pub participants: usize,
    pub winners: Vec<String>,
    pub converged: bool,
    pub overall: f32,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub t: f64,
    pub view: RunView,
    #[serde(skip)]
    pub sfx: Option<Sfx>,
}

struct Decker {
    logic: u32,
    intuition: u32,
    willpower: u32,
    hacking: u32,
    cybercombat: u32,
    computer: u32,
    software: u32,
    attack: u32,
    sleaze: u32,
    dataproc: u32,
    firewall: u32,
    hot: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum IceKind {
    Patrol,
    Killer,
    Black,
    Blaster,
    Tar,
    Acid,
    Track,
    Crash,
    Spider,
}

impl IceKind {
    fn key(self) -> &'static str {
        match self {
            IceKind::Patrol => "patrol",
            IceKind::Killer => "killer",
            IceKind::Black => "black",
            IceKind::Blaster => "blaster",
            IceKind::Tar => "tar",
            IceKind::Acid => "acid",
            IceKind::Track => "track",
            IceKind::Crash => "crash",
            IceKind::Spider => "spider",
        }
    }
}

struct Gen<'a, R: Rng> {
    rng: &'a mut R,
    lx: &'a Lexicon,
    v: RunView,
    d: Decker,
    r: u32,
    frames: Vec<(f64, RunView, Option<Sfx>)>,
    roll_id: u32,
    file_secured: bool,
    os_warned: bool,
}

fn count_hits(d: &[u8]) -> u32 {
    d.iter().filter(|&&x| x >= 5).count() as u32
}

fn glitches(d: &[u8], hits: u32) -> (bool, bool) {
    let ones = d.iter().filter(|&&x| x == 1).count();
    let g = !d.is_empty() && ones * 2 > d.len();
    (g, g && hits == 0)
}

impl<'a, R: Rng> Gen<'a, R> {
    fn d6(&mut self, n: u32) -> Vec<u8> {
        (0..n.max(1)).map(|_| self.rng.gen_range(1..=6)).collect()
    }

    /// Würfelt einen Pool; `limit` begrenzt die Erfolge (SR5-Limit), `explode` = Sechser explodieren (Edge).
    fn pool(
        &mut self,
        label: impl Into<String>,
        n: u32,
        limit: Option<u32>,
        explode: bool,
    ) -> Pool {
        let mut dice = self.d6(n);
        if explode {
            let mut extra = dice.iter().filter(|&&x| x == 6).count();
            while extra > 0 && dice.len() < 40 {
                let add = self.d6(extra as u32);
                extra = add.iter().filter(|&&x| x == 6).count();
                dice.extend(add);
            }
        }
        let mut hits = count_hits(&dice);
        if let (Some(l), false) = (limit, explode) {
            hits = hits.min(l);
        }
        Pool {
            label: label.into(),
            dice,
            hits,
        }
    }

    /// Text aus dem Wörterbuch des Designs.
    /// `{decker}` und `{host}` stehen in jedem Text zur Verfügung.
    fn tx(&self, key: &str, args: &[(&str, String)]) -> String {
        let mut all = args.to_vec();
        all.push(("decker", self.v.decker.clone()));
        all.push(("host", self.v.host.clone()));
        self.lx.t(key, &all)
    }

    fn ice_name(&self, kind: IceKind) -> String {
        self.tx(&format!("ice.{}", kind.key()), &[])
    }

    fn set_ice_status(&mut self, idx: usize, status: &str) {
        let key = match status {
            "crashed" => "st.crashed",
            "shaken" => "st.shaken",
            _ => "st.active",
        };
        let text = self.tx(key, &[]);
        let ice = &mut self.v.ice[idx];
        ice.status = status.into();
        ice.status_text = text;
    }

    fn hot_mod(&self) -> String {
        if self.d.hot {
            self.tx("mod.hot", &[])
        } else {
            String::new()
        }
    }

    fn wound_label(&self, wm: u32) -> String {
        if wm > 0 {
            self.tx("mod.wound", &[("n", wm.to_string())])
        } else {
            String::new()
        }
    }

    fn log(&mut self, text: impl Into<String>, tone: &str) {
        self.v.log.push(LogLine {
            text: text.into(),
            tone: tone.into(),
        });
    }

    /// Frame ohne Würfelwurf (Würfelfeld wird geleert).
    fn push(&mut self, weight: f64, sfx: Option<Sfx>) {
        self.v.roll = None;
        self.frames.push((weight, self.v.clone(), sfx));
    }

    fn push_roll(&mut self, weight: f64, sfx: Option<Sfx>) {
        self.frames.push((weight, self.v.clone(), sfx));
    }

    fn set_node(&mut self, idx: usize, status: &str) {
        for (i, n) in self.v.nodes.iter_mut().enumerate() {
            if i < idx && n.status != "alert" {
                n.status = "done".into();
            }
        }
        self.v.nodes[idx].status = status.into();
        self.v.node = idx;
    }

    /// Phase setzen: `key` ohne Präfix (z. B. "ice"); der Text kommt vom Aufrufer.
    fn phase(&mut self, key: &str, text: impl Into<String>) {
        self.v.phase = self.tx(&format!("phase.{key}"), &[]);
        self.v.phase_key = key.into();
        self.v.phase_text = text.into();
    }

    fn add_os(&mut self, n: u32) {
        let cap = if self.file_secured {
            OS_MAX
        } else {
            OS_MAX - 1
        };
        self.v.os = (self.v.os + n).min(cap);
        if self.v.os >= 30 && !self.os_warned {
            self.os_warned = true;
            let t = self.tx(
                "log.os_warn",
                &[("os", self.v.os.to_string()), ("max", OS_MAX.to_string())],
            );
            self.log(t, "warn");
        }
    }

    /// Verteidigungserfolge gegen illegale Aktionen erhöhen den OW (gedämpft,
    /// damit die Spannung über den ganzen Run verteilt bleibt).
    fn add_os_hits(&mut self, hits: u32) {
        let n = (hits as f64 * 0.6).round() as u32;
        self.add_os(n);
    }

    fn matrix_damage(&mut self, n: u8) -> u8 {
        // Deck nie vor dem Ende bricken – höchstens auf 1 Kästchen Rest.
        let room = self
            .v
            .matrix_max
            .saturating_sub(1)
            .saturating_sub(self.v.matrix_cm);
        let n = n.min(room);
        self.v.matrix_cm += n;
        n
    }

    fn stun_damage(&mut self, n: u8) -> u8 {
        let room = self
            .v
            .stun_max
            .saturating_sub(2)
            .saturating_sub(self.v.stun);
        let n = n.min(room);
        self.v.stun += n;
        n
    }

    fn hot(&self) -> u32 {
        if self.d.hot {
            2
        } else {
            0
        }
    }

    fn wound_mod(&self) -> u32 {
        (self.v.matrix_cm / 3 + self.v.stun / 3) as u32
    }

    /// Vergleichende Probe Decker gegen Host/IC. Gibt den Wurf zurück (ohne Frame).
    #[allow(clippy::too_many_arguments)]
    fn opposed(
        &mut self,
        actor: &str,
        action: &str,
        att_label: String,
        att_n: u32,
        limit: Option<u32>,
        def_label: String,
        def_n: u32,
        force_success: bool,
    ) -> Roll {
        let mut tries = 0;
        loop {
            let a = self.pool(att_label.clone(), att_n, limit, false);
            let d = self.pool(def_label.clone(), def_n, None, false);
            let net = a.hits as i32 - d.hits as i32;
            let (g, c) = glitches(&a.dice, a.hits);
            tries += 1;
            if force_success && (net <= 0 || c) && tries < 60 {
                continue;
            }
            self.roll_id += 1;
            return Roll {
                id: self.roll_id,
                actor: actor.into(),
                action: action.into(),
                attacker: a,
                defender: Some(d),
                threshold: None,
                net,
                glitch: g,
                critical: c,
                edge: false,
                success: net > 0,
                verdict: String::new(),
            };
        }
    }

    /// Edge: Grenzen sprengen – Edge-Würfel dazu, Sechser explodieren, keine Limits.
    fn edge_roll(
        &mut self,
        action: &str,
        att_label: String,
        att_n: u32,
        def_label: String,
        def_n: u32,
    ) -> Roll {
        self.v.edge = self.v.edge.saturating_sub(1);
        let edge = self.v.edge_max as u32;
        let mut tries = 0;
        loop {
            let a = self.pool(
                self.tx(
                    "mod.edge",
                    &[("label", att_label.clone()), ("n", edge.to_string())],
                ),
                att_n + edge,
                None,
                true,
            );
            let d = self.pool(def_label.clone(), def_n, None, false);
            let net = a.hits as i32 - d.hits as i32;
            tries += 1;
            if net <= 0 && tries < 80 {
                continue;
            }
            let (g, c) = glitches(&a.dice, a.hits);
            self.roll_id += 1;
            return Roll {
                id: self.roll_id,
                actor: "decker".into(),
                action: action.into(),
                attacker: a,
                defender: Some(d),
                threshold: None,
                net,
                glitch: g,
                critical: c,
                edge: true,
                success: true,
                verdict: String::new(),
            };
        }
    }

    fn show_roll(&mut self, mut roll: Roll, verdict: impl Into<String>, weight: f64) {
        roll.verdict = verdict.into();
        let sfx = if roll.critical || roll.glitch {
            Sfx::Glitch
        } else {
            Sfx::Dice
        };
        self.v.roll = Some(roll);
        self.push_roll(weight, Some(sfx));
    }

    /// Host-Verteidigung: Stufe + Attribut (`attr_key` = "firewall" | "attack").
    fn host_def(&self, attr: usize, attr_key: &str) -> (String, u32) {
        let a = self.v.asdf[attr] as u32;
        let label = self.tx(
            "pool.host",
            &[
                ("r", self.r.to_string()),
                ("attr", self.tx(&format!("attr.{attr_key}"), &[])),
                ("a", a.to_string()),
            ],
        );
        (label, self.r + a)
    }

    fn pool_label(&self, key: &str, a: u32, b: u32) -> String {
        self.tx(key, &[("a", a.to_string()), ("b", b.to_string())])
    }

    // ----------------------------------------------------------------- Phasen

    fn jack_in(&mut self) {
        self.set_node(0, "active");
        let dk = self.v.decker.clone();
        let t = self.tx("pt.jackin", &[("decker", dk.clone())]);
        self.phase("jackin", t);
        let t = self.tx(
            "log.deck_online",
            &[("deck", self.v.deck.clone()), ("decker", dk)],
        );
        self.log(t, "info");
        if self.d.hot {
            let t = self.tx("log.hot", &[]);
            self.log(t, "warn");
        } else {
            let t = self.tx("log.cold", &[]);
            self.log(t, "dim");
        }
        self.push(1.2, Some(Sfx::Click));

        self.set_node(1, "active");
        let t = self.tx("pt.grid", &[("host", self.v.host.clone())]);
        self.phase("grid", t);
        let [a, s, d, f] = self.v.asdf.map(|x| x.to_string());
        let t = self.tx(
            "log.target",
            &[
                ("host", self.v.host.clone()),
                ("r", self.r.to_string()),
                ("a", a),
                ("s", s),
                ("d", d),
                ("f", f),
            ],
        );
        self.log(t, "info");
        self.push(1.0, None);

        // Matrixwahrnehmung (Erfolgsprobe, Schwelle 1)
        let n = self.d.computer + self.d.intuition + self.hot();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let label = self.pool_label("pool.perception", self.d.computer, self.d.intuition)
                + &self.hot_mod();
            let mut a = self.pool(label, n, Some(self.d.dataproc), false);
            if attempt >= 3 && a.hits == 0 {
                a.dice[0] = 5;
                a.hits = 1;
            }
            let (g, c) = glitches(&a.dice, a.hits);
            self.roll_id += 1;
            let ok = a.hits >= 1;
            let roll = Roll {
                id: self.roll_id,
                actor: "decker".into(),
                action: self.tx("act.perception", &[]),
                attacker: a,
                defender: None,
                threshold: Some(1),
                net: 0,
                glitch: g,
                critical: c,
                edge: false,
                success: ok,
                verdict: String::new(),
            };
            if ok {
                let sculpt = self.v.host_sculpt.clone();
                let t = self.tx("log.host_found", &[("sculpt", sculpt)]);
                self.log(t, "good");
                let v = self.tx("v.host_found", &[]);
                self.show_roll(roll, v, 1.5);
                break;
            }
            let t = self.tx("log.host_miss", &[]);
            self.log(t, "dim");
            let v = self.tx("v.no_hit", &[]);
            self.show_roll(roll, v, 1.2);
        }
    }

    fn marks(&mut self, target: u8) {
        self.set_node(2, "active");
        let t = self.tx("pt.marks", &[("host", self.v.host.clone())]);
        self.phase("marks", t);
        let brute = self.rng.gen_bool(0.35);
        let dk = self.v.decker.clone();
        if brute {
            let t = self.tx("log.choose_brute", &[("decker", dk)]);
            self.log(t, "warn");
        } else {
            let t = self.tx("log.choose_hack", &[("decker", dk)]);
            self.log(t, "info");
        }
        let mut attempts = 0;
        while self.v.marks < target {
            attempts += 1;
            let need = target - self.v.marks;
            // Mehrere Marken auf einmal: −4 für 2 Marken, −10 für 3 (nur Brute Force)
            let want: u8 = if brute && need >= 2 && self.rng.gen_bool(0.4) {
                2
            } else {
                1
            };
            let pen = if want == 2 { 4 } else { 0 };
            let (action, att_label, base, limit, def_attr, def_name) = if brute {
                (
                    self.tx("act.brute", &[]),
                    self.pool_label("pool.brute", self.d.cybercombat, self.d.logic),
                    self.d.cybercombat + self.d.logic,
                    self.d.attack,
                    3,
                    "firewall",
                )
            } else {
                (
                    self.tx("act.hack", &[]),
                    self.pool_label("pool.hack", self.d.hacking, self.d.logic),
                    self.d.hacking + self.d.logic,
                    self.d.sleaze,
                    3,
                    "firewall",
                )
            };
            let att_label = format!(
                "{att_label}{}{}{}",
                self.hot_mod(),
                if pen > 0 {
                    self.tx("mod.two_marks", &[])
                } else {
                    String::new()
                },
                self.wound_label(self.wound_mod())
            );
            let action = action.as_str();
            let n = (base + self.hot())
                .saturating_sub(pen + self.wound_mod())
                .max(2);
            let (def_label, def_n) = self.host_def(def_attr, def_name);
            let force = attempts >= 5;
            let use_edge = attempts >= 3 && self.v.edge > 0 && self.rng.gen_bool(0.5);
            let roll = if use_edge {
                let t = self.tx("log.edge_marks", &[("decker", self.v.decker.clone())]);
                self.log(t, "edge");
                self.edge_roll(action, att_label, n, def_label, def_n)
            } else {
                self.opposed(
                    "decker",
                    action,
                    att_label,
                    n,
                    Some(limit),
                    def_label,
                    def_n,
                    force,
                )
            };
            let def_hits = roll.defender.as_ref().map_or(0, |d| d.hits);
            self.add_os_hits(def_hits);
            if roll.success {
                let got = want.min(need);
                self.v.marks += got;
                let verdict = self.tx(
                    if got > 1 { "v.marks" } else { "v.mark" },
                    &[
                        ("n", got.to_string()),
                        ("marks", self.v.marks.to_string()),
                        ("target", target.to_string()),
                    ],
                );
                let t = self.tx(
                    "log.mark_ok",
                    &[
                        ("action", action.to_string()),
                        ("net", roll.net.to_string()),
                        ("marks", self.v.marks.to_string()),
                        ("target", target.to_string()),
                        ("os", self.v.os.to_string()),
                    ],
                );
                self.log(t, "good");
                if brute && self.v.alert != "active" {
                    self.v.alert = "active".into();
                    let t = self.tx("log.brute_noticed", &[]);
                    self.log(t, "bad");
                }
                if roll.glitch {
                    let t = self.tx("log.glitch_trace", &[]);
                    self.log(t, "warn");
                    self.add_os(2);
                }
                self.show_roll(roll, verdict, 1.6);
            } else {
                if roll.critical {
                    let dmg = self.matrix_damage(2);
                    let t = self.tx("log.crit_glitch", &[("n", dmg.to_string())]);
                    self.log(t, "bad");
                } else {
                    let t = self.tx(
                        "log.mark_fail",
                        &[
                            ("action", action.to_string()),
                            ("decker", self.v.decker.clone()),
                        ],
                    );
                    self.log(t, "bad");
                }
                self.v.alert = "active".into();
                self.v.nodes[2].status = "alert".into();
                let v = self.tx("v.blocked", &[]);
                self.show_roll(roll, v, 1.5);
                self.v.nodes[2].status = "active".into();
            }
        }
        let t = self.tx("log.marks_done", &[("n", self.v.marks.to_string())]);
        self.log(t, "good");
    }

    fn ice_fights(&mut self, count: usize) {
        self.set_node(3, "active");
        let alarm_before = self.v.alert == "active";
        if !alarm_before {
            self.v.alert = "passive".into();
        }
        let t = self.tx("pt.ice_start", &[]);
        self.phase("ice", t);
        let mut pool = vec![
            IceKind::Killer,
            IceKind::Acid,
            IceKind::Tar,
            IceKind::Track,
            IceKind::Crash,
            IceKind::Blaster,
        ];
        if self.r >= 6 || self.d.hot {
            pool.push(IceKind::Black);
        }
        pool.shuffle(self.rng);
        let mut list = vec![IceKind::Patrol];
        list.extend(pool.into_iter().take(count.saturating_sub(1)));
        if self.rng.gen_bool(0.18) && count >= 3 {
            let last = list.len() - 1;
            list[last] = IceKind::Spider;
        }

        for kind in list {
            self.v.combat_turn += 1;
            let hp_max = (8 + self.r.div_ceil(2)) as u8;
            let name = self.ice_name(kind);
            self.v.ice.push(IceView {
                name: name.clone(),
                kind: kind.key().into(),
                status: String::new(),
                status_text: String::new(),
                hp: hp_max,
                hp_max,
            });
            let idx = self.v.ice.len() - 1;
            self.set_ice_status(idx, "active");
            let turn = self.v.combat_turn.to_string();
            let t = if kind == IceKind::Spider {
                self.tx("log.spider_enters", &[("turn", turn)])
            } else {
                self.tx("log.ice_launch", &[("turn", turn), ("ice", name.clone())])
            };
            self.log(t, "bad");
            let t = self.tx("pt.ice_active", &[("ice", name.clone())]);
            self.phase("ice", t);
            self.v.nodes[3].status = "alert".into();
            self.push(1.1, Some(Sfx::Ice));

            if kind == IceKind::Patrol {
                // Patrouillen-IC: Matrixwahrnehmung gegen Logik + Schleicher
                let act = self.tx("act.patrol", &[]);
                let host2 = self.tx("pool.host2", &[("r", self.r.to_string())]);
                let def = self.pool_label("pool.patrol_def", self.d.logic, self.d.sleaze);
                let roll = self.opposed(
                    "ice",
                    &act,
                    host2,
                    self.r * 2,
                    None,
                    def,
                    self.d.logic + self.d.sleaze,
                    !alarm_before,
                );
                if roll.success {
                    self.v.alert = "active".into();
                    let t = self.tx("log.patrol_found", &[]);
                    self.log(t, "bad");
                    let v = self.tx("v.detected", &[]);
                    self.show_roll(roll, v, 1.4);
                } else {
                    let t = self.tx("log.patrol_miss", &[]);
                    self.log(t, "good");
                    let v = self.tx("v.unnoticed", &[]);
                    self.show_roll(roll, v, 1.4);
                }
                self.set_ice_status(idx, "shaken");
                continue;
            }

            let mut exchanges = 0;
            while self.v.ice[idx].hp > 0 {
                exchanges += 1;
                // IC (oder Spinne) greift an: Host × 2 gegen Intuition + Firewall
                let (att_label, att_n) = if kind == IceKind::Spider {
                    (self.tx("pool.spider_att", &[]), self.r + 6)
                } else {
                    (
                        self.tx("pool.host2", &[("r", self.r.to_string())]),
                        self.r * 2,
                    )
                };
                let act = self.tx("act.ice_attack", &[("ice", name.clone())]);
                let def = self.pool_label("pool.ice_def", self.d.intuition, self.d.firewall);
                let roll = self.opposed(
                    "ice",
                    &act,
                    att_label,
                    att_n,
                    None,
                    def,
                    self.d.intuition + self.d.firewall,
                    false,
                );
                if roll.success {
                    let net = roll.net.max(0) as u8;
                    let verdict = self.ice_effect(kind, net);
                    self.show_roll(roll, verdict, 1.5);
                } else {
                    let t = self.tx("log.ice_miss", &[("ice", name.clone())]);
                    self.log(t, "good");
                    let v = self.tx("v.blocked", &[]);
                    self.show_roll(roll, v, 1.3);
                }

                // Decker kontert mit Datenspike
                let wm = self.wound_mod();
                let n = (self.d.cybercombat + self.d.logic + self.hot())
                    .saturating_sub(wm)
                    .max(3);
                let force = exchanges >= 2;
                let (def_label, def_n) = if kind == IceKind::Spider {
                    (self.tx("pool.spider_def", &[]), 4 + self.v.asdf[3] as u32)
                } else {
                    self.host_def(3, "firewall")
                };
                let att_label = self.pool_label("pool.brute", self.d.cybercombat, self.d.logic)
                    + &self.hot_mod()
                    + &self.wound_label(wm);
                let spike = self.tx("act.spike", &[]);
                let roll = if exchanges >= 2 && self.v.edge > 0 && self.rng.gen_bool(0.35) {
                    let t = self.tx("log.edge_spike", &[("decker", self.v.decker.clone())]);
                    self.log(t, "edge");
                    self.edge_roll(&spike, att_label, n, def_label, def_n)
                } else {
                    self.opposed(
                        "decker",
                        &spike,
                        att_label,
                        n,
                        Some(self.d.attack),
                        def_label,
                        def_n,
                        force,
                    )
                };
                self.add_os_hits(roll.defender.as_ref().map_or(0, |d| d.hits));
                if roll.success {
                    let dmg = (roll.net as u32 + self.d.attack) as u8;
                    let ice = &mut self.v.ice[idx];
                    ice.hp = ice.hp.saturating_sub(dmg);
                    let crashed = ice.hp == 0;
                    let n = dmg.to_string();
                    if crashed {
                        let t = if kind == IceKind::Spider {
                            self.set_ice_status(idx, "shaken");
                            self.tx("log.spider_out", &[])
                        } else {
                            self.set_ice_status(idx, "crashed");
                            self.tx(
                                "log.spike_crash",
                                &[
                                    ("n", n.clone()),
                                    ("ice", name.clone()),
                                    ("os", self.v.os.to_string()),
                                ],
                            )
                        };
                        self.log(t, "good");
                        let v = self.tx("v.crash", &[("n", n)]);
                        self.show_roll(roll, v, 1.6);
                    } else {
                        let t =
                            self.tx("log.spike_hit", &[("n", n.clone()), ("ice", name.clone())]);
                        self.log(t, "good");
                        let v = self.tx("v.dmg", &[("n", n)]);
                        self.show_roll(roll, v, 1.5);
                    }
                } else {
                    let t = self.tx("log.spike_fail", &[]);
                    self.log(t, "bad");
                    let v = self.tx("v.no_through", &[]);
                    self.show_roll(roll, v, 1.3);
                }
            }
            self.v.nodes[3].status = "active".into();
        }
        let t = self.tx("log.ice_cleared", &[]);
        self.log(t, "good");
    }

    fn ice_effect(&mut self, kind: IceKind, net: u8) -> String {
        let base: u8 = if self.r >= 6 { 2 } else { 1 };
        let name = self.ice_name(kind);
        match kind {
            IceKind::Killer | IceKind::Spider => {
                let d = self.matrix_damage(base + net);
                let key = if d == 0 { "log.dmg_hold" } else { "log.dmg" };
                let t = self.tx(key, &[("ice", name), ("n", d.to_string())]);
                self.log(t, "bad");
                self.tx("v.matrix_dmg", &[("n", d.to_string())])
            }
            IceKind::Black => {
                let d = self.matrix_damage(base + net);
                let s = self.stun_damage(if self.d.hot { base + net } else { net.max(1) });
                self.v.link_locked = true;
                let t = self.tx(
                    "log.dmg_black",
                    &[("n", d.to_string()), ("s", s.to_string())],
                );
                self.log(t, "bad");
                self.tx("v.black", &[])
            }
            IceKind::Blaster => {
                let d = self.matrix_damage(base + net);
                let s = self.stun_damage(net.max(1));
                self.v.link_locked = true;
                let t = self.tx(
                    "log.dmg_blaster",
                    &[("n", d.to_string()), ("s", s.to_string())],
                );
                self.log(t, "bad");
                self.tx("v.blaster", &[])
            }
            IceKind::Tar => {
                self.v.link_locked = true;
                let t = self.tx("log.dmg_tar", &[]);
                self.log(t, "bad");
                self.tx("v.tar", &[])
            }
            IceKind::Acid => {
                self.d.firewall = self.d.firewall.saturating_sub(1).max(1);
                let t = self.tx("log.dmg_acid", &[("n", self.d.firewall.to_string())]);
                self.log(t, "bad");
                self.tx("v.acid", &[])
            }
            IceKind::Track => {
                self.add_os(2 + net as u32);
                let t = self.tx("log.dmg_track", &[("os", self.v.os.to_string())]);
                self.log(t, "bad");
                self.tx("v.track", &[])
            }
            IceKind::Crash => {
                if self.v.marks > 0 {
                    self.v.marks -= 1;
                }
                let t = self.tx("log.dmg_crash", &[("n", self.v.marks.to_string())]);
                self.log(t, "bad");
                self.tx("v.crash_ice", &[])
            }
            IceKind::Patrol => String::new(),
        }
    }

    fn search_and_crack(&mut self) {
        self.set_node(4, "active");
        let t = self.tx("pt.search", &[]);
        self.phase("search", t);
        let n = self.d.computer + self.d.intuition + self.hot();
        let label =
            self.pool_label("pool.perception", self.d.computer, self.d.intuition) + &self.hot_mod();
        let mut a = self.pool(label, n, Some(self.d.dataproc), false);
        if a.hits < 2 {
            for d in a
                .dice
                .iter_mut()
                .filter(|d| **d < 5)
                .take(2 - a.hits as usize)
            {
                *d = 5;
            }
            a.hits = count_hits(&a.dice).min(self.d.dataproc);
        }
        let (g, c) = glitches(&a.dice, a.hits);
        self.roll_id += 1;
        let roll = Roll {
            id: self.roll_id,
            actor: "decker".into(),
            action: self.tx("act.search", &[]),
            attacker: a,
            defender: None,
            threshold: Some(2),
            net: 0,
            glitch: g,
            critical: c,
            edge: false,
            success: true,
            verdict: String::new(),
        };
        let folder = self
            .lx
            .list("folders")
            .choose(self.rng)
            .cloned()
            .unwrap_or_default();
        let t = self.tx(
            "log.search_ok",
            &[("folder", folder), ("n", self.v.participants.to_string())],
        );
        self.log(t, "good");
        let v = self.tx("v.found_file", &[]);
        self.show_roll(roll, v, 1.5);

        self.set_node(5, "active");
        // Datenbombe?
        if self.rng.gen_bool(0.45) {
            let bomb = self.rng.gen_range(2..=self.r.max(3));
            let b = bomb.to_string();
            let t = self.tx("pt.bomb", &[("n", b.clone())]);
            self.phase("bomb", t);
            let t = self.tx("log.bomb_found", &[("n", b.clone())]);
            self.log(t, "warn");
            self.push(1.0, Some(Sfx::Ice));
            let act = self.tx("act.defuse", &[]);
            let label = self.pool_label("pool.defuse", self.d.software, self.d.logic);
            let def = self.tx("pool.bomb", &[("n", b)]);
            let roll = self.opposed(
                "decker",
                &act,
                label,
                self.d.software + self.d.logic + self.hot(),
                Some(self.d.dataproc),
                def,
                bomb * 2,
                false,
            );
            if roll.success {
                let t = self.tx("log.bomb_defused", &[]);
                self.log(t, "good");
                let v = self.tx("v.defused", &[]);
                self.show_roll(roll, v, 1.5);
            } else {
                let boom: u8 = (0..bomb)
                    .map(|_| self.rng.gen_range(1..=6u8))
                    .filter(|&x| x >= 5)
                    .count() as u8
                    + 1;
                let d = self.matrix_damage(boom);
                let t = if d > 0 {
                    self.tx("log.bomb_boom", &[("n", d.to_string())])
                } else {
                    self.tx("log.bomb_boom_hold", &[])
                };
                self.log(t, "bad");
                let v = self.tx("v.boom", &[]);
                self.show_roll(roll, v, 1.6);
            }
        }

        // Absturz-IC kann Marken gelöscht haben → nachsetzen
        while self.v.marks < 3 {
            let (def_label, def_n) = self.host_def(3, "firewall");
            let act = self.tx("act.hack", &[]);
            let label = self.pool_label("pool.hack", self.d.hacking, self.d.logic);
            let roll = self.opposed(
                "decker",
                &act,
                label,
                self.d.hacking + self.d.logic + self.hot(),
                Some(self.d.sleaze),
                def_label,
                def_n,
                true,
            );
            self.add_os_hits(roll.defender.as_ref().map_or(0, |d| d.hits));
            self.v.marks += 1;
            let m = self.v.marks.to_string();
            let t = self.tx("log.remark", &[("n", m.clone())]);
            self.log(t, "good");
            let v = self.tx("v.remark", &[("n", m)]);
            self.show_roll(roll, v, 1.3);
        }

        let t = self.tx("pt.crack", &[]);
        self.phase("crack", t);
        let (att_label, n) = (
            self.pool_label("pool.hack", self.d.hacking, self.d.logic) + &self.hot_mod(),
            self.d.hacking + self.d.logic + self.hot(),
        );
        let def_label = self.tx("pool.crypt", &[("r", self.r.to_string())]);
        let act = self.tx("act.crack", &[]);
        let first = self.opposed(
            "decker",
            &act,
            att_label.clone(),
            n,
            Some(self.d.attack),
            def_label.clone(),
            self.r * 2,
            false,
        );
        self.add_os_hits(first.defender.as_ref().map_or(0, |d| d.hits));
        if !first.success {
            let t = self.tx("log.crack_hold", &[]);
            self.log(t, "bad");
            let v = self.tx("v.holds", &[]);
            self.show_roll(first, v, 1.3);
            let roll = if self.v.edge > 0 {
                let t = self.tx("log.edge_crack", &[("decker", self.v.decker.clone())]);
                self.log(t, "edge");
                self.edge_roll(&act, att_label, n, def_label, self.r * 2)
            } else {
                self.opposed(
                    "decker",
                    &act,
                    att_label,
                    n,
                    Some(self.d.attack),
                    def_label,
                    self.r * 2,
                    true,
                )
            };
            self.add_os_hits(roll.defender.as_ref().map_or(0, |d| d.hits));
            self.file_secured = true;
            let t = self.tx("log.cracked", &[("os", self.v.os.to_string())]);
            self.log(t, "good");
            let v = self.tx("v.cracked", &[]);
            self.show_roll(roll, v, 1.6);
        } else {
            self.file_secured = true;
            let t = self.tx("log.cracked", &[("os", self.v.os.to_string())]);
            self.log(t, "good");
            let v = self.tx("v.cracked", &[]);
            self.show_roll(first, v, 1.6);
        }
    }

    fn decode(&mut self, participants: &[String], winners: &[String]) {
        let t = self.tx("pt.decode_start", &[]);
        self.phase("decode", t);
        self.v.nodes[5].status = "target".into();
        let is_winner = |n: &str| winners.iter().any(|w| w == n);
        // Alle Gewinner (max. 24) plus Nieten, damit etwas rausfliegen kann
        let shown_winners: Vec<String> = winners.iter().take(24).cloned().collect();
        let fill = (shown_winners.len() + 4).clamp(12, 24) - shown_winners.len();
        let mut others: Vec<String> = participants
            .iter()
            .filter(|p| !is_winner(p))
            .cloned()
            .collect();
        others.shuffle(self.rng);
        others.truncate(fill);
        let mut names = others;
        names.extend(shown_winners);
        names.shuffle(self.rng);
        self.v.candidates = names
            .iter()
            .map(|n| Candidate {
                name: n.clone(),
                state: "offen".into(),
            })
            .collect();
        let t = self.tx("log.register", &[("n", participants.len().to_string())]);
        self.log(t, "info");
        self.push(1.2, Some(Sfx::Dice));

        let losers: Vec<usize> = (0..self.v.candidates.len())
            .filter(|&i| !is_winner(&self.v.candidates[i].name))
            .collect();
        let steps = losers.len().clamp(1, 4);
        let mut order = losers.clone();
        order.shuffle(self.rng);
        let chunk = order.len().div_ceil(steps).max(1);
        for part in order.chunks(chunk) {
            for &i in part {
                self.v.candidates[i].state = "raus".into();
            }
            let left = self
                .v
                .candidates
                .iter()
                .filter(|c| c.state == "offen")
                .count();
            let t = if left == 1 {
                self.tx("pt.decode_left1", &[])
            } else {
                self.tx("pt.decode_left", &[("n", left.to_string())])
            };
            self.phase("decode", t);
            self.push(0.9, Some(Sfx::Click));
        }
        for c in self.v.candidates.iter_mut() {
            if is_winner(&c.name) {
                c.state = "gewinner".into();
            }
        }
    }

    fn jack_out(&mut self) {
        self.set_node(6, "active");
        let t = self.tx("pt.jackout", &[]);
        self.phase("jackout", t);
        let converge = self.v.os >= 32 && self.rng.gen_bool(0.3);
        if self.v.link_locked {
            let (def_label, def_n) = self.host_def(0, "attack");
            let act = self.tx("act.jackout", &[]);
            let label = self.pool_label("pool.jackout", self.d.hacking, self.d.willpower);
            let roll = self.opposed(
                "decker",
                &act,
                label,
                self.d.hacking + self.d.willpower,
                None,
                def_label,
                def_n,
                true,
            );
            let t = self.tx("log.unlock", &[]);
            self.log(t, "good");
            let v = self.tx("v.out", &[]);
            self.show_roll(roll, v, 1.4);
        }
        if converge {
            self.v.os = OS_MAX;
            self.v.converged = true;
            let t = self.tx("log.converge", &[("max", OS_MAX.to_string())]);
            self.log(t, "bad");
            let t = self.tx("log.converge2", &[]);
            self.log(t, "edge");
            self.v.matrix_cm = self.v.matrix_max;
            let t = self.tx("pt.converge", &[]);
            self.phase("converge", t);
            self.push(1.4, Some(Sfx::Glitch));
        } else {
            let t = self.tx(
                "log.out",
                &[("os", self.v.os.to_string()), ("max", OS_MAX.to_string())],
            );
            self.log(t, "good");
            self.push(1.0, Some(Sfx::Click));
        }
    }
}

pub struct RunSetup<'a> {
    pub participants: &'a [String],
    /// Bereits fair gezogene Gewinner (in Reihenfolge Preis 1, 2, …)
    pub winners: &'a [String],
    pub decker: &'a str,
    pub duration: f64,
    /// Begriffe des gewählten Designs
    pub lexicon: &'a Lexicon,
}

/// Erzeugt den kompletten Run. Das letzte Frame enthält die Gewinner.
pub fn generate<R: Rng>(rng: &mut R, s: RunSetup<'_>) -> Vec<Frame> {
    let r: u32 = rng.gen_range(4..=7);
    let mut attrs = [r as u8, r as u8 + 1, r as u8 + 2, r as u8 + 3];
    attrs.shuffle(rng);
    let deck_rating: u32 = rng.gen_range(4..=6);
    let mut deck_attrs = [
        deck_rating + 1,
        deck_rating,
        deck_rating - 1,
        deck_rating - 2,
    ];
    deck_attrs.shuffle(rng);
    let hot = rng.gen_bool(0.6);
    let d = Decker {
        logic: 6,
        intuition: 5,
        willpower: 4,
        hacking: rng.gen_range(5..=7),
        cybercombat: rng.gen_range(5..=6),
        computer: 5,
        software: 5,
        attack: deck_attrs[0],
        sleaze: deck_attrs[1],
        dataproc: deck_attrs[2],
        firewall: deck_attrs[3].max(2),
        hot,
    };
    let edge_max: u8 = rng.gen_range(3..=5);
    let lx = s.lexicon;
    let pick = |rng: &mut R, key: &str| lx.list(key).choose(rng).cloned().unwrap_or_default();
    let host = pick(rng, "hosts");
    let host_sculpt = pick(rng, "sculpts");
    let deck = pick(rng, "decks");
    let v = RunView {
        host,
        host_sculpt,
        host_rating: r as u8,
        asdf: attrs,
        decker: s.decker.to_uppercase(),
        deck,
        sim: lx.t(if hot { "sim.hot" } else { "sim.cold" }, &[]),
        hot,
        os: 0,
        os_max: OS_MAX,
        marks: 0,
        matrix_cm: 0,
        matrix_max: (8 + deck_rating.div_ceil(2)) as u8,
        stun: 0,
        stun_max: 10,
        edge: edge_max,
        edge_max,
        alert: "none".into(),
        link_locked: false,
        combat_turn: 0,
        ice: vec![],
        nodes: (0..7)
            .map(|i| NodeView {
                label: lx.list("nodes").get(i).cloned().unwrap_or_default(),
                status: "locked".into(),
            })
            .collect(),
        node: 0,
        phase: String::new(),
        phase_key: "init".into(),
        phase_text: String::new(),
        roll: None,
        log: vec![],
        candidates: vec![],
        participants: s.participants.len(),
        winners: vec![],
        converged: false,
        overall: 0.0,
        done: false,
    };
    let mut g = Gen {
        rng,
        lx,
        v,
        d,
        r,
        frames: vec![],
        roll_id: 0,
        file_secured: false,
        os_warned: false,
    };

    let ice_count = if s.duration < 20.0 {
        2
    } else if s.duration < 35.0 {
        3
    } else {
        4
    };
    let ice_count = if r >= 6 { ice_count } else { ice_count.min(3) };

    g.jack_in();
    g.marks(3);
    g.ice_fights(ice_count);
    g.search_and_crack();
    g.decode(s.participants, s.winners);
    g.jack_out();

    // Abschluss
    g.v.winners = s.winners.to_vec();
    g.v.done = true;
    g.v.roll = None;
    g.set_node(6, "done");
    for n in g.v.nodes.iter_mut() {
        n.status = "done".into();
    }
    g.v.nodes[5].status = "target".into();
    let t = g.tx("pt.done", &[]);
    g.phase("done", t);
    let names: Vec<String> = s.winners.iter().take(6).map(|w| format!("@{w}")).collect();
    let more = s.winners.len().saturating_sub(6);
    let list = if more > 0 {
        g.tx(
            "log.and_more",
            &[("list", names.join(", ")), ("n", more.to_string())],
        )
    } else {
        names.join(", ")
    };
    let t = if s.winners.len() == 1 {
        g.tx("log.win1", &[("list", list)])
    } else {
        g.tx(
            "log.winN",
            &[("list", list), ("n", s.winners.len().to_string())],
        )
    };
    g.log(t, "good");
    g.push(0.0, Some(Sfx::Win1));

    // Zeitachse auf die gewünschte Dauer skalieren
    let duration = s.duration.max(8.0);
    let n = g.frames.len();
    let total: f64 = g.frames[..n - 1]
        .iter()
        .map(|f| f.0)
        .sum::<f64>()
        .max(0.001);
    let mut out = Vec::with_capacity(n);
    let mut cum = 0.0;
    for (i, (w, mut view, sfx)) in g.frames.into_iter().enumerate() {
        let t = if i == n - 1 {
            duration
        } else {
            cum / total * (duration - 0.8)
        };
        view.overall = if i == n - 1 {
            1.0
        } else {
            (t / duration) as f32
        };
        out.push(Frame { t, view, sfx });
        cum += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn runs_always_finish_with_winner() {
        let people: Vec<String> = (0..37).map(|i| format!("user{i}")).collect();
        for seed in 0..400u64 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let winner = people[(seed as usize) % people.len()].clone();
            let winners = vec![winner.clone()];
            let dur = [10.0, 25.0, 45.0][(seed % 3) as usize];
            let frames = generate(
                &mut rng,
                RunSetup {
                    participants: &people,
                    winners: &winners,
                    decker: "areadox",
                    duration: dur,
                    lexicon: &Lexicon::standard(),
                },
            );
            let last = frames.last().unwrap();
            assert_eq!(last.view.winners, winners);
            assert!(last.view.done);
            assert!((last.t - dur).abs() < 1e-9);
            assert!(frames.windows(2).all(|w| w[0].t <= w[1].t), "Zeit monoton");
            assert!(
                frames.len() > 10 && frames.len() < 80,
                "frames={}",
                frames.len()
            );
            assert_eq!(last.view.marks, 3);
            assert!(last.view.os <= OS_MAX);
            assert!(last
                .view
                .candidates
                .iter()
                .any(|c| c.name == winner && c.state == "gewinner"));
            for f in &frames[..frames.len() - 1] {
                assert!(f.view.matrix_cm < f.view.matrix_max || f.view.converged);
                assert!(f.view.os < OS_MAX || f.view.converged);
                if let Some(r) = &f.view.roll {
                    assert!(r.attacker.dice.iter().all(|d| (1..=6).contains(d)));
                    assert_eq!(r.attacker.hits <= count_hits(&r.attacker.dice), true);
                }
            }
        }
    }

    #[test]
    fn single_participant() {
        let people = vec!["solo".to_string()];
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let frames = generate(
            &mut rng,
            RunSetup {
                participants: &people,
                winners: &["solo".to_string()],
                decker: "x",
                duration: 15.0,
                lexicon: &Lexicon::standard(),
            },
        );
        assert_eq!(
            frames.last().unwrap().view.winners,
            vec!["solo".to_string()]
        );
    }

    #[test]
    fn several_winners() {
        let people: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
        for n in [2usize, 5, 30, 40] {
            let winners: Vec<String> = people.iter().take(n).cloned().collect();
            let mut rng = rand::rngs::StdRng::seed_from_u64(n as u64);
            let frames = generate(
                &mut rng,
                RunSetup {
                    participants: &people,
                    winners: &winners,
                    decker: "x",
                    duration: 20.0,
                    lexicon: &Lexicon::standard(),
                },
            );
            let last = &frames.last().unwrap().view;
            assert_eq!(last.winners, winners);
            let won: Vec<&Candidate> = last
                .candidates
                .iter()
                .filter(|c| c.state == "gewinner")
                .collect();
            assert_eq!(won.len(), n.min(24));
            assert!(last.candidates.iter().all(|c| c.state != "offen"));
            assert!(last.candidates.len() <= 24);
        }
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    use rand::SeedableRng;

    /// Jedes eingebaute Design liefert vollständige Texte ohne offene Platzhalter.
    #[test]
    fn all_themes_render_clean_runs() {
        let reg = crate::themes::registry();
        let people: Vec<String> = (0..30).map(|i| format!("p{i}")).collect();
        for (id, ..) in reg.list() {
            let theme = reg.get(&id);
            for seed in 0..60u64 {
                let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                let winners: Vec<String> = people
                    .iter()
                    .take(1 + (seed % 9) as usize)
                    .cloned()
                    .collect();
                let frames = generate(
                    &mut rng,
                    RunSetup {
                        participants: &people,
                        winners: &winners,
                        decker: "streamer",
                        duration: [10.0, 25.0, 45.0][(seed % 3) as usize],
                        lexicon: &theme.lexicon,
                    },
                );
                let last = &frames.last().unwrap().view;
                let mut texts: Vec<String> = last.log.iter().map(|l| l.text.clone()).collect();
                for f in &frames {
                    texts.push(f.view.phase.clone());
                    texts.push(f.view.phase_text.clone());
                    texts.push(f.view.sim.clone());
                    texts.extend(f.view.nodes.iter().map(|n| n.label.clone()));
                    texts.extend(
                        f.view
                            .ice
                            .iter()
                            .flat_map(|i| [i.name.clone(), i.status_text.clone()]),
                    );
                    if let Some(r) = &f.view.roll {
                        texts.extend([
                            r.action.clone(),
                            r.verdict.clone(),
                            r.attacker.label.clone(),
                        ]);
                        texts.extend(r.defender.iter().map(|d| d.label.clone()));
                    }
                }
                for t in &texts {
                    assert!(
                        !t.contains('{') && !t.contains('}'),
                        "{id}: offener Platzhalter in „{t}“"
                    );
                    assert!(
                        !t.contains('.')
                            || !t.starts_with(|c: char| c.is_ascii_lowercase())
                            || t.contains(' '),
                        "{id}: Schlüssel statt Text: {t}"
                    );
                }
                assert_eq!(last.phase_key, "done");
                assert!(!last.phase.is_empty(), "{id}: Phasenname");
                assert_eq!(last.nodes.len(), 7);
                assert!(
                    last.nodes.iter().all(|n| !n.label.is_empty()),
                    "{id}: Knotennamen"
                );
            }
        }
    }
}

#[cfg(test)]
mod print {
    use super::*;
    use rand::SeedableRng;
    #[test]
    #[ignore]
    fn print_run() {
        let people: Vec<String> = (0..9).map(|i| format!("user{i}")).collect();
        let seed: u64 = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let frames = generate(
            &mut rng,
            RunSetup {
                participants: &people,
                winners: &["user3".to_string()],
                decker: "areadox",
                duration: 30.0,
                lexicon: &Lexicon::standard(),
            },
        );
        for f in &frames {
            let r = f
                .view
                .roll
                .as_ref()
                .map(|r| {
                    format!(
                        "  [{}: {}W6 {:?} → {} vs {} | {}]",
                        r.action,
                        r.attacker.dice.len(),
                        r.attacker.dice,
                        r.attacker.hits,
                        r.defender.as_ref().map_or(
                            format!("S{}", r.threshold.unwrap_or(0)),
                            |d| format!("{}W6→{}", d.dice.len(), d.hits)
                        ),
                        r.verdict
                    )
                })
                .unwrap_or_default();
            println!(
                "{:5.1}s OW{:2} M{} CM{}/{} ST{}/{} E{} | {}{}",
                f.t,
                f.view.os,
                f.view.marks,
                f.view.matrix_cm,
                f.view.matrix_max,
                f.view.stun,
                f.view.stun_max,
                f.view.edge,
                f.view.phase_text,
                r
            );
        }
        println!("--- LOG");
        for l in &frames.last().unwrap().view.log {
            println!("[{}] {}", l.tone, l.text);
        }
    }
}
