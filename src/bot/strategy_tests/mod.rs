use super::*;
use crate::sim::*;
use std::collections::BTreeSet;

const STARTER: &str = include_str!("../../../tests/fixtures/training/starter.luau");
const SENTRY: &str = include_str!("../../../tests/fixtures/training/sentry.luau");

struct Trace {
    cells: Vec<BTreeSet<(i32, i32)>>,
    distance: Vec<f64>,
    shots: Vec<u32>,
    hits: u32,
}

/// 固定观察边界，实际调用参赛脚本与陪练样本。考察经过哪些格、是否交火，而非仅检测返回值合法。
fn duel(map: &str, scripts: &[&str], seconds: u32) -> (World, Trace) {
    let map: Arc<ArenaMap> = Arc::new(map.parse().unwrap());
    let seats: Vec<_> = (0..scripts.len())
        .map(|i| Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId((i % 2) as u32),
        })
        .collect();
    let clock = ClockRules::unlimited();
    let mut w = World::new(map, seats, clock).unwrap();
    let mut bots = scripts
        .iter()
        .zip(w.tanks())
        .map(|(src, &t)| {
            let script = Script::from_source("test", src).unwrap();
            LuauBot::for_world(&script, &w, t.seat.slot).unwrap()
        })
        .collect::<Vec<_>>();
    let mut trace = Trace {
        cells: vec![BTreeSet::new(); scripts.len()],
        distance: vec![0.; scripts.len()],
        shots: vec![0; scripts.len()],
        hits: 0,
    };

    // 教学机器人的每次直射都应有无遮挡的敌方目标。
    let direct_only: Vec<_> = scripts
        .iter()
        .map(|&s| s == STARTER || s == SENTRY)
        .collect();
    for _ in 0..HZ * seconds {
        let before = w.tanks().to_vec();
        let mut controls = Vec::new();
        for ((bot, t), &direct) in bots.iter_mut().zip(&before).zip(&direct_only) {
            if t.alive {
                let action = bot.decide(&w).unwrap();
                if direct && action.fire.unwrap_or(false) {
                    assert!(
                        w.sight(t.seat.slot).unwrap().enemy_ahead,
                        "fire must target an enemy before any wall or teammate"
                    );
                }
                controls.push((t.seat.slot, action));
            }
        }

        w.step(&controls, &[]);
        for (i, (old, new)) in before.iter().zip(w.tanks()).enumerate() {
            trace.cells[i].insert((new.center.x / CELL, new.center.y / CELL));
            trace.distance[i] += f64::from(new.center.x - old.center.x)
                .hypot(f64::from(new.center.y - old.center.y))
                / f64::from(CELL);
        }

        for e in w.events() {
            match e {
                Event::Fired { owner, .. } => trace.shots[(owner.0 - 1) as usize] += 1,
                Event::Hit {
                    kind: HitKind::Enemy,
                    ..
                } => trace.hits += 1,
                _ => (),
            }
        }

        if w.outcome().is_some() {
            break;
        }
    }

    (w, trace)
}

#[test]
fn historical_training_pair_leaves_spawn_and_fights() {
    let (w, t) = duel(SWITCHYARD, &[SENTRY, STARTER], 30);
    eprintln!(
        "{} ticks {:?}, travel {:?}, cells {:?}, shots {:?}, hits {}",
        w.tick(),
        w.outcome(),
        t.distance,
        t.cells.iter().map(BTreeSet::len).collect::<Vec<_>>(),
        t.shots,
        t.hits
    );
    for i in 0..2 {
        assert!(
            t.distance[i] > 2.0 && t.cells[i].len() >= 3,
            "slot {i} failed to leave the spawn area"
        );
    }

    assert!(t.shots.iter().all(|&n| n > 0));
    assert!(w.outcome().is_some() && t.hits > 0);
}

#[test]
fn each_strategy_can_navigate_to_and_hit_an_initially_hidden_opponent() {
    const IDLE: &str = "return function(_) return nil end";

    for src in [SENTRY, STARTER] {
        let (w, t) = duel(SWITCHYARD, &[src, IDLE], 30);
        assert!(t.cells[0].len() >= 3);
        assert!(
            t.shots[0] > 0 && t.hits > 0,
            "travel {:?} shots {:?} hits {}",
            t.distance,
            t.shots,
            t.hits
        );
        assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(0))));
    }
}

#[test]
fn full_roster_navigates_and_confirms_line_of_fire() {
    let (_, t) = duel(
        SWITCHYARD,
        &[
            STARTER, SENTRY, STARTER, SENTRY, SENTRY, STARTER, SENTRY, STARTER,
        ],
        15,
    );
    assert!(t.cells.iter().filter(|c| c.len() >= 2).count() >= 4);
    assert!(t.hits > 0);
}
