//! Schatzkammer-Rätsel (WoW-Stil): Der Wächter stellt ein Rätsel, dann das
//! Runenschloss, dann die Feuerschalen in der richtigen Reihenfolge – danach
//! öffnet sich die Kammer und alle würfeln um den Loot (1–100).

use super::Stage;
use crate::matrix::{Frame, RunSetup};
use crate::sound::Sfx;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

const SYMBOLS: u8 = 8;

#[derive(Serialize, Clone)]
struct Riddle {
    q: String,
    options: Vec<String>,
    /// bereits falsch geratene Antworten
    wrong: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guess: Option<usize>,
}

#[derive(Serialize, Clone)]
struct Dial {
    cur: u8,
    target: u8,
}

#[derive(Serialize, Clone)]
struct Brazier {
    sym: u8,
    lit: bool,
}

#[derive(Serialize, Clone)]
struct Scene {
    /// riddle | runes | braziers | door | loot
    stage: &'static str,
    hp: i32,
    hp_max: i32,
    seals: u8,
    riddle: Riddle,
    dials: Vec<Dial>,
    dial: usize,
    braziers: Vec<Brazier>,
    /// Reihenfolge als Symbole (wird nach und nach geflüstert)
    order_hint: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trap: Option<String>,
    door_open: bool,
    /// guess | wrong | right | turn | lock | light | fail | open | ""
    fx: &'static str,
    fx_id: u32,
}

pub(super) fn run<R: Rng>(rng: &mut R, s: &RunSetup<'_>) -> Vec<Frame> {
    let mut st = Stage::new(rng, s, "riddle");
    st.v.host = st.pick("dungeons");
    let raw = st.pick("riddles");
    let mut parts: Vec<String> = raw.split('|').map(|p| p.trim().to_string()).collect();
    if parts.len() < 2 {
        parts = vec!["?".into(), "!".into(), "…".into()];
    }
    let q = parts.remove(0);
    let answer = parts[0].clone();
    let mut options: Vec<String> = parts.into_iter().take(4).collect();
    options.shuffle(st.rng);
    let right = options.iter().position(|o| *o == answer).unwrap_or(0);
    let hp_max = 100;
    let n_dials = 5;
    let dials: Vec<Dial> = (0..n_dials)
        .map(|_| {
            let target = st.rng.gen_range(0..SYMBOLS);
            let off = st.rng.gen_range(2..SYMBOLS);
            Dial {
                cur: (target + off) % SYMBOLS,
                target,
            }
        })
        .collect();
    let mut syms: Vec<u8> = (0..SYMBOLS).collect();
    syms.shuffle(st.rng);
    let braziers: Vec<Brazier> = syms[..4]
        .iter()
        .map(|&s| Brazier { sym: s, lit: false })
        .collect();
    let mut order: Vec<usize> = (0..4).collect();
    order.shuffle(st.rng);
    let mut sc = Scene {
        stage: "riddle",
        hp: hp_max,
        hp_max,
        seals: 0,
        riddle: Riddle {
            q: q.clone(),
            options,
            wrong: vec![],
            right: None,
            guess: None,
        },
        dials,
        dial: 0,
        braziers,
        order_hint: vec![],
        trap: None,
        door_open: false,
        fx: "",
        fx_id: 0,
    };
    let push = |st: &mut Stage<R>, sc: &mut Scene, w: f64, sfx: Option<Sfx>, fx: &'static str| {
        sc.fx = fx;
        sc.fx_id += 1;
        st.push(w, sfx, sc);
    };
    let traps = st.lx.list("traps").to_vec();
    let trap = |st: &mut Stage<R>, sc: &mut Scene| {
        let t = traps.choose(st.rng).cloned().unwrap_or_default();
        let (key, text) = t.split_once('|').unwrap_or(("fire", t.as_str()));
        let dmg = st.rng.gen_range(8..=16).min(sc.hp - 10).max(0);
        sc.hp -= dmg;
        sc.trap = Some(key.to_string());
        let line = st.tx(text, &[]);
        st.log(
            "rd.log.trap",
            &[("trap", line), ("n", dmg.to_string())],
            "bad",
        );
        push(st, sc, 1.1, Some(Sfx::Glitch), "fail");
        sc.trap = None;
    };

    st.phase("enter", "rd.pt.enter", &[]);
    st.log("rd.log.enter", &[], "info");
    push(&mut st, &mut sc, 1.3, Some(Sfx::Click), "");

    // 1) Rätsel des Wächters
    st.phase("riddle", "rd.pt.riddle", &[]);
    st.log("rd.log.riddle", &[("q", q)], "warn");
    push(&mut st, &mut sc, 2.2, Some(Sfx::Ice), "");
    let wrong_tries = if s.duration < 20.0 {
        0
    } else {
        st.rng.gen_range(0..=2)
    };
    let mut wrong_opts: Vec<usize> = (0..sc.riddle.options.len())
        .filter(|&i| i != right)
        .collect();
    wrong_opts.shuffle(st.rng);
    for &w in wrong_opts.iter().take(wrong_tries) {
        sc.riddle.guess = Some(w);
        st.log(
            "rd.log.guess",
            &[("a", sc.riddle.options[w].clone())],
            "info",
        );
        push(&mut st, &mut sc, 1.2, Some(Sfx::Click), "guess");
        sc.riddle.wrong.push(w);
        sc.riddle.guess = None;
        st.log("rd.log.wrong", &[], "bad");
        trap(&mut st, &mut sc);
    }
    sc.riddle.guess = Some(right);
    st.log(
        "rd.log.guess",
        &[("a", sc.riddle.options[right].clone())],
        "info",
    );
    push(&mut st, &mut sc, 1.2, Some(Sfx::Click), "guess");
    sc.riddle.right = Some(right);
    sc.seals = 1;
    st.v.marks = 1;
    st.log("rd.log.right", &[], "good");
    push(&mut st, &mut sc, 1.4, Some(Sfx::Win1), "right");

    // 2) Runenschloss
    sc.stage = "runes";
    st.phase("runes", "rd.pt.runes", &[]);
    st.log("rd.log.runes", &[], "info");
    push(&mut st, &mut sc, 1.0, Some(Sfx::Click), "");
    for i in 0..n_dials {
        sc.dial = i;
        // manchmal eins zu weit gedreht
        if st.rng.gen_bool(0.25) && s.duration >= 25.0 {
            sc.dials[i].cur = (sc.dials[i].target + 1) % SYMBOLS;
            push(&mut st, &mut sc, 0.6, Some(Sfx::Click), "turn");
            st.log("rd.log.overturn", &[], "warn");
            sc.dials[i].cur = sc.dials[i].target;
        } else {
            sc.dials[i].cur = sc.dials[i].target;
        }
        st.log("rd.log.rune", &[("n", (i + 1).to_string())], "dim");
        push(&mut st, &mut sc, 0.7, Some(Sfx::Click), "lock");
    }
    sc.seals = 2;
    st.v.marks = 2;
    st.log("rd.log.runes_done", &[], "good");
    push(&mut st, &mut sc, 1.1, Some(Sfx::Win1), "right");

    // 3) Feuerschalen in der richtigen Reihenfolge
    sc.stage = "braziers";
    st.phase("braziers", "rd.pt.braziers", &[]);
    st.log("rd.log.whisper", &[], "edge");
    sc.order_hint = order.iter().map(|&i| sc.braziers[i].sym).collect();
    push(&mut st, &mut sc, 1.4, Some(Sfx::Ice), "");
    if s.duration >= 25.0 && st.rng.gen_bool(0.5) {
        // falsche Schale als zweite
        sc.braziers[order[0]].lit = true;
        push(&mut st, &mut sc, 0.7, Some(Sfx::Click), "light");
        sc.braziers[order[2]].lit = true;
        push(&mut st, &mut sc, 0.7, Some(Sfx::Click), "light");
        for b in sc.braziers.iter_mut() {
            b.lit = false;
        }
        st.log("rd.log.wrong_order", &[], "bad");
        trap(&mut st, &mut sc);
    }
    for &i in &order {
        sc.braziers[i].lit = true;
        push(&mut st, &mut sc, 0.7, Some(Sfx::Click), "light");
    }
    sc.seals = 3;
    st.v.marks = 3;
    st.log("rd.log.braziers_done", &[], "good");

    // Kammer öffnen
    sc.stage = "door";
    sc.door_open = true;
    st.phase("door", "rd.pt.door", &[]);
    st.log("rd.log.door", &[], "good");
    push(&mut st, &mut sc, 1.6, Some(Sfx::Win1), "open");
    sc.stage = "loot";
    st.finale(s, &sc, true);
    st.finish(s, &sc)
}
