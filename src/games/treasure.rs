//! Schatzsuche im Dorf (Palia-Stil): Hinweise einsammeln, das Suchgebiet
//! eingrenzen, graben (heiß/kalt), Schatzkiste heben, Lose ziehen.

use super::Stage;
use crate::matrix::{Frame, RunSetup};
use crate::sound::Sfx;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

const W: i32 = 14;
const H: i32 = 8;

#[derive(Serialize, Clone)]
struct Deco {
    x: i32,
    y: i32,
    /// tree | flower | rock | house | pond | bush | mushroom | lamp
    k: &'static str,
}

#[derive(Serialize, Clone)]
struct Hole {
    x: i32,
    y: i32,
    item: String,
}

#[derive(Serialize, Clone)]
struct Clue {
    x: i32,
    y: i32,
    taken: bool,
}

#[derive(Serialize, Clone)]
struct Zone {
    x: f32,
    y: f32,
    r: f32,
}

#[derive(Serialize, Clone)]
struct Scene {
    w: i32,
    h: i32,
    decor: Vec<Deco>,
    px: i32,
    py: i32,
    holes: Vec<Hole>,
    clues: Vec<Clue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone: Option<Zone>,
    /// 0 = eiskalt … 1 = gefunden
    heat: f32,
    heat_text: String,
    /// hidden | found | open
    chest: &'static str,
    cx: i32,
    cy: i32,
    /// walk | dig | clue | event | found | open | ""
    fx: &'static str,
    fx_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    stamina: u8,
    stamina_max: u8,
    tool: String,
    /// zuletzt gefundener Gegenstand
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<String>,
}

fn d(a: (i32, i32), b: (i32, i32)) -> f32 {
    (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f32).sqrt()
}

pub(super) fn run<R: Rng>(rng: &mut R, s: &RunSetup<'_>) -> Vec<Frame> {
    let mut st = Stage::new(rng, s, "treasure");
    st.v.host = st.pick("places");
    let tool = st.pick("tools");

    // Karte
    let mut used: Vec<(i32, i32)> = vec![];
    let mut decor = vec![];
    let home = (1, H / 2);
    used.push(home);
    decor.push(Deco {
        x: 0,
        y: H / 2 - 1,
        k: "house",
    });
    used.push((0, H / 2 - 1));
    // Teich
    let (pxp, pyp) = (st.rng.gen_range(5..W - 4), st.rng.gen_range(0..2) * (H - 2));
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        decor.push(Deco {
            x: pxp + dx,
            y: pyp + dy,
            k: "pond",
        });
        used.push((pxp + dx, pyp + dy));
    }
    let kinds = [
        "tree", "tree", "tree", "flower", "flower", "rock", "bush", "mushroom", "lamp",
    ];
    for _ in 0..22 {
        let p = (st.rng.gen_range(0..W), st.rng.gen_range(0..H));
        if used.iter().any(|u| d(*u, p) < 1.0) || d(p, home) < 2.0 {
            continue;
        }
        used.push(p);
        decor.push(Deco {
            x: p.0,
            y: p.1,
            k: kinds.choose(st.rng).unwrap(),
        });
    }
    let free = |used: &Vec<(i32, i32)>, p: (i32, i32)| !used.contains(&p);
    // Schatz rechts auf der Karte
    let treasure = loop {
        let p = (st.rng.gen_range(W / 2..W - 1), st.rng.gen_range(1..H - 1));
        if free(&used, p) {
            break p;
        }
    };
    used.push(treasure);
    // drei Hinweise verteilt
    let mut clues = vec![];
    for band in [(2, W / 3), (W / 3, 2 * W / 3), (W / 2, W - 1)] {
        let p = loop {
            let p = (st.rng.gen_range(band.0..band.1), st.rng.gen_range(0..H));
            if free(&used, p) && d(p, treasure) > 2.5 {
                break p;
            }
        };
        used.push(p);
        clues.push(Clue {
            x: p.0,
            y: p.1,
            taken: false,
        });
    }

    let mut sc = Scene {
        w: W,
        h: H,
        decor,
        px: home.0,
        py: home.1,
        holes: vec![],
        clues,
        zone: None,
        heat: 0.0,
        heat_text: String::new(),
        chest: "hidden",
        cx: treasure.0,
        cy: treasure.1,
        fx: "",
        fx_id: 0,
        event: None,
        stamina: 10,
        stamina_max: 10,
        tool: tool.clone(),
        item: None,
    };
    let push = |st: &mut Stage<R>, sc: &mut Scene, w: f64, sfx: Option<Sfx>, fx: &'static str| {
        sc.fx = fx;
        sc.fx_id += 1;
        st.push(w, sfx, sc);
    };

    st.phase("start", "tr.pt.start", &[]);
    st.log("tr.log.start", &[("tool", tool)], "info");
    push(&mut st, &mut sc, 1.3, Some(Sfx::Click), "");

    // Hinweise sammeln
    st.phase("clues", "tr.pt.clues", &[]);
    let radii = [4.0f32, 2.6, 1.6];
    let events = st.lx.list("events").to_vec();
    let mut clue_lines = st.lx.list("clues").to_vec();
    clue_lines.shuffle(st.rng);
    let event_at = if s.duration >= 20.0 && !events.is_empty() {
        st.rng.gen_range(0..3)
    } else {
        9
    };
    for k in 0..3 {
        let (cx, cy) = (sc.clues[k].x, sc.clues[k].y);
        sc.px = cx;
        sc.py = cy;
        push(&mut st, &mut sc, 0.8, None, "walk");
        sc.clues[k].taken = true;
        st.v.marks = k as u8 + 1;
        // Zone um den Schatz, leicht versetzt
        let r = radii[k];
        let off = r * 0.45;
        let ang: f32 = st.rng.gen_range(0.0..std::f32::consts::TAU);
        sc.zone = Some(Zone {
            x: treasure.0 as f32 + ang.cos() * off,
            y: treasure.1 as f32 + ang.sin() * off,
            r,
        });
        let dir = {
            let (dx, dy) = ((treasure.0 - cx) as f32, (treasure.1 - cy) as f32);
            let a = dy.atan2(dx).to_degrees();
            let i = (((a + 360.0 + 22.5) % 360.0) / 45.0) as usize % 8;
            st.tx(
                &format!("tr.dir.{}", ["e", "se", "s", "sw", "w", "nw", "n", "ne"][i]),
                &[],
            )
        };
        let line = clue_lines.get(k).cloned().unwrap_or_default();
        st.log(
            "tr.log.clue",
            &[("n", (k + 1).to_string()), ("clue", line), ("dir", dir)],
            "good",
        );
        push(&mut st, &mut sc, 1.1, Some(Sfx::Click), "clue");
        if k == event_at {
            let ev = events.choose(st.rng).cloned().unwrap_or_default();
            let (key, text) = ev.split_once('|').unwrap_or(("crow", ev.as_str()));
            sc.event = Some(key.to_string());
            sc.stamina = sc.stamina.saturating_sub(1);
            st.phase("event", "tr.pt.event", &[]);
            let line = st.tx(text, &[]);
            st.v.log.push(crate::matrix::LogLine {
                text: line,
                tone: "warn".into(),
            });
            push(&mut st, &mut sc, 1.2, Some(Sfx::Ice), "event");
            sc.event = None;
            st.phase("clues", "tr.pt.clues", &[]);
        }
    }
    st.log("tr.log.clues_done", &[], "good");

    // Graben: erst daneben, dann der Schatz
    st.phase("dig", "tr.pt.dig", &[]);
    let misses = if s.duration < 20.0 {
        1
    } else if s.duration < 45.0 {
        2
    } else {
        3
    };
    let mut spots: Vec<(i32, i32)> = vec![];
    let mut tries = 0;
    while spots.len() < misses && tries < 400 {
        tries += 1;
        let p = (
            treasure.0 + st.rng.gen_range(-3..=3),
            treasure.1 + st.rng.gen_range(-2..=2),
        );
        if p.0 < 0
            || p.1 < 0
            || p.0 >= W
            || p.1 >= H
            || p == treasure
            || !free(&used, p)
            || spots.contains(&p)
        {
            continue;
        }
        spots.push(p);
    }
    // von weit nach nah
    spots.sort_by(|a, b| d(*b, treasure).partial_cmp(&d(*a, treasure)).unwrap());
    spots.push(treasure);
    let heat_keys = [
        "tr.heat.0",
        "tr.heat.1",
        "tr.heat.2",
        "tr.heat.3",
        "tr.heat.4",
    ];
    for p in spots {
        sc.px = p.0;
        sc.py = p.1;
        sc.item = None;
        push(&mut st, &mut sc, 0.7, None, "walk");
        sc.stamina = sc.stamina.saturating_sub(1).max(1);
        push(&mut st, &mut sc, 0.9, Some(Sfx::Dice), "dig");
        if p == treasure {
            sc.heat = 1.0;
            sc.heat_text = st.tx("tr.heat.5", &[]);
            sc.chest = "found";
            st.v.marks = 3;
            st.phase("found", "tr.pt.found", &[]);
            st.log("tr.log.found", &[], "good");
            push(&mut st, &mut sc, 1.4, Some(Sfx::Win1), "found");
        } else {
            let dist = d(p, treasure);
            let lvl = ((4.0 - dist).clamp(0.0, 4.0)) as usize;
            sc.heat = (lvl as f32 + 0.5) / 5.5;
            sc.heat_text = st.tx(heat_keys[lvl], &[]);
            let item = st.pick("finds");
            sc.holes.push(Hole {
                x: p.0,
                y: p.1,
                item: item.clone(),
            });
            sc.item = Some(item.clone());
            st.log(
                "tr.log.dig",
                &[("item", item), ("heat", sc.heat_text.clone())],
                "dim",
            );
            push(&mut st, &mut sc, 1.0, Some(Sfx::Click), "");
        }
    }
    sc.item = None;
    sc.chest = "open";
    st.log("tr.log.open", &[], "edge");
    push(&mut st, &mut sc, 1.0, Some(Sfx::Click), "open");
    st.finale(s, &sc, false);
    st.finish(s, &sc)
}
