//! Bosskampf im Anime-Stil: Verwandlung, Spezialattacken mit Cut-ins,
//! drei Lebensbalken (Formen) des Bosses, Kraft der Freundschaft aus dem
//! Chat (die Namen der Teilnehmer fliegen in die Leiste), ultimative Technik.

use super::Stage;
use crate::matrix::{Frame, RunSetup};
use crate::sound::Sfx;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::Serialize;

#[derive(Serialize, Clone)]
struct Cutin {
    /// hero | boss | ult
    who: &'static str,
    text: String,
}

#[derive(Serialize, Clone)]
struct Dmg {
    /// hero | boss
    who: &'static str,
    n: i32,
    crit: bool,
}

#[derive(Serialize, Clone)]
struct Scene {
    hero: String,
    hero_hp: i32,
    hero_max: i32,
    boss: String,
    boss_hp: i32,
    boss_max: i32,
    /// gebrochene Balken (0–3)
    bars_broken: u8,
    bars: u8,
    combo: u32,
    /// Freundschaftskraft 0–1
    power: f32,
    /// Namen, die gerade in die Leiste fliegen
    sparks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cutin: Option<Cutin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dmg: Option<Dmg>,
    ko: bool,
    /// transform | appear | attack | hit | dodge | form | power | ult | ko | ""
    fx: &'static str,
    fx_id: u32,
}

pub(super) fn run<R: Rng>(rng: &mut R, s: &RunSetup<'_>) -> Vec<Frame> {
    let mut st = Stage::new(rng, s, "battle");
    st.v.host = st.pick("arenas");
    let boss = st.pick("bosses");
    let mut sc = Scene {
        hero: st.v.decker.clone(),
        hero_hp: 100,
        hero_max: 100,
        boss: boss.clone(),
        boss_hp: 100,
        boss_max: 100,
        bars_broken: 0,
        bars: 3,
        combo: 0,
        power: 0.0,
        sparks: vec![],
        cutin: None,
        dmg: None,
        ko: false,
        fx: "",
        fx_id: 0,
    };
    let push = |st: &mut Stage<R>, sc: &mut Scene, w: f64, sfx: Option<Sfx>, fx: &'static str| {
        sc.fx = fx;
        sc.fx_id += 1;
        st.push(w, sfx, sc);
        sc.cutin = None;
        sc.dmg = None;
        sc.sparks.clear();
    };

    st.phase("transform", "bt.pt.transform", &[]);
    sc.cutin = Some(Cutin {
        who: "hero",
        text: st.tx("bt.cut.transform", &[]),
    });
    st.log("bt.log.transform", &[], "edge");
    push(&mut st, &mut sc, 1.6, Some(Sfx::Win1), "transform");

    st.phase("appear", "bt.pt.appear", &[("boss", boss.clone())]);
    let line = st.pick("boss_lines");
    sc.cutin = Some(Cutin {
        who: "boss",
        text: line.clone(),
    });
    st.log(
        "bt.log.appear",
        &[("boss", boss.clone()), ("line", line)],
        "bad",
    );
    push(&mut st, &mut sc, 1.6, Some(Sfx::Ice), "appear");

    let per_bar = if s.duration < 20.0 {
        1
    } else if s.duration < 45.0 {
        2
    } else {
        3
    };
    for bar in 0..3u8 {
        st.phase("fight", "bt.pt.fight", &[("n", (bar + 1).to_string())]);
        let last_bar = bar == 2;
        let exchanges = if last_bar {
            per_bar.max(1)
        } else {
            per_bar + 1
        };
        for ex in 0..exchanges {
            // Held greift an
            let mv = st.pick("moves");
            let crit = st.rng.gen_bool(0.25);
            let mut n = st.rng.gen_range(18..=32) * if crit { 2 } else { 1 };
            let final_hit = !last_bar && ex == exchanges - 1;
            if final_hit {
                n = sc.boss_hp; // Balken bricht
            } else {
                n = n.min(sc.boss_hp - 8).max(4);
            }
            sc.boss_hp -= n;
            sc.combo += 1;
            sc.cutin = Some(Cutin {
                who: "hero",
                text: format!("{mv}!"),
            });
            sc.dmg = Some(Dmg {
                who: "boss",
                n,
                crit,
            });
            st.log(
                if crit { "bt.log.crit" } else { "bt.log.attack" },
                &[("move", mv), ("n", n.to_string()), ("boss", boss.clone())],
                "good",
            );
            push(&mut st, &mut sc, 1.1, Some(Sfx::Dice), "attack");
            if final_hit {
                sc.bars_broken += 1;
                st.v.marks = sc.bars_broken;
                sc.boss_hp = sc.boss_max;
                let key = format!("bt.cut.form{}", bar + 2);
                sc.cutin = Some(Cutin {
                    who: "boss",
                    text: st.tx(&key, &[]),
                });
                st.log(
                    "bt.log.form",
                    &[("boss", boss.clone()), ("n", (bar + 2).to_string())],
                    "warn",
                );
                push(&mut st, &mut sc, 1.4, Some(Sfx::Glitch), "form");
                continue;
            }
            // Boss schlägt zurück
            let bm = st.pick("boss_moves");
            if st.rng.gen_bool(0.3) {
                st.log("bt.log.dodge", &[("move", bm)], "edge");
                push(&mut st, &mut sc, 0.8, Some(Sfx::Click), "dodge");
            } else {
                let d = st.rng.gen_range(10..=22).min(sc.hero_hp - 12).max(0);
                sc.hero_hp -= d;
                sc.combo = 0;
                sc.cutin = Some(Cutin {
                    who: "boss",
                    text: format!("{bm}!"),
                });
                sc.dmg = Some(Dmg {
                    who: "hero",
                    n: d,
                    crit: false,
                });
                st.log(
                    "bt.log.hit",
                    &[("move", bm), ("n", d.to_string()), ("boss", boss.clone())],
                    "bad",
                );
                push(&mut st, &mut sc, 1.0, Some(Sfx::Glitch), "hit");
            }
        }
    }

    // Kraft der Freundschaft: Namen aus dem Chat laden die Leiste
    st.phase("power", "bt.pt.power", &[]);
    st.log(
        "bt.log.power",
        &[("n", s.participants.len().to_string())],
        "edge",
    );
    let mut names: Vec<String> = s.participants.to_vec();
    names.shuffle(st.rng);
    names.truncate(18);
    let chunks: Vec<Vec<String>> = names
        .chunks(names.len().div_ceil(3).max(1))
        .map(|c| c.to_vec())
        .collect();
    let k = chunks.len().max(1) as f32;
    for (i, c) in chunks.into_iter().enumerate() {
        sc.sparks = c;
        sc.power = (i as f32 + 1.0) / k;
        let fx = "power";
        sc.fx = fx;
        sc.fx_id += 1;
        st.push(0.9, Some(Sfx::Click), &sc);
        sc.sparks.clear();
    }
    sc.power = 1.0;

    // Ultimative Technik
    st.phase("ult", "bt.pt.ult", &[]);
    let ult = st.pick("ultimates");
    sc.cutin = Some(Cutin {
        who: "ult",
        text: ult.clone(),
    });
    st.log("bt.log.ult", &[("move", ult)], "edge");
    push(&mut st, &mut sc, 1.8, Some(Sfx::Win1), "ult");
    let n = sc.boss_hp;
    sc.boss_hp = 0;
    sc.bars_broken = 3;
    sc.ko = true;
    sc.dmg = Some(Dmg {
        who: "boss",
        n: n * 9 + 999,
        crit: true,
    });
    st.log("bt.log.ko", &[("boss", boss.clone())], "good");
    push(&mut st, &mut sc, 1.5, Some(Sfx::Glitch), "ko");
    sc.power = 0.0;
    st.finale(s, &sc, false);
    st.finish(s, &sc)
}
