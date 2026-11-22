//! 从指定物理状态开局，单独覆盖在界面上很难精确摆出的临界位置。

use super::*;

const SHELL_RADIUS: i32 = SIZES.shell_radius;
const NOSE: i32 = SIZES.turret_offset + SIZES.turret_half_length;
const CORRIDOR: &str = "+-+-+-+-+-+-+
|> . . . . <|
+ +-+-+-+-+ +
|. . . . . .|
+ +-+-+-+-+ +
|. . . . . .|
+ + + + + + +
|. . . . . .|
+-+-+-+-+-+-+";

fn u(n: i32) -> i32 {
    (i64::from(n) * i64::from(CELL) / 1024) as i32
}

fn spot(x: i32, y: i32) -> Point {
    Point { x: u(x), y: u(y) }
}

fn world(map: &str, teams: &[u32]) -> World {
    let seats = teams
        .iter()
        .enumerate()
        .map(|(i, &team)| Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId(team),
        })
        .collect();
    World::new(
        Arc::new(map.parse().unwrap()),
        seats,
        ClockRules::unlimited(),
    )
    .unwrap()
}

#[test]
fn narrow_corridor_repeats_without_tunneling_or_losing_distance() {
    // 中央走廊上下只相隔一格。射弹留在这里，两端的出生车均在别的列。
    let mut w = world(CORRIDOR, &[0, 1]);
    let origin = Point {
        x: 3 * CELL,
        y: CELL + CELL / 2,
    };
    w.shells.push(Shell {
        id: 1,
        owner: SlotId(1),
        center: origin,
        velocity: Point {
            x: 0,
            y: (CELL - 2 * (WALL_HALF + SHELL_RADIUS)) / 6,
        },
        life: LIFE,
        bounces: [0; BOUNCE_LIMIT],
    });
    let mut bounces = Vec::new();
    for _ in 0..HZ {
        w.step(&[], &[]);
        for event in w.events() {
            if let Event::Bounce { at, .. } = event {
                bounces.push(at.y);
            }
        }

        let y = w.shells()[0].center.y;
        assert!(
            (CELL + WALL_HALF + SHELL_RADIUS..=2 * CELL - WALL_HALF - SHELL_RADIUS).contains(&y)
        );
    }

    let low = CELL + WALL_HALF + SHELL_RADIUS;
    let high = 2 * CELL - WALL_HALF - SHELL_RADIUS;

    // 先用穿越净宽次数，再独立折算最后一段剩余行程。
    assert_eq!(
        bounces,
        [high, low, high, low, high, low, high, low, high, low]
    );
    assert_eq!(
        w.shells()[0].center,
        Point {
            x: origin.x,
            y: origin.y
                + (HZ as i32 * ((CELL - 2 * (WALL_HALF + SHELL_RADIUS)) / 6) - 10 * (high - low))
        }
    );
}

#[test]
fn corner_reflects_both_axes_but_shared_tile_edge_does_not() {
    let map: ArenaMap = CORRIDOR.parse().unwrap();
    // 凹角会同时接触两侧墙面，连续直墙的内部则不应有接缝。
    let face = WALL_HALF + SHELL_RADIUS;
    let p = Point {
        x: face + 15,
        y: face + 15,
    };
    let d = Point { x: -50, y: -50 };
    let hit = walls(&map, p, d, SHELL_RADIUS).unwrap();
    assert_eq!(hit.reflect(d), Point { x: 50, y: 50 });
    let p = Point {
        x: 3 * CELL,
        y: CELL - WALL_HALF - SHELL_RADIUS - 15,
    };
    let d = Point { x: 50, y: 50 };
    let hit = walls(&map, p, d, SHELL_RADIUS).unwrap();
    assert_eq!(hit.reflect(d), Point { x: 50, y: -50 });
    let at = p.plus(hit.t.part(d));
    assert!(walls(&map, at, Point { x: 50, y: 0 }, SHELL_RADIUS).is_none());
}

#[test]
fn shells_outlive_their_owner_while_teams_are_still_contesting() {
    let mut w = world(
        "+-+-+-+-+-+-+-+
|> . . . . . <|
+ + + + + + + +
|> . . . . . .|
+ + + + + + + +
|. . . . . . .|
+-+-+-+-+-+-+-+",
        &[1, 2, 1],
    );
    w.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(true),
                ..ControlUpdate::default()
            },
        )],
        &[],
    );
    w.step(&[], &[SlotId(1)]);

    assert_eq!(w.outcome(), None);
    assert!(!w.tanks()[0].alive);
    assert!(!w.tanks()[0].fire_down);
    assert_eq!(w.shells()[0].owner, SlotId(1));
    for _ in 0..HZ * 4 {
        w.step(&[], &[]);
        if w.outcome().is_some() {
            break;
        }
    }

    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(1))));
    assert!(w.events().iter().any(|e| matches!(
        e,
        Event::Hit {
            owner: SlotId(1),
            target: SlotId(2),
            kind: HitKind::Enemy,
            ..
        }
    )));
}

const PARTITION: &str = "+-+-+-+-+-+
|> .|. . <|
+ + + + + +
|. .|. . .|
+ + + + + +
|. . . . .|
+-+-+-+-+-+";

#[test]
fn thin_partition_is_drivable_on_both_sides_and_open_beyond_the_tip() {
    let mut w = world(PARTITION, &[1, 2]);
    let wall_x = 2 * CELL;
    w.tanks[0].center = Point {
        x: wall_x - CELL / 2,
        y: CELL / 2,
    };
    w.tanks[1].center = Point {
        x: wall_x + CELL / 2,
        y: CELL / 2,
    };

    // 一格内原先会被整块墙挡住的空间，两侧都可用，一直到车身接触实际墙面。
    let drive = ControlUpdate {
        drive: Some(Drive::Forward),
        ..ControlUpdate::default()
    };
    let frames = [(SlotId(1), drive), (SlotId(2), drive)];
    for _ in 0..HZ / 2 {
        w.step(&frames, &[]);
    }

    for t in w.tanks() {
        let gap = (t.center.x - wall_x).abs() - NOSE;
        assert!((WALL_HALF..=WALL_HALF + FORWARD_SPEED).contains(&gap));
    }

    let side = w.tanks[0].center.x;
    w.tanks[0].heading = Heading::bearing(64);
    for _ in 0..HZ * 2 {
        w.step(&[(SlotId(1), drive)], &[]);
    }

    assert_eq!(w.tanks[0].center.x, side);
    assert!(w.tanks[0].center.y > 2 * CELL + WALL_HALF + NOSE);
    w.tanks[0].heading = Heading::bearing(0);
    for _ in 0..HZ / 2 {
        w.step(&[(SlotId(1), drive)], &[]);
    }

    assert!(w.tanks[0].center.x > wall_x + NOSE);
}

#[test]
fn bullet_bounces_at_visible_faces_and_cap_without_reversing_at_grid_seams() {
    let map: ArenaMap = PARTITION.parse().unwrap();
    let face = map
        .wall_rectangles()
        .find(|r| r.min.x == 2 * CELL - WALL_HALF && r.max.x == 2 * CELL + WALL_HALF)
        .unwrap();
    assert_eq!(face.max.y, 2 * CELL + WALL_HALF);
    for (p, d, x, y, at) in [
        (
            Point {
                x: face.min.x - SHELL_RADIUS - 18,
                y: CELL - 18,
            },
            Point { x: 36, y: 36 },
            true,
            false,
            Point {
                x: face.min.x - SHELL_RADIUS,
                y: CELL,
            },
        ),
        (
            Point {
                x: face.max.x + SHELL_RADIUS + 18,
                y: CELL + 18,
            },
            Point { x: -36, y: -36 },
            true,
            false,
            Point {
                x: face.max.x + SHELL_RADIUS,
                y: CELL,
            },
        ),
        (
            Point {
                x: 2 * CELL,
                y: face.max.y + SHELL_RADIUS + 18,
            },
            Point { x: 0, y: -72 },
            false,
            true,
            Point {
                x: 2 * CELL,
                y: face.max.y + SHELL_RADIUS,
            },
        ),
    ] {
        let hit = walls(&map, p, d, SHELL_RADIUS).unwrap();
        assert_eq!(
            hit.reflect(d),
            Point {
                x: if x { -d.x } else { d.x },
                y: if y { -d.y } else { d.y }
            }
        );
        assert_eq!(p.plus(hit.t.part(d)), at);
    }

    let mut w = world(PARTITION, &[1, 2]);
    let start = Point {
        x: face.min.x - SHELL_RADIUS - 18,
        y: CELL,
    };
    w.shells.push(Shell {
        id: 1,
        owner: SlotId(1),
        center: start,
        velocity: Point { x: 72, y: 0 },
        life: LIFE,
        bounces: [0; BOUNCE_LIMIT],
    });
    w.step(&[], &[]);

    assert_eq!(
        w.shells()[0].center.x,
        face.min.x - SHELL_RADIUS - (72 - 18)
    );
    assert_eq!(
        w.events()
            .iter()
            .filter(|e| matches!(e, Event::Bounce { .. }))
            .count(),
        1
    );
    assert!(!w.sight(SlotId(1)).unwrap().enemy_ahead);
}

#[test]
fn near_target_overlapping_the_spawned_round_is_hit_not_the_owner() {
    let mut w = world(PARTITION, &[1, 2]);
    w.tanks[0].center = spot(900, 800);
    w.tanks[0].heading = Heading::bearing(32);
    w.tanks[1].center = spot(1275, 1020);
    w.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(true),
                ..ControlUpdate::default()
            },
        )],
        &[],
    );

    assert_eq!(w.outcome(), Some(Outcome::Winner(TeamId(1))));
    assert!(w.shells().is_empty());
}

#[test]
fn straight_fire_sensor_matches_quantized_velocity_at_a_wall_tip() {
    let mut w = world(SWITCHYARD, &[0, 1]);
    w.tanks[0].center = Point {
        x: u(4610),
        y: u(1041) + CELL,
    };
    w.tanks[0].heading = Heading::bearing(6);
    w.tanks[1].center = Point {
        x: u(6450),
        y: u(1096) + CELL,
    };

    assert!(
        !w.sight(SlotId(1)).unwrap().enemy_ahead,
        "the first impact will be the thin wall cap"
    );
    w.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(true),
                ..ControlUpdate::default()
            },
        )],
        &[],
    );
    for _ in 0..160 {
        if w.events().iter().any(|e| matches!(e, Event::Bounce { .. })) {
            return;
        }
        assert!(w.tanks()[1].alive);
        w.step(&[], &[]);
    }

    panic!("shell did not reach the wall tip");
}

#[test]
fn owner_arming_and_short_bounce_window_are_not_a_total_bounce_limit() {
    let mut s = Shell {
        id: 0,
        owner: SlotId(1),
        center: Point::default(),
        velocity: Point::default(),
        life: LIFE,
        bounces: [0; 5],
    };
    assert!(!s.deadly_to_owner());
    for _ in 0..12 {
        assert!(!s.record_bounce());
        assert!(s.deadly_to_owner());
        s.life -= 8;
    }

    for i in 0..4 {
        assert!(!s.record_bounce());
        s.life -= i % 2;
    }

    assert!(s.record_bounce()); // 两步内五次碰撞，历时约 33.3 毫秒，小于 35 毫秒。
}

#[test]
fn normal_turret_participates_in_hits_and_rotates_with_the_hull() {
    let w = world(PARTITION, &[1, 2]);
    let mut t = w.tanks[0];
    let hit = |t: &Tank, x, y| {
        body(
            t.center.plus(Point {
                x: x * PX,
                y: y * PX,
            }),
            Point::default(),
            t,
            SIZES.shell_radius,
        )
        .is_some()
    };
    assert!(hit(&t, 58, 0));
    assert!(!hit(&t, 60, 0));
    assert!(!hit(&t, 52, 14));
    assert!(hit(&t, 0, 34));
    assert!(!hit(&t, 0, 36));
    t.heading = Heading::bearing(64);

    assert!(hit(&t, 0, 58));
    assert!(!hit(&t, 58, 0));
}

// 把其余炮弹放在远离射击通道的位置，弹药数量仍由裁判计算。
#[test]
fn a_blocked_convoy_cannot_move_into_another_tanks_old_pose() {
    let map = "+-+-+-+-+-+
|> . > . >|
+ + + + + +
|. . . . .|
+ + + + + +
|. . . . .|
+-+-+-+-+-+";
    let spacing = NOSE + SIZES.half_length + FORWARD_SPEED / 2;
    for order in [[0, 1, 2], [2, 1, 0]] {
        let mut w = world(map, &[0, 1, 1]);
        for (tank, position) in w.tanks.iter_mut().zip(order) {
            tank.center = Point {
                x: CELL + position * spacing,
                y: CELL / 2,
            };
            if position < 2 {
                tank.drive = Drive::Forward;
            }
        }

        // 前车静止，中车被挡住时，尾车也不能驶入中车的原位。
        let before = w.tanks.clone();
        w.step(&[], &[]);
        assert_eq!(w.tanks, before);
    }
}
