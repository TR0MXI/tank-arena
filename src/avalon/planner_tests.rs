use super::*;

fn car(p: Point) -> Car {
    Car {
        seat: Seat {
            slot: SlotId(1),
            team: s::TeamId(1),
        },
        pos: p,
        h: Heading(0),
        weapon: None,
        shield: 0,
        boost: 0,
    }
}

#[test]
fn cached_steps_match_referee_for_hits_and_evictions() {
    let map = ArenaMap::generate(92509, [12, 8]).unwrap();
    let mut cache = Moves::new(&map);
    // Include wall-blocked steps, direction reversals, wraparound and changes of boost.
    let mut blocked = false;
    for i in 0..160u32 {
        let mut tank = car(Point {
            x: s::WALL_HALF + SIZES.half_length + (1 + i as i32 % 55) * PX,
            y: CELL / 2,
        })
        .tank();
        tank.heading = Heading(i.wrapping_mul(0xa17d78cb));
        for a in ACTIONS {
            for boost in [false, true] {
                (tank.drive, tank.turn) = a;
                let expected = tank.proposed_pose(&map, boost);
                for _ in 0..2 {
                    assert_eq!(cache.proposed(tank, a, boost), expected);
                }
                blocked |= a.0 != Drive::Stop && expected.0 == tank.center;
            }
        }
    }
    assert!(blocked);
}

#[test]
fn swept_filter_keeps_turret_grazes_near_misses_and_horizon_edge() {
    let mut me = car(Point {
        x: CELL,
        y: 2 * CELL,
    });
    me.boost = 7;
    for heading in [0, 32, 64, 127, 217] {
        me.h = Heading::bearing(heading);
        for t in [1, 7, 8, 12, 55, 180] {
            let span = travel(me, t);
            assert_eq!(
                span,
                (t.min(7) as i32) * (s::FORWARD_SPEED * (100 + s::BOOST_PERCENT) / 100)
                    + (t.saturating_sub(7) as i32) * s::FORWARD_SPEED
            );
            let mut at = me.tank();
            at.center = add(at.center, Point { x: span, y: 0 });
            for dir in [
                Point::default(),
                Point {
                    x: -6 * PX - 1,
                    y: 5 * PX + 1,
                },
                Point { x: 0, y: 60 * PX },
            ] {
                for x in (-80..=80).step_by(4) {
                    for y in (-80..=80).step_by(4) {
                        let p = add(
                            at.center,
                            Point {
                                x: x * PX,
                                y: y * PX,
                            },
                        );
                        let leg = Leg {
                            id: 0,
                            owner: SlotId(2),
                            p,
                            d: dir,
                            r: SIZES.shell_radius,
                            armed: true,
                        };
                        let box_ = Swept::new(leg);
                        let mid = add(
                            p,
                            Point {
                                x: dir.x / 2,
                                y: dir.y / 2,
                            },
                        );
                        let r = SIZES.half_length + 22 * PX + leg.r;
                        if ball::touches(leg, &at)
                            || (t <= 55
                                && (at.center.x - mid.x).abs() < r
                                && (at.center.y - mid.y).abs() < r)
                        {
                            assert!(box_.near(at.center, 0) && box_.near(me.pos, span));
                        }
                    }
                }
            }
        }
    }
}

fn rocking(p: Point, a: Act, b: Act) -> MotionHistory {
    let mut h = MotionHistory::default();
    h.steps.fill(Some((p, a)));
    h.steps[0] = Some((p, b));
    h
}

#[test]
fn settling_is_local_and_axis_specific_and_does_not_delay_the_first_reversal() {
    use Drive::{Forward as F, Reverse as B, Stop as S};
    use Turn::{Left as L, None as N, Right as R};
    let p = Point::default();
    let mut h = rocking(p, (F, L), (F, R));
    assert_eq!(h.settle(p, (F, L)), (F, N)); // keep rolling through aim wobble
    assert_eq!(h.settle(p, (B, R)), (B, R)); // first reversal of the throttle
    h = rocking(p, (F, L), (B, L));
    assert_eq!(h.settle(p, (F, R)), (S, R)); // keep the useful steering
    // No gradual suppression while making ground, or before six observations.
    h.steps[5].as_mut().unwrap().0.x += CELL / 6;
    assert_eq!(h.settle(p, (F, R)), (F, R));
    h.steps[5] = None;
    assert_eq!(h.settle(p, (F, R)), (F, R));
}

#[test]
fn blocked_commands_and_skipped_frames_are_not_evidence_of_rocking() {
    use super::super::tests::{OPEN, view};
    let m = OPEN.parse().unwrap();
    let c = [car(Point { x: CELL, y: CELL })];
    let mut v = view(&m, &c);
    let mut h = MotionHistory::default();
    h.observe(&v, None);
    for t in 51..=57 {
        v.tick = t;
        (v.sight.drive, v.sight.turn) = if t % 2 == 1 {
            (Drive::Forward, Turn::Left)
        } else {
            (Drive::Reverse, Turn::Right)
        };
        h.observe(&v, Some(t - 1)); // the keys changed but neither pose component did
    }
    assert!(h.steps.iter().all(|s| s.unwrap().1 == IDLE));
    h.commit(IDLE, vec![IDLE; 4]);
    v.tick += 2;
    h.observe(&v, Some(v.tick - 2));
    assert!(h.steps.iter().all(Option::is_none) && h.tail.is_empty());
}

#[test]
fn a_quiet_backtrack_can_settle_but_a_live_escape_and_a_shot_keep_the_whole_plan() {
    use super::super::tests::{OPEN, view};
    let m = OPEN.parse().unwrap();
    let me = car(Point {
        x: 2 * CELL,
        y: CELL,
    });
    let mut enemy = car(Point {
        x: 5 * CELL,
        y: CELL,
    });
    enemy.seat = Seat {
        slot: SlotId(2),
        team: s::TeamId(2),
    };
    let c = [me, enemy];
    let mut v = view(&m, &c);
    let goal = Goal {
        point: enemy.pos,
        enemy,
        combat: false,
        task: "flank",
        supply: None,
        tempo: 0.,
        relief_from: None,
    };
    let solve = |v: &View, h: &MotionHistory, fire: bool| {
        let mut log = Thought::default();
        let plan = choose(v, &goal, &[Point::default(); 8], h, 0, fire, &mut log);
        (plan, log)
    };
    let unstressed = MotionHistory::default();
    let opposite = |(d, t)| {
        (
            match d {
                Drive::Forward => Drive::Reverse,
                Drive::Reverse => Drive::Forward,
                d => d,
            },
            match t {
                Turn::Left => Turn::Right,
                Turn::Right => Turn::Left,
                t => t,
            },
        )
    };
    let ((action, _), _) = solve(&v, &unstressed, false);
    assert_eq!(action, (Drive::Forward, Turn::None));
    let h = rocking(me.pos, action, opposite(action));
    let ((settled, tail), _) = solve(&v, &h, false);
    assert_eq!(settled, IDLE);
    assert_eq!(tail[0], action); // only the first step, no multi-tick steering lock
    assert_eq!(solve(&v, &h, true).0, solve(&v, &unstressed, true).0);

    // A crossing round will hit a stationary tank. Prime history *against* the
    // original escape so this really would be a reversal, then compare full plans.
    let balls = [Ball {
        id: 9,
        owner: enemy.seat.slot,
        pos: Point {
            x: me.pos.x,
            y: 90 * PX,
        },
        v: Point {
            x: 0,
            y: SIZES.shell_speed,
        },
        life: 24,
        kind: None,
        deadly: true,
    }];
    v.balls = &balls;
    let (escape, why) = solve(&v, &unstressed, false);
    assert_eq!(why.hazard, Some(9));
    assert_eq!(why.first_hit, None);
    let h = rocking(me.pos, escape.0, opposite(escape.0));
    assert_ne!(h.settle(me.pos, escape.0), escape.0);
    let (actual, why) = solve(&v, &h, false);
    assert_eq!(actual, escape);
    assert_eq!(why.first_hit, None);
}
