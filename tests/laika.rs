//! 验证可观察行为，不把脚本的内部目标名或某个固定回放哈希当成测试。
use std::{collections::BTreeSet, sync::Arc};
use tank_arena::{
    bot::{LuauBot, Script},
    sim::*,
};

const LAIKA: &str = include_str!("../assets/strategies/laika.luau");
const BANK: &str = "\
+-+-+-+-+-+
|. . . . .|
+ + + + + +
|. >|< . .|
+ + + + + +
|. . . . .|
+-+-+-+-+-+";

fn arena(map: &str, teams: &[u32]) -> World {
    let seats = teams
        .iter()
        .enumerate()
        .map(|(i, &team)| Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId(team),
        })
        .collect();
    World::new(Arc::new(map.parse().unwrap()), seats, ClockRules::default()).unwrap()
}
fn pilot(w: &World, slot: u32) -> LuauBot {
    LuauBot::for_world(
        &Script::from_source("test", LAIKA).unwrap(),
        w,
        SlotId(slot),
    )
    .unwrap()
}

#[test]
fn bank_shot_reaches_a_hidden_enemy_before_hunting_around_the_wall() {
    let mut w = arena(BANK, &[0, 1]).with_behavior_seed(Some(0));
    let mut bot = pilot(&w, 1);
    let mut indirect = BTreeSet::new();
    let mut bounced = BTreeSet::new();
    for _ in 0..300 {
        let direct = w.sight(SlotId(1)).unwrap().enemy_ahead;
        let act = bot.decide(&w).unwrap();
        w.step(&[(SlotId(1), act)], &[]);
        for e in w.events() {
            match *e {
                Event::Fired { shell, .. } if !direct => {
                    indirect.insert(shell);
                }
                Event::Bounce { shell, .. } => {
                    bounced.insert(shell);
                }
                Event::Hit {
                    shell,
                    kind: HitKind::Enemy,
                    ..
                } => {
                    assert!(indirect.contains(&shell) && bounced.contains(&shell));
                    return;
                }
                _ => (),
            }
        }
        if w.outcome().is_some() {
            break;
        }
    }
    panic!("no confirmed bank shot: {:?}", w.outcome());
}

#[test]
fn dodges_a_live_shell_that_kills_an_idle_tank() {
    let map = "\
+-+-+-+-+-+
|. . . . .|
+ + + + + +
|> . v . .|
+ + + + + +
|. . . . .|
+-+-+-+-+-+";
    let mut w = arena(map, &[0, 1]);
    // 同一个已发射的炮弹；此后双方均不得再开火。
    w.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(true),
                ..Default::default()
            },
        )],
        &[],
    );
    let mut idle = w.clone();
    let mut bot = pilot(&w, 2);
    for _ in 0..100 {
        idle.step(&[], &[]);
        if w.outcome().is_none() {
            let mut action = bot.decide(&w).unwrap();
            action.fire = Some(false);
            w.step(&[(SlotId(2), action)], &[]);
        }
    }
    assert!(!idle.tank(SlotId(2)).unwrap().alive);
    assert!(w.tank(SlotId(2)).unwrap().alive);
}

#[test]
fn navigates_to_an_opponent_outside_firing_range() {
    let mut w = arena(SWITCHYARD, &[0, 1]);
    let mut bot = pilot(&w, 1);
    let mut cells = BTreeSet::new();
    for _ in 0..HZ * 30 {
        let c = w.tanks()[0].center;
        cells.insert((c.x / CELL, c.y / CELL));
        let act = bot.decide(&w).unwrap();
        w.step(&[(SlotId(1), act)], &[]);
        if w.outcome().is_some() {
            break;
        }
    }
    assert!(cells.len() >= 3);
    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(0))));
}

fn corridor(distance: usize, ally: bool) -> (World, SlotId) {
    let n = distance + 2;
    let edge = format!("{}+", "+-".repeat(n));
    let opening = format!("{}+", "+ ".repeat(n));
    let row = |xs: Vec<&str>| format!("|{}|", xs.join(" "));
    let floor = row(vec!["."; n]);
    let mut line = vec!["."; n];
    line[0] = ">";
    line[distance] = "<";
    if ally {
        line[2] = ">";
    }
    let source = [
        edge.clone(),
        floor.clone(),
        opening.clone(),
        row(line),
        opening,
        floor,
        edge,
    ]
    .join("\n");
    (
        arena(&source, if ally { &[0, 0, 1] } else { &[0, 1] }),
        SlotId(1),
    )
}

#[test]
fn reference_corridor_matches_goal_boundary_and_fire_edges() {
    // 网页版决策器的受控直廊：相同位置、满弹、无来弹；时钟 60 Hz。只推进时钟，不反馈按键。
    for d in [4, 5, 7] {
        let (mut w, slot) = corridor(d, false);
        let mut bot = pilot(&w, slot.0);
        let mut shots = vec![];
        let mut held = vec![];
        let mut moved = false;
        let mut down = false;
        for tick in 0..65 {
            assert!(w.sight(slot).unwrap().enemy_ahead);
            let a = bot.decide(&w).unwrap();
            let pressed = a.fire == Some(true);
            if pressed {
                held.push(tick);
            }
            if pressed && !down {
                shots.push(tick);
            }
            down = pressed;
            moved |= a.drive == Some(Drive::Forward);
            w.step(&[], &[]);
        }
        if d == 4 {
            assert_eq!(shots, [9, 42]);
            assert_eq!(held, [9, 10, 42, 43]);
            assert!(!moved);
        } else {
            assert!(shots.is_empty());
            assert!(moved);
        }
    }
}

#[test]
fn does_not_fire_through_a_teammate_at_normal_engagement_distance() {
    let (mut w, slot) = corridor(4, true);
    let mut bot = pilot(&w, slot.0);
    for _ in 0..65 {
        assert!(!w.sight(slot).unwrap().enemy_ahead);
        assert_ne!(bot.decide(&w).unwrap().fire, Some(true));
        w.step(&[], &[]);
    }
}

#[test]
fn identical_seeds_repeat_the_same_fan_but_other_seeds_can_choose_different_inputs() {
    fn opening(seed: u64) -> Vec<ControlUpdate> {
        let mut w = arena(BANK, &[0, 1]).with_behavior_seed(Some(seed));
        let mut bot = pilot(&w, 1);
        (0..60)
            .map(|_| {
                let a = bot.decide(&w).unwrap();
                w.step(&[(SlotId(1), a)], &[]);
                a
            })
            .collect()
    }
    let a = opening(0);
    assert_eq!(a, opening(0));
    assert_ne!(a, opening(11));
}

#[test]
fn finishes_planning_from_a_cold_start_with_eight_tanks_and_forty_shells() {
    let n = 31;
    let mut rows = Vec::new();
    let edge = format!("{}+", "+-".repeat(n));
    let open = format!("{}+", "+ ".repeat(n));
    for y in 0..n {
        rows.push(if y == 0 { edge.clone() } else { open.clone() });
        let row: Vec<_> = (0..n)
            .map(|x| {
                if (14..17).contains(&x) && (14..17).contains(&y) && (x, y) != (16, 16) {
                    ">"
                } else {
                    "."
                }
            })
            .collect();
        rows.push(format!("|{}|", row.join(" ")));
    }
    rows.push(edge);
    let mut w = arena(&rows.join("\n"), &[0, 1, 0, 1, 0, 1, 0, 1]);
    for tick in 0..(2 * AMMO - 1) {
        let controls: Vec<_> = w
            .tanks()
            .iter()
            .map(|t| {
                (
                    t.seat.slot,
                    ControlUpdate {
                        fire: Some(tick % 2 == 0),
                        ..Default::default()
                    },
                )
            })
            .collect();
        w.step(&controls, &[]);
    }
    assert_eq!(w.shells().len(), MAX_SLOTS * usize::from(AMMO));
    let mut bots: Vec<_> = w.tanks().iter().map(|t| pilot(&w, t.seat.slot.0)).collect();
    let mut responded = [false; MAX_SLOTS];
    for _ in 0..24 {
        for (i, bot) in bots.iter_mut().enumerate() {
            let a = bot.decide(&w).unwrap();
            responded[i] |=
                a.drive != Some(Drive::Stop) || a.turn != Some(Turn::None) || a.fire == Some(true);
        }
        // 压测初次完整分片规划，不以“第一次返回空操作”充当通过。
        w.step(&[], &[]);
    }
    assert!(responded.iter().any(|&v| v));
}

#[test]
fn treats_its_own_returning_shell_as_dangerous() {
    let mut w = arena(
        "\
+-+-+-+-+-+-+-+
|. . . . . . .|
+ + + + + + + +
|. . > . . . .|
+ + + + + + + +
|. . . . . . <|
+-+-+-+-+-+-+-+",
        &[0, 1],
    );
    let slot = SlotId(1);
    // 朝远墙发射，原地转至侧向，让回弹横穿车身；不允许再次开火。
    w.step(
        &[(
            slot,
            ControlUpdate {
                fire: Some(true),
                turn: Some(Turn::Right),
                ..Default::default()
            },
        )],
        &[],
    );
    for _ in 1..19 {
        w.step(&[], &[]);
    }
    w.step(
        &[(
            slot,
            ControlUpdate {
                fire: Some(false),
                turn: Some(Turn::None),
                ..Default::default()
            },
        )],
        &[],
    );
    for _ in 0..400 {
        let shell = &w.shells()[0];
        if shell.deadly_to_owner() && shell.center.x - w.tanks()[0].center.x < CELL {
            break;
        }
        w.step(&[], &[]);
    }
    assert!(w.shells()[0].deadly_to_owner());
    let mut idle = w.clone();
    let mut bot = pilot(&w, 1);
    for _ in 0..90 {
        idle.step(&[], &[]);
        if w.outcome().is_none() {
            let mut act = bot.decide(&w).unwrap();
            act.fire = Some(false);
            w.step(&[(slot, act)], &[]);
        }
    }
    assert!(!idle.tank(slot).unwrap().alive);
    assert!(w.tank(slot).unwrap().alive);
}

#[test]
fn enemy_barrel_changes_which_side_of_a_symmetric_wall_is_taken() {
    // 路长、敌车位置、我方姿态不变；另一边没有更近的目标，也没有来弹触发紧急闪避。
    for (enemy, expected) in [('9', Turn::Right), ('3', Turn::Left)] {
        let floor = "|. . . . . . .|";
        let middle = format!("|>|. . . . {enemy} .|");
        let map = format!(
            "+-+-+-+-+-+-+-+\n{}\n+-+-+-+-+-+-+-+",
            [floor, floor, &middle, floor, floor].join("\n+ + + + + + + +\n")
        );
        let w = arena(&map, &[0, 1]);
        assert!(w.shells().is_empty());
        let mut b = pilot(&w, 1);
        assert_eq!(b.decide(&w).unwrap().turn, Some(expected));
    }
}

#[test]
fn empty_default_weapon_increases_separation_instead_of_hunting() {
    let edge = format!("{}+", "+-".repeat(14));
    let gap = format!("{}+", "+ ".repeat(14));
    let floor = format!("|{}|", vec!["."; 14].join(" "));
    let mut spawns = ["."; 14];
    spawns[4] = "<";
    spawns[8] = ">";
    let mid = format!("|{}|", spawns.join(" "));
    let map = [
        &edge, &floor, &gap, &floor, &gap, &mid, &gap, &floor, &gap, &floor, &edge,
    ]
    .map(String::as_str)
    .join("\n");
    let mut w = arena(&map, &[0, 1]);
    let slot = SlotId(1);
    // 五颗自己的弹均向远墙飞离，短时间内不会弹回近身。
    for t in 0..2 * AMMO - 1 {
        w.step(
            &[(
                slot,
                ControlUpdate {
                    fire: Some(t % 2 == 0),
                    ..Default::default()
                },
            )],
            &[],
        );
    }
    assert_eq!(w.sight(slot).unwrap().ammo, 0);
    let gap = |w: &World| {
        let a = w.tanks()[0].center;
        let b = w.tanks()[1].center;
        (a.x - b.x).abs() + (a.y - b.y).abs()
    };
    let initial = gap(&w);
    let mut b = pilot(&w, 1);
    for _ in 0..32 {
        let a = b.decide(&w).unwrap();
        assert_ne!(a.fire, Some(true));
        w.step(&[(slot, a)], &[]);
    }
    assert!(gap(&w) > initial + CELL / 4);
}
