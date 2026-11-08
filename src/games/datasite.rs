//! Data-Site hacken wie in EVE Online: Ein Virus (Kohärenz/Stärke) öffnet
//! Knoten auf einem Sechseck-Raster, bekämpft Verteidigungs-Subsysteme,
//! nutzt Hilfsprogramme und knackt am Ende den Systemkern.

use super::Stage;
use crate::matrix::{Frame, RunSetup};
use crate::sound::Sfx;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

const R: i32 = 3;
const DEFENSES: [&str; 4] = ["firewall", "antivirus", "restoration", "suppressor"];
const UTILITIES: [&str; 4] = [
    "kernel_rot",
    "self_repair",
    "poly_shield",
    "secondary_vector",
];

#[derive(Clone)]
struct Cell {
    q: i32,
    r: i32,
    kind: &'static str,
    open: bool,
    dead: bool,
    coh: i32,
    coh_max: i32,
    str: i32,
}

#[derive(Serialize, Clone)]
struct NodeView {
    q: i32,
    r: i32,
    /// hidden | empty | core | cache | firewall | antivirus | restoration | suppressor | kernel_rot | …
    kind: String,
    /// hidden | blocked | open | alive | dead | taken
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coh: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coh_max: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    str: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize, Clone)]
struct Scene {
    nodes: Vec<NodeView>,
    coh: i32,
    coh_max: i32,
    str: i32,
    shield: u8,
    dot: u8,
    /// gehaltene Hilfsprogramme (Anzeigenamen)
    slots: Vec<String>,
    analyzer: String,
    ship: String,
    /// Knoten, auf dem der Virus gerade arbeitet
    cursor: usize,
    /// angegriffener Knoten
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<usize>,
    /// Effekt für die Animation: open | hit | back | kill | heal | util | boom | ""
    fx: String,
    /// zählt hoch, damit gleiche Effekte nacheinander neu abgespielt werden
    fx_id: u32,
    core_open: bool,
}

fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dq, dr) = (a.0 - b.0, a.1 - b.1);
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

struct Hack<'a, R: Rng> {
    st: Stage<'a, R>,
    cells: Vec<Cell>,
    core: usize,
    coh: i32,
    coh_max: i32,
    str_base: i32,
    shield: u8,
    dot: u8,
    regen: u8,
    slots: Vec<&'static str>,
    taken: Vec<usize>,
    analyzer: String,
    ship: String,
    cursor: usize,
    target: Option<usize>,
    fx: &'static str,
    fx_id: u32,
    core_open: bool,
}

impl<'a, R: Rng> Hack<'a, R> {
    fn pos(&self, i: usize) -> (i32, i32) {
        (self.cells[i].q, self.cells[i].r)
    }

    fn neighbours(&self, i: usize) -> Vec<usize> {
        let p = self.pos(i);
        (0..self.cells.len())
            .filter(|&j| dist(p, self.pos(j)) == 1)
            .collect()
    }

    fn is_defense(&self, i: usize) -> bool {
        DEFENSES.contains(&self.cells[i].kind) || self.cells[i].kind == "core"
    }

    fn alive_def(&self, i: usize) -> bool {
        self.cells[i].open && !self.cells[i].dead && self.is_defense(i)
    }

    /// Verdeckte Knoten neben einer lebenden Verteidigung sind gesperrt.
    fn blocked(&self, i: usize) -> bool {
        !self.cells[i].open && self.neighbours(i).into_iter().any(|j| self.alive_def(j))
    }

    fn name(&self, kind: &str) -> String {
        self.st.tx(&format!("ds.kind.{kind}"), &[])
    }

    fn strength(&self) -> i32 {
        let suppressed =
            (0..self.cells.len()).any(|i| self.alive_def(i) && self.cells[i].kind == "suppressor");
        if suppressed {
            (self.str_base - 10).max(20)
        } else {
            self.str_base
        }
    }

    fn scene(&mut self) -> Scene {
        let nodes = (0..self.cells.len())
            .map(|i| {
                let c = &self.cells[i];
                let state = if !c.open {
                    if self.blocked(i) {
                        "blocked"
                    } else {
                        "hidden"
                    }
                } else if self.taken.contains(&i) {
                    "taken"
                } else if c.dead {
                    "dead"
                } else if self.is_defense(i) {
                    "alive"
                } else {
                    "open"
                };
                let fighter = c.open && self.is_defense(i);
                NodeView {
                    q: c.q,
                    r: c.r,
                    kind: if c.open {
                        c.kind.into()
                    } else {
                        "hidden".into()
                    },
                    state: state.into(),
                    num: (c.open && c.kind == "empty")
                        .then(|| dist(self.pos(i), self.pos(self.core))),
                    coh: fighter.then_some(c.coh.max(0)),
                    coh_max: fighter.then_some(c.coh_max),
                    str: fighter.then_some(c.str),
                    name: (c.open && c.kind != "empty").then(|| self.name(c.kind)),
                }
            })
            .collect();
        self.fx_id += 1;
        Scene {
            nodes,
            coh: self.coh,
            coh_max: self.coh_max,
            str: self.strength(),
            shield: self.shield,
            dot: self.dot,
            slots: self.slots.iter().map(|k| self.name(k)).collect(),
            analyzer: self.analyzer.clone(),
            ship: self.ship.clone(),
            cursor: self.cursor,
            target: self.target,
            fx: self.fx.into(),
            fx_id: self.fx_id,
            core_open: self.core_open,
        }
    }

    fn frame(&mut self, w: f64, sfx: Option<Sfx>, fx: &'static str) {
        self.fx = fx;
        let sc = self.scene();
        self.st.push(w, sfx, &sc);
    }

    fn take_slot(&mut self, kind: &'static str) -> bool {
        if let Some(p) = self.slots.iter().position(|k| *k == kind) {
            self.slots.remove(p);
            true
        } else {
            false
        }
    }

    /// Schaden am Virus – der Virus stirbt nie (der Run ist Inszenierung).
    fn hurt(&mut self, dmg: i32) -> i32 {
        if self.coh - dmg < 12 && self.take_slot("self_repair") {
            self.regen = 3;
            let n = 20;
            self.coh = (self.coh + n).min(self.coh_max);
            self.st
                .log("ds.log.repair", &[("n", n.to_string())], "edge");
        }
        let d = dmg.min(self.coh - 6).max(0);
        self.coh -= d;
        if self.coh <= self.coh_max / 4 {
            self.st
                .log("ds.log.low", &[("n", self.coh.to_string())], "warn");
        }
        d
    }

    fn open(&mut self, i: usize) {
        self.cursor = i;
        self.cells[i].open = true;
        let kind = self.cells[i].kind;
        match kind {
            "empty" => {
                let n = dist(self.pos(i), self.pos(self.core));
                self.st.log("ds.log.empty", &[("n", n.to_string())], "dim");
                self.frame(0.55, Some(Sfx::Click), "open");
            }
            "cache" => {
                let roll: f64 = self.st.rng.gen();
                if roll < 0.5 && self.slots.len() < 3 {
                    let u = *UTILITIES.choose(self.st.rng).unwrap();
                    self.slots.push(u);
                    self.taken.push(i);
                    let name = self.name(u);
                    self.st.log("ds.log.cache_util", &[("name", name)], "good");
                    self.frame(0.8, Some(Sfx::Click), "util");
                } else if roll < 0.75 {
                    self.st.log("ds.log.cache_empty", &[], "dim");
                    self.taken.push(i);
                    self.frame(0.6, Some(Sfx::Click), "open");
                } else {
                    let k = *["firewall", "antivirus"].choose(self.st.rng).unwrap();
                    self.arm(i, k);
                    let name = self.name(k);
                    self.st.log("ds.log.cache_def", &[("name", name)], "bad");
                    self.frame(0.9, Some(Sfx::Ice), "open");
                    self.fight(i);
                }
            }
            k if UTILITIES.contains(&k) => {
                if self.slots.len() < 3 {
                    self.slots.push(k);
                }
                self.taken.push(i);
                let name = self.name(k);
                self.st.log("ds.log.util", &[("name", name)], "good");
                self.frame(0.8, Some(Sfx::Click), "util");
            }
            "core" => {
                self.st.log("ds.log.core_found", &[], "warn");
                self.st.phase("core", "ds.pt.core", &[]);
                self.frame(1.0, Some(Sfx::Ice), "open");
                self.fight(i);
            }
            _ => {
                let c = &self.cells[i];
                let (coh, st) = (c.coh, c.str);
                let name = self.name(kind);
                self.st.log(
                    "ds.log.def",
                    &[
                        ("name", name.clone()),
                        ("coh", coh.to_string()),
                        ("str", st.to_string()),
                    ],
                    "bad",
                );
                if kind == "suppressor" {
                    let n = self.strength();
                    self.st
                        .log("ds.log.suppress", &[("n", n.to_string())], "warn");
                }
                self.frame(0.9, Some(Sfx::Ice), "open");
                self.fight(i);
            }
        }
    }

    fn arm(&mut self, i: usize, kind: &'static str) {
        let rng = &mut self.st.rng;
        let (coh, st) = match kind {
            "firewall" => (rng.gen_range(80..=110), rng.gen_range(10..=20)),
            "antivirus" => (rng.gen_range(40..=60), rng.gen_range(30..=40)),
            "restoration" => (rng.gen_range(60..=80), 10),
            "suppressor" => (rng.gen_range(50..=70), 15),
            _ => (rng.gen_range(90..=120), rng.gen_range(10..=20)),
        };
        let c = &mut self.cells[i];
        c.kind = kind;
        c.coh = coh;
        c.coh_max = coh;
        c.str = st;
    }

    fn fight(&mut self, t: usize) {
        self.target = Some(t);
        let tname = self.name(self.cells[t].kind);
        let is_core = self.cells[t].kind == "core";
        if !is_core {
            self.st
                .phase("fight", "ds.pt.fight", &[("target", tname.clone())]);
        }
        // Hilfsprogramme einsetzen
        if self.cells[t].coh >= 70 && self.take_slot("kernel_rot") {
            self.cells[t].coh /= 2;
            self.st
                .log("ds.log.rot", &[("name", tname.clone())], "edge");
            self.frame(0.8, Some(Sfx::Glitch), "util");
        }
        if self.cells[t].coh >= 50 && self.dot == 0 && self.take_slot("secondary_vector") {
            self.dot = 3;
            self.st
                .log("ds.log.vector", &[("name", tname.clone())], "edge");
            self.frame(0.7, Some(Sfx::Click), "util");
        }
        if self.cells[t].str >= 30 && self.shield == 0 && self.take_slot("poly_shield") {
            self.shield = 2;
            self.st.log("ds.log.shield_up", &[], "edge");
            self.frame(0.7, Some(Sfx::Click), "util");
        }
        let mut rounds = 0;
        loop {
            rounds += 1;
            let mut hit = self.strength();
            if rounds >= 7 {
                hit = hit.max(self.cells[t].coh);
            }
            self.cells[t].coh -= hit;
            let mut left = self.cells[t].coh.max(0);
            self.st.log(
                "ds.log.hit",
                &[
                    ("name", tname.clone()),
                    ("n", hit.to_string()),
                    ("left", left.to_string()),
                ],
                "good",
            );
            if self.dot > 0 && left > 0 {
                self.dot -= 1;
                self.cells[t].coh -= 20;
                left = self.cells[t].coh.max(0);
                self.st.log(
                    "ds.log.vector_hit",
                    &[("name", tname.clone()), ("n", "20".into())],
                    "edge",
                );
            }
            if left == 0 {
                self.cells[t].dead = true;
                if is_core {
                    self.core_open = true;
                    self.st.log("ds.log.core_down", &[], "good");
                    self.frame(1.6, Some(Sfx::Win1), "boom");
                } else {
                    self.st
                        .log("ds.log.kill", &[("name", tname.clone())], "good");
                    if self.cells[t].kind == "suppressor" {
                        let n = self.strength();
                        self.st
                            .log("ds.log.suppress_off", &[("n", n.to_string())], "good");
                    }
                    self.frame(1.1, Some(Sfx::Dice), "kill");
                }
                break;
            }
            self.frame(0.85, Some(Sfx::Dice), "hit");
            // Gegenschlag
            let back = self.cells[t].str;
            if self.shield > 0 {
                self.shield -= 1;
                self.st.log("ds.log.shield", &[], "edge");
                self.frame(0.7, Some(Sfx::Click), "util");
            } else {
                let d = self.hurt(back);
                self.st.log(
                    "ds.log.back",
                    &[("name", tname.clone()), ("n", d.to_string())],
                    "bad",
                );
                self.frame(0.85, Some(Sfx::Glitch), "back");
            }
            if self.regen > 0 {
                self.regen -= 1;
                self.coh = (self.coh + 10).min(self.coh_max);
                self.st.log("ds.log.repair", &[("n", "10".into())], "edge");
            }
            // Wiederherstellungsknoten heilen andere Verteidigungen
            let healers: Vec<usize> = (0..self.cells.len())
                .filter(|&i| i != t && self.alive_def(i) && self.cells[i].kind == "restoration")
                .collect();
            if !healers.is_empty() && self.cells[t].coh < self.cells[t].coh_max {
                let n = 8 * healers.len() as i32;
                self.cells[t].coh = (self.cells[t].coh + n).min(self.cells[t].coh_max);
                self.st.log(
                    "ds.log.restore",
                    &[("name", tname.clone()), ("n", n.to_string())],
                    "bad",
                );
            }
        }
        self.target = None;
        self.st.phase("explore", "ds.pt.explore", &[]);
    }
}

pub(super) fn run<R: Rng>(rng: &mut R, s: &RunSetup<'_>) -> Vec<Frame> {
    let mut st = Stage::new(rng, s, "datasite");
    st.v.host = st.pick("sites");
    let analyzer = st.pick("analyzers");
    let ship = st.pick("ships");
    let mut cells = vec![];
    for q in -R..=R {
        for r in -R..=R {
            if (q + r).abs() <= R {
                cells.push(Cell {
                    q,
                    r,
                    kind: "empty",
                    open: false,
                    dead: false,
                    coh: 0,
                    coh_max: 0,
                    str: 0,
                });
            }
        }
    }
    let n = cells.len();
    let p = |i: usize, c: &Vec<Cell>| (c[i].q, c[i].r);
    let edge: Vec<usize> = (0..n)
        .filter(|&i| dist(p(i, &cells), (0, 0)) == R)
        .collect();
    let start = *edge.choose(st.rng).unwrap();
    let far: Vec<usize> = (0..n)
        .filter(|&i| dist(p(i, &cells), p(start, &cells)) >= 5)
        .collect();
    let core = *far.choose(st.rng).unwrap();
    // kürzester Weg (für die Platzierung einer Verteidigung vor dem Kern)
    let mut path = vec![start];
    while *path.last().unwrap() != core {
        let cur = *path.last().unwrap();
        let mut next: Vec<usize> = (0..n)
            .filter(|&j| dist(p(cur, &cells), p(j, &cells)) == 1)
            .filter(|&j| {
                dist(p(j, &cells), p(core, &cells)) < dist(p(cur, &cells), p(core, &cells))
            })
            .collect();
        next.shuffle(st.rng);
        path.push(next[0]);
    }
    let mut free: Vec<usize> = (0..n).filter(|&i| i != start && i != core).collect();
    free.shuffle(st.rng);
    let guard = path[path.len() - 2];
    let n_def = if s.duration < 20.0 {
        2
    } else if s.duration < 40.0 {
        3
    } else {
        4
    };
    let mut hack = Hack {
        st,
        cells,
        core,
        coh: 0,
        coh_max: 0,
        str_base: 0,
        shield: 0,
        dot: 0,
        regen: 0,
        slots: vec![],
        taken: vec![],
        analyzer,
        ship,
        cursor: start,
        target: None,
        fx: "",
        fx_id: 0,
        core_open: false,
    };
    hack.arm(core, "core");
    hack.cells[core].kind = "core";
    let guard_kind = *["firewall", "antivirus"].choose(hack.st.rng).unwrap();
    hack.arm(guard, guard_kind);
    free.retain(|&i| i != guard);
    let mut it = free.into_iter();
    let mut kinds: Vec<&'static str> = DEFENSES.to_vec();
    kinds.shuffle(hack.st.rng);
    for k in kinds.into_iter().take(n_def - 1) {
        // nicht direkt neben den Start, sonst ist sofort alles gesperrt
        if let Some(i) = it
            .by_ref()
            .find(|&i| dist(hack.pos(i), hack.pos(start)) > 1)
        {
            hack.arm(i, k);
        }
    }
    for _ in 0..hack.st.rng.gen_range(2..=3) {
        let u = *UTILITIES.choose(hack.st.rng).unwrap();
        if let Some(i) = it.next() {
            hack.cells[i].kind = u;
        }
    }
    for _ in 0..2 {
        if let Some(i) = it.next() {
            hack.cells[i].kind = "cache";
        }
    }

    // Virus
    hack.coh_max = hack.st.rng.gen_range(70..=90);
    hack.coh = hack.coh_max;
    hack.str_base = hack.st.rng.gen_range(25..=35);
    hack.st.phase("probe", "ds.pt.start", &[]);
    hack.st.log(
        "ds.log.start",
        &[
            ("ship", hack.ship.clone()),
            ("analyzer", hack.analyzer.clone()),
        ],
        "info",
    );
    hack.st.log(
        "ds.log.virus",
        &[
            ("coh", hack.coh.to_string()),
            ("str", hack.str_base.to_string()),
        ],
        "info",
    );
    hack.cells[start].open = true;
    hack.frame(1.3, Some(Sfx::Click), "open");
    hack.st.phase("explore", "ds.pt.explore", &[]);

    // Erkunden, bis der Kern offen ist
    let mut steps = 0;
    let max_steps = (s.duration / 2.0).clamp(8.0, 22.0) as usize;
    while !hack.core_open {
        steps += 1;
        let frontier: Vec<usize> = (0..n)
            .filter(|&i| !hack.cells[i].open)
            .filter(|&i| {
                hack.neighbours(i)
                    .into_iter()
                    .any(|j| hack.cells[j].open && (!hack.is_defense(j) || hack.cells[j].dead))
            })
            .collect();
        let valid: Vec<usize> = frontier
            .iter()
            .copied()
            .filter(|&i| !hack.blocked(i))
            .collect();
        let dcore = |h: &Hack<R>, i: usize| dist(h.pos(i), h.pos(core));
        let pick = if valid.contains(&core) {
            Some(core)
        } else if valid.is_empty() {
            None
        } else {
            let best = valid.iter().map(|&i| dcore(&hack, i)).min().unwrap();
            let greedy = steps > max_steps || hack.st.rng.gen_bool(0.55);
            let cand: Vec<usize> = valid
                .iter()
                .copied()
                .filter(|&i| {
                    if greedy {
                        dcore(&hack, i) == best
                    } else {
                        dcore(&hack, i) <= best + 1
                    }
                })
                .collect();
            cand.choose(hack.st.rng).copied()
        };
        match pick {
            Some(i) => hack.open(i),
            None => {
                // alles gesperrt: die Verteidigung angreifen, die dem Kern am nächsten ist
                let t = (0..n)
                    .filter(|&i| hack.alive_def(i))
                    .min_by_key(|&i| dcore(&hack, i));
                match t {
                    Some(t) => hack.fight(t),
                    None => break,
                }
            }
        }
        if steps > 80 {
            break;
        }
    }
    hack.cursor = core;
    let scene = hack.scene();
    hack.st.finale(s, &scene, false);
    let scene = hack.scene();
    hack.st.finish(s, &scene)
}
