use std::sync::Arc;
use tank_arena::sim::*;

const DUEL: &str = "+-+-+-+-+-+-+-+
|> . . . . . <|
+ + + + + + + +
|. . . . . . .|
+ + + + + + + +
|. . . . . . .|
+-+-+-+-+-+-+-+";
const BANK: &str = include_str!("fixtures/diagonal.map");

fn seats(teams: &[u32]) -> Vec<Seat> {
    teams
        .iter()
        .enumerate()
        .map(|(i, &t)| Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId(t),
        })
        .collect()
}

fn arena(map: &str, teams: &[u32], secs: Option<u64>) -> World {
    World::new(
        Arc::new(map.parse().unwrap()),
        seats(teams),
        secs.map_or(Ok(ClockRules::unlimited()), ClockRules::with_time_limit)
            .unwrap(),
    )
    .unwrap()
}

fn fire(id: u32) -> (SlotId, ControlUpdate) {
    (
        SlotId(id),
        ControlUpdate {
            fire: Some(true),
            ..ControlUpdate::default()
        },
    )
}

fn hits(w: &mut World, first: &[(SlotId, ControlUpdate)]) -> Vec<Event> {
    w.step(first, &[]);
    let mut all = w.events().to_vec();
    for _ in 0..1200 {
        if w.outcome().is_some() {
            break;
        }
        w.step(&[], &[]);
        all.extend_from_slice(w.events());
    }

    all
}

#[test]
fn rebound_around_corner_and_self_hit() {
    let mut w = arena(BANK, &[0, 1], None);
    assert!(!w.sight(SlotId(1)).unwrap().enemy_ahead);
    let e = hits(&mut w, &[fire(1)]);
    assert!(e.iter().any(|e| matches!(e, Event::Bounce { .. })));
    assert!(e.iter().any(|e| matches!(
        e,
        Event::Hit {
            kind: HitKind::Enemy,
            ..
        }
    )));
    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(0))));
    let mut w = arena(
        "+-+-+-+-+-+-+-+
|< . . . . . ^|
+ + + + + + + +
|. . . . . . .|
+ + + + + + + +
|. . . . . . .|
+-+-+-+-+-+-+-+",
        &[0, 1],
        None,
    );
    let e = hits(&mut w, &[fire(1)]);
    assert!(e.iter().any(|e| matches!(
        e,
        Event::Hit {
            kind: HitKind::SelfHit,
            ..
        }
    )));
    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(1))));
}

#[test]
fn simultaneous_hits_and_friendly_fire() {
    let mut w = arena(DUEL, &[0, 1], None);
    assert!(w.sight(SlotId(1)).unwrap().enemy_ahead);
    hits(&mut w, &[fire(1), fire(2)]);
    assert_eq!(w.outcome(), Some(Outcome::MutualElimination));
    let mut w = arena(
        "+-+-+-+-+-+-+-+-+-+
|> . . > . . . . <|
+ + + + + + + + + +
|. . . . . . . . .|
+ + + + + + + + + +
|. . . . . . . . .|
+-+-+-+-+-+-+-+-+-+",
        &[0, 0, 1],
        None,
    );
    assert!(!w.sight(SlotId(1)).unwrap().enemy_ahead);
    let e = hits(&mut w, &[fire(1)]);
    assert!(e.iter().any(|e| matches!(
        e,
        Event::Hit {
            kind: HitKind::Friendly,
            ..
        }
    )));
    assert_eq!(w.outcome(), None);
    assert!(!w.tanks()[1].alive);
}

#[test]
fn exact_timeout_and_unlimited() {
    for sec in [10, 60] {
        let mut w = arena(DUEL, &[0, 1], Some(sec));
        let n = u64::from(HZ) * sec;
        for _ in 1..n {
            w.step(&[], &[]);
        }

        assert_eq!(w.outcome(), None);
        w.step(&[], &[]);
        assert_eq!((w.tick(), w.outcome()), (n, Some(Outcome::Timeout)));
    }

    let mut w = arena(DUEL, &[0, 1], None);
    for _ in 0..61 * HZ {
        w.step(&[], &[]);
    }

    assert_eq!(w.outcome(), None);
    w.stop();
    assert_eq!(w.outcome(), Some(Outcome::Stopped));
}

#[test]
fn validates_configuration_once() {
    for bad in ["", "0", "9", "10.5", "-10", "+20"] {
        assert!(bad.parse::<ClockRules>().is_err());
    }

    assert_eq!("75".parse::<ClockRules>().unwrap().seconds(), Some(75));
    assert!(ClockRules::with_time_limit(u64::MAX).is_err());
    let map: ArenaMap = SWITCHYARD.replace('\n', "\r\n").parse().unwrap();
    assert_eq!(map.spawns().len(), 8);
    for teams in [&[0][..], &[2, 2], &[0, 1, 2, 3, 4, 5, 6, 7, 8]] {
        assert!(Roster::new(seats(teams), &map).is_err());
    }

    assert!(Roster::new(seats(&[1, 1, 2, 2, 2]), &map).is_ok());
    for invalid in [0, 9, u32::MAX] {
        let mut invalid_seats = seats(&[1, 2]);
        invalid_seats[0].slot = SlotId(invalid);
        assert!(matches!(
            Roster::new(invalid_seats, &map),
            Err(ConfigError::SlotRange)
        ));
    }

    for bad in [
        "##\n##",
        "#####\n#>#<#\n#####",
        "#####\n#>.<.\n#####",
        "#####\n#?!<#\n#####",
    ] {
        assert!(bad.parse::<ArenaMap>().is_err());
    }
}

#[test]
fn simultaneous_forfeits() {
    let mut w = arena(SWITCHYARD, &[2, 2, 3, 3, 3], None);
    w.forfeit(&[SlotId(1)]);
    assert_eq!(w.outcome(), None);
    w.forfeit(&[SlotId(2), SlotId(3), SlotId(4), SlotId(5)]);
    assert_eq!(w.outcome(), Some(Outcome::AllForfeited));
    assert_eq!(w.tick(), 0);
}

#[test]
fn last_step_kill_beats_timeout() {
    let mut probe = arena(DUEL, &[0, 1], None);
    hits(&mut probe, &[fire(1)]);
    let flight = probe.tick();
    for seconds in [10, 60] {
        let mut w = arena(DUEL, &[0, 1], Some(seconds));
        let last = seconds * u64::from(HZ);
        for _ in 0..last - flight {
            w.step(&[], &[]);
        }

        hits(&mut w, &[fire(1)]);

        assert_eq!(
            (w.tick(), w.outcome()),
            (last, Some(Outcome::Winner(TeamId(0))))
        );
    }
}

#[test]
fn mirrored_bank_shot_and_reordered_seats_do_not_change_victor() {
    let mirrored = BANK
        .lines()
        .map(|s| {
            s.chars()
                .rev()
                .map(|c| match c {
                    '1' => '3',
                    '3' => '1',
                    '>' => '<',
                    '<' => '>',
                    x => x,
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut w = arena(BANK, &[0, 1], None);
    let mut other = World::new(
        Arc::new(mirrored.parse().unwrap()),
        seats(&[0, 1]).into_iter().rev().collect(),
        ClockRules::unlimited(),
    )
    .unwrap();
    hits(&mut w, &[fire(1)]);
    hits(&mut other, &[fire(1)]);
    assert_eq!((w.tick(), w.outcome()), (other.tick(), other.outcome()));
}

#[test]
fn ammo_slots_and_lifetime_share_a_physical_clock() {
    let edge = format!("{}+", "+-".repeat(62));
    let open = format!("{}+", "+ ".repeat(62));
    let floor = format!("|{}|", vec!["."; 62].join(" "));
    let row = floor.replacen('.', ">", 1);
    let other = floor.replacen('.', "<", 1);
    let map = format!("{edge}\n{row}\n{open}\n{floor}\n{open}\n{other}\n{edge}");
    let mut w = arena(&map, &[0, 1], None);
    let mut state = Vec::new();
    let spacing = HZ / 5;
    for i in 0..HZ * 12 {
        // 成功点射五次后，空仓扣下扳机并保持到炮弹过期。
        let down = if i.is_multiple_of(spacing) && i / spacing <= u32::from(AMMO) {
            Some(true)
        } else if i % spacing == 1 && i / spacing < u32::from(AMMO) {
            Some(false)
        } else {
            None
        };
        w.step(
            &[(
                SlotId(1),
                ControlUpdate {
                    fire: down,
                    ..Default::default()
                },
            )],
            &[],
        );
        if (i + 1).is_multiple_of(HZ / 5) {
            state.push((w.remaining_ammo(SlotId(1)).unwrap(), w.shells().to_vec()));
        }
    }

    assert_eq!(state[usize::from(AMMO) - 1].0, 0);
    assert_eq!(w.remaining_ammo(SlotId(1)).unwrap(), AMMO);
    assert!(w.tanks()[0].fire_down);
    assert!(!w.sight(SlotId(1)).unwrap().can_fire);
    w.step(&[fire(1)], &[]);

    assert!(w.shells().is_empty());
    w.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(false),
                ..Default::default()
            },
        )],
        &[],
    );
    w.step(&[fire(1)], &[]);
    assert_eq!(w.shells().len(), 1);
}

#[test]
fn quarter_turn_of_the_maze_preserves_a_bank_shot() {
    let rows = BANK.lines().map(str::as_bytes).collect::<Vec<_>>();
    let rotated = (0..rows[0].len())
        .map(|x| {
            rows.iter()
                .rev()
                .map(|r| match r[x] {
                    b'1' => '3',
                    b'^' => '>',
                    b'|' => '-',
                    b'-' => '|',
                    cell => cell as char,
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut a = arena(BANK, &[0, 1], None);
    let mut b = arena(&rotated, &[0, 1], None);
    let ea = hits(&mut a, &[fire(1)]);
    let eb = hits(&mut b, &[fire(1)]);
    let spins = |es: &[Event]| {
        es.iter()
            .filter(|e| matches!(e, Event::Bounce { .. }))
            .count()
    };
    assert!(spins(&ea) > 0);
    assert_eq!(spins(&ea), spins(&eb));
    assert_eq!(a.outcome(), Some(Outcome::Winner(TeamId(0))));
    assert_eq!((a.outcome(), a.tick()), (b.outcome(), b.tick()));
}

#[test]
fn team_starts_have_turning_clearance_and_alternative_exits() {
    for teams in [&[1, 2, 1, 2][..], &[1, 2, 1, 2, 2]] {
        let mut w = arena(SWITCHYARD, teams, None);
        let starts = w.tanks().to_vec();
        for t in &starts {
            let (x, y) = (t.center.x / CELL, t.center.y / CELL);
            let exits = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                .into_iter()
                .filter(|(dx, dy)| {
                    w.map().passage(
                        (x as usize, y as usize),
                        ((x + dx) as usize, (y + dy) as usize),
                    )
                })
                .count();
            assert!(exits >= 2, "slot {:?} has only {exits} exits", t.seat.slot);
            assert!(!w.sight(t.seat.slot).unwrap().enemy_ahead);
        }

        let controls = starts
            .iter()
            .map(|t| {
                (
                    t.seat.slot,
                    ControlUpdate {
                        turn: Some(Turn::Right),
                        ..ControlUpdate::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        for i in 1..=256 {
            w.step(&controls, &[]);
            for (a, b) in starts.iter().zip(w.tanks()) {
                assert_eq!(a.center, b.center);
                assert_eq!(
                    b.heading.0,
                    a.heading.0.wrapping_add(Heading::STEP.wrapping_mul(i))
                );
            }
        }
    }
}

#[test]
fn thin_map_checks_openings_not_just_floor_connectivity() {
    let open = "+-+-+-+
|> . <|
+ + + +
|. . .|
+ + + +
|. . .|
+-+-+-+";
    let m: ArenaMap = open.replace('\n', "\r\n").parse().unwrap();
    assert_eq!(m.wall_rectangles().count(), 4);
    assert!(m.passage((0, 0), (0, 1)));
    assert!(!m.passage((0, 0), (1, 1)));
    for data in [
        open.replacen("+-+", "+ +", 1),
        open.replacen('|', " ", 1),
        open.replacen("+ + + +", "+-+-+-+", 1),
        open.replacen('>', "#", 1),
    ] {
        assert!(data.parse::<ArenaMap>().is_err(), "{data}");
    }
}

#[test]
fn bundled_thin_maze_bank_shot_works() {
    let map = include_str!("../assets/maps/bank-shot.map");
    let mut w = arena(map, &[1, 2], None);
    let aim = ControlUpdate {
        turn: Some(Turn::Right),
        ..ControlUpdate::default()
    };
    while w.tanks()[0].heading.bearing_units() < 32.5 {
        w.step(&[(SlotId(1), aim)], &[]);
    }

    assert!(!w.sight(SlotId(1)).unwrap().enemy_ahead);
    let events = hits(
        &mut w,
        &[(
            SlotId(1),
            ControlUpdate {
                turn: Some(Turn::None),
                fire: Some(true),
                ..Default::default()
            },
        )],
    );
    assert!(events.iter().any(|e| matches!(e, Event::Bounce { .. })));
    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(1))));
}

#[test]
fn duplicated_seats_and_insufficient_spawns_are_distinct_from_unbalanced_teams() {
    let small: ArenaMap = include_str!("../assets/maps/bank-shot.map")
        .parse()
        .unwrap();
    let mut duplicate = seats(&[1, 2]);
    duplicate[1].slot = duplicate[0].slot;
    assert_eq!(
        Roster::new(duplicate, &small).unwrap_err(),
        ConfigError::Duplicate
    );
    assert_eq!(
        Roster::new(seats(&[1, 2, 2]), &small).unwrap_err(),
        ConfigError::Spawns
    );
    let full: ArenaMap = SWITCHYARD.parse().unwrap();
    assert!(Roster::new(seats(&[1, 2, 2, 2, 2, 2, 2, 2]), &full).is_ok());
}

#[test]
fn held_trigger_requires_a_release_and_shoots_from_the_pre_move_muzzle() {
    let mut w = arena(DUEL, &[0, 1], None);
    let id = SlotId(1);
    let idle = w.tanks()[1];
    let start = w.tanks()[0];
    w.step(&[], &[]);

    assert_eq!(w.tanks()[0], start);
    let press = ControlUpdate {
        drive: Some(Drive::Forward),
        turn: Some(Turn::Right),
        fire: Some(true),
    };
    w.step(&[(id, press)], &[]);

    assert!(w.events().iter().any(|e| matches!(e, Event::Fired { .. })));
    let mut last = w.tanks()[0];
    assert!(last.fire_down);

    // 按本步开始时的姿态开火，不使用移动或转向后的位置和朝向。
    let shell = w.shells()[0];
    assert_eq!(shell.center.y, start.center.y);
    assert_eq!(
        shell.center.x - start.center.x,
        SIZES.muzzle + SIZES.shell_speed
    );
    assert_eq!(shell.velocity.y, 0);

    // 重复提交 true 或缺席更新都不能再次触发开火。
    w.step(&[fire(1)], &[]);
    for _ in 0..12 {
        assert_ne!(w.tanks()[0].center, last.center);
        assert_ne!(w.tanks()[0].heading, last.heading);
        assert_eq!(w.tanks()[1], idle);
        last = w.tanks()[0];
        w.step(&[], &[]);
    }

    assert!(!w.sight(id).unwrap().can_fire);
    assert_eq!(w.shells().len(), 1);
    w.step(
        &[(
            id,
            ControlUpdate {
                fire: Some(false),
                ..Default::default()
            },
        )],
        &[],
    );

    assert!(w.sight(id).unwrap().can_fire);
    w.step(&[fire(1)], &[]);

    assert_eq!(w.shells().len(), 2);
    let before = w.tanks()[0];
    w.step(
        &[(
            id,
            ControlUpdate {
                drive: Some(Drive::Stop),
                ..Default::default()
            },
        )],
        &[],
    );

    assert_eq!(w.tanks()[0].center, before.center);
    assert_ne!(w.tanks()[0].heading, before.heading);
    let heading = w.tanks()[0].heading;
    w.step(
        &[(
            id,
            ControlUpdate {
                turn: Some(Turn::None),
                ..Default::default()
            },
        )],
        &[],
    );
    w.step(&[], &[]);

    assert_eq!(
        (w.tanks()[0].center, w.tanks()[0].heading),
        (before.center, heading)
    );
}

/// 独立的物理验收值，单位为上游客户端的逻辑像素。
#[test]
fn online_balance_is_observable() {
    let map = "+-+-+-+-+-+-+-+-+
|. . . > . . . .|
+ + + + + + + + +
|. . . . . . . .|
+ + + + + + + + +
|. . . . . . . <|
+-+-+-+-+-+-+-+-+";
    let px = CELL / 200;
    let mut w = arena(map, &[1, 2], None);
    let start = w.tanks()[0].center;
    let second = |w: &mut World, input| {
        w.step(&[(SlotId(1), input)], &[]);
        for _ in 1..HZ {
            w.step(&[], &[]);
        }
    };
    second(
        &mut w,
        ControlUpdate {
            drive: Some(Drive::Forward),
            ..Default::default()
        },
    );

    assert_eq!(w.tanks()[0].center.x - start.x, 319 * px);
    second(
        &mut w,
        ControlUpdate {
            drive: Some(Drive::Reverse),
            ..Default::default()
        },
    );

    assert_eq!(w.tanks()[0].center.x - start.x, (319 - 256) * px);
    let x = w.tanks()[0].center.x;
    second(
        &mut w,
        ControlUpdate {
            drive: Some(Drive::Stop),
            fire: Some(true),
            ..Default::default()
        },
    );

    assert_eq!(w.shells()[0].center.x - x, (50 + 360) * px);
    assert_eq!(w.shells()[0].life, 9 * HZ as u16);
    second(
        &mut w,
        ControlUpdate {
            turn: Some(Turn::Right),
            ..Default::default()
        },
    );

    assert!((w.tanks()[0].heading.radians() - 5.).abs() < 1e-6);
    let map: ArenaMap = "+-+-+-+-+-+
|> . . . <|
+ +-+-+-+ +
|. . . . .|
+ +-+-+-+ +
|. . . . .|
+-+-+-+-+-+"
        .parse()
        .unwrap();
    let at = |y| {
        map.wall_rectangles()
            .find(|r| r.min.y < y && r.max.y > y && r.min.x < 2 * CELL && r.max.x > 2 * CELL)
            .unwrap()
    };
    let (outer, a, b) = (at(0), at(CELL), at(2 * CELL));
    assert_eq!((outer.min.y, outer.max.y), (-8 * px, 8 * px));
    assert_eq!(a.max.y - a.min.y, 16 * px);
    assert_eq!(b.min.y - a.max.y, 184 * px);
    assert_eq!(
        (2 * SIZES.half_width, 2 * SIZES.half_length),
        (60 * px, 80 * px)
    );
    assert_eq!(
        (
            2 * SIZES.turret_half_width,
            2 * SIZES.turret_half_length,
            SIZES.turret_offset
        ),
        (14 * px, 28 * px, 40 * px)
    );
    assert_eq!((SIZES.shell_radius, AMMO, LIFE), (5 * px, 5, 600));
}

#[test]
fn referee_runs_at_sixty_ticks_per_second() {
    assert_eq!(HZ, 60);
    assert_eq!(ClockRules::default().ticks(), Some(3600));
}

#[test]
fn batch_sights_follow_roster_order_even_with_empty_slots_and_dead_owners() {
    let seats = vec![
        Seat {
            slot: SlotId(8),
            team: TeamId(0),
        },
        Seat {
            slot: SlotId(2),
            team: TeamId(1),
        },
    ];
    let mut world = World::new(
        Arc::new(DUEL.parse().unwrap()),
        seats,
        ClockRules::unlimited(),
    )
    .unwrap();
    for missing in [SlotId(0), SlotId(1), SlotId(9), SlotId(u32::MAX)] {
        assert!(world.tank(missing).is_none());
        assert!(world.sight(missing).is_none());
        assert!(world.remaining_ammo(missing).is_none());
    }

    world.step(&[fire(8)], &[]);
    world.forfeit(&[SlotId(8)]);
    let sights: Vec<_> = world.sights().collect();
    assert_eq!(
        sights,
        [
            world.sight(SlotId(2)).unwrap(),
            world.sight(SlotId(8)).unwrap()
        ]
    );
    assert_eq!(sights[0].ammo, AMMO);
    assert_eq!(sights[1].ammo, AMMO - 1);
    assert!(!sights[1].enemy_ahead && !sights[1].can_fire);
}
