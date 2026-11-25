use super::*;
pub(super) const OPEN: &str = "+-+-+-+-+-+-+\n|> . . . . <|\n+ + + + + + +\n|. . . . . .|\n+ + + + + + +\n|. . . . . .|\n+-+-+-+-+-+-+";
fn car(id: u32, team: u32, x: f64, y: f64) -> Car {
    Car {
        seat: Seat {
            slot: SlotId(id),
            team: s::TeamId(team),
        },
        pos: Point {
            x: (x * CELL as f64) as i32,
            y: (y * CELL as f64) as i32,
        },
        h: Heading(0),
        shield: 0,
        boost: 0,
        weapon: None,
    }
}
pub(super) fn view<'a>(m: &'a ArenaMap, c: &'a [Car]) -> View<'a> {
    View {
        map: m,
        tick: 50,
        me: c[0],
        weapon_slots_used: 0,
        sight: Sight {
            drive: Drive::Stop,
            turn: Turn::None,
            enemy_ahead: true,
            ammo: 5,
            can_fire: true,
            fire_down: false,
        },
        cars: c,
        balls: &[],
        crates: &[],
    }
}
fn shoot(v: &View) -> bool {
    fire::trigger(v, &[Point::default(); 8], None, &mut 0).press
}
#[test]
fn weapons_have_individual_offsets_speeds_lives() {
    let m = OPEN.parse().unwrap();
    let c = [car(1, 1, 0.5, 0.5), car(2, 2, 1.5, 0.5)];
    let mut v = view(&m, &c);
    for (w, k, life) in [
        (Item::Laser, ShotKind::Laser, s::LASER_LIFE),
        (Item::Double, ShotKind::Double, s::DOUBLE_LIFE),
        (Item::Shotgun, ShotKind::Pellet, 120),
        (Item::Missile, ShotKind::Missile, s::MISSILE_LIFE),
        (Item::Gatling, ShotKind::Gatling, 120),
        (Item::Frag, ShotKind::Frag, s::FRAG_LIFE),
    ] {
        v.me.weapon = Some(w);
        let b = ball::outgoing(&v);
        assert_eq!((b.kind, b.life), (Some(k), life));
        assert!(shoot(&v));
    }
    v.me.weapon = Some(Item::Double);
    let p = ball::outgoing(&v);
    v.sight.ammo -= 1;
    let q = ball::outgoing(&v);
    assert_eq!((p.pos.y - q.pos.y).abs(), 18 * PX);
    v.me.boost = 10;
    assert_eq!(ball::outgoing(&v).v.x, SIZES.shell_speed * 130 / 100);
}
#[test]
fn friendly_and_shield_block_shot() {
    let m = OPEN.parse().unwrap();
    let mut c = [car(1, 1, 1., 1.), car(2, 2, 2.5, 1.), car(3, 1, 1.7, 1.)];
    assert!(!shoot(&view(&m, &c)));
    c[2].pos.y += CELL;
    assert!(shoot(&view(&m, &c)));
    c[1].shield = 120;
    assert!(!shoot(&view(&m, &c)));
}
#[test]
fn gatling_holds_and_frag_uses_a_second_edge() {
    let m = OPEN.parse().unwrap();
    let mut c = [car(1, 1, 1., 1.), car(2, 2, 3., 1.)];
    c[0].weapon = Some(Item::Gatling);
    let mut v = view(&m, &c);
    v.sight.fire_down = true;
    assert!(shoot(&v));
    c[0].weapon = Some(Item::Frag);
    let mut v = view(&m, &c);
    let mut b = ball::outgoing(&v);
    b.pos.x = c[1].pos.x - b.v.x * s::FUSE as i32;
    let rounds = [b];
    v.balls = &rounds;
    v.sight.fire_down = true;
    v.sight.can_fire = false;
    assert!(!shoot(&v));
    v.sight.fire_down = false;
    v.sight.can_fire = true;
    assert!(shoot(&v));
}
#[test]
fn mine_needs_a_pursuer_and_allied_clearance() {
    let m = OPEN.parse().unwrap();
    let mut c = [car(1, 1, 2., 1.5), car(2, 2, 1., 1.5), car(3, 1, 4., 1.5)];
    c[0].weapon = Some(Item::Mine);
    assert!(shoot(&view(&m, &c)));
    c[2].pos = c[1].pos;
    assert!(!shoot(&view(&m, &c)));
}
#[test]
fn seeker_changes_direction_after_delay() {
    let m = OPEN.parse().unwrap();
    let c = [car(1, 1, 0.5, 1.5), car(2, 2, 3., 2.)];
    let v = view(&m, &c);
    let mut b = ball::outgoing(&v);
    b.kind = Some(ShotKind::Missile);
    b.life = s::MISSILE_LIFE - s::SEEK_DELAY + 1;
    b.pos = Point {
        x: 3 * CELL,
        y: CELL,
    };
    let mut l = None;
    ball::trace(&v, b, 1, |_, x| {
        l = Some(x);
        true
    });
    let l = l.unwrap();
    assert!(l.armed && l.d.y > 0);
}
#[test]
fn active_crates_can_change_the_waypoint() {
    let m = OPEN.parse().unwrap();
    let c = [car(1, 1, 0.5, 1.5), car(2, 2, 5.5, 1.5)];
    for kind in Item::ALL {
        let crates = [CrateDrop {
            item: kind,
            center: Point {
                x: CELL,
                y: CELL * 3 / 2,
            },
        }];
        let mut v = view(&m, &c);
        v.crates = &crates;
        let g = route::mission(
            &v,
            &[Point::default(); 8],
            &[OpponentRead::default(); 8],
            None,
            false,
            false,
        );
        assert_eq!(g.task, kind.key());
    }
}
#[test]
fn empty_lane_advances_and_can_be_replayed() {
    let m = OPEN.parse().unwrap();
    let c = [car(1, 1, 1.5, 1.5), car(2, 2, 5.5, 1.5)];
    let v = view(&m, &c);
    let mut a = Avalon::default();
    let mut b = Avalon::default();
    let (k, l) = a.decide(&v);
    assert_eq!(k, b.decide(&v).0);
    assert_eq!(k.drive, Some(Drive::Forward));
    assert!(l.depth >= 30);
}

#[test]
fn fixed_corridor_ricochet_agrees_with_referee_each_tick() {
    let map: ArenaMap = OPEN.parse().unwrap();
    let seats = vec![car(1, 1, 0., 0.).seat, car(2, 2, 0., 0.).seat];
    let mut w = s::World::new(std::sync::Arc::new(map), seats, s::ClockRules::unlimited()).unwrap();
    let c: Vec<_> = w
        .tanks()
        .iter()
        .map(|t| Car {
            seat: t.seat,
            pos: t.center,
            h: t.heading,
            weapon: None,
            shield: 0,
            boost: 0,
        })
        .collect();
    // Rotate in place first rather than injecting a projectile or reading hidden controls.
    for _ in 0..34 {
        w.step(
            &[(
                SlotId(1),
                ControlUpdate {
                    drive: None,
                    turn: Some(Turn::Right),
                    fire: None,
                },
            )],
            &[],
        );
    }
    let t = w.tanks()[0];
    let mut c = c;
    c[0].h = t.heading;
    let v = view(w.map(), &c);
    let b = ball::outgoing(&v);
    let mut ends = vec![Point::default(); 101];
    let mut bounced = false;
    ball::trace(&v, b, 100, |tick, l| {
        ends[tick] = add(l.p, l.d);
        bounced |= l.armed;
        true
    });
    assert!(bounced);
    let mut checked = 0;
    let mut actual_bounce = false;
    for (i, end) in ends.iter().enumerate().skip(1) {
        w.step(
            &[(
                SlotId(1),
                ControlUpdate {
                    drive: None,
                    turn: Some(Turn::None),
                    fire: Some(i == 1),
                },
            )],
            &[],
        );
        let Some(s) = w.shells().first() else { break };
        assert_eq!(s.center, *end, "tick {i}");
        actual_bounce |= s.deadly_to_owner();
        checked += 1;
    }
    assert!(actual_bounce && checked >= 3);
}

#[test]
fn distant_clear_opening_uses_one_full_rack_slot_not_a_volley() {
    let map = OPEN.parse().unwrap();
    let cars = [car(1, 1, 0.5, 1.5), car(2, 2, 4.5, 1.5)];
    let mut v = view(&map, &cars);
    assert!(shoot(&v));
    v.sight.ammo -= 1;
    assert!(!shoot(&v));
    v.sight.ammo = s::AMMO;
    v.sight.enemy_ahead = false;
    assert!(!shoot(&v));
    let blocked = [cars[0], cars[1], car(3, 1, 2.5, 1.5)];
    assert!(!shoot(&view(&map, &blocked)));
}

#[test]
fn last_slot_waits_for_a_better_opening_unless_a_real_refill_is_due() {
    let map = OPEN.parse().unwrap();
    let cars = [car(1, 1, 1., 1.5), car(2, 2, 2.2, 1.5)];
    let mut v = view(&map, &cars);
    v.sight.ammo = 1;
    let mut round = ball::outgoing(&v);
    round.owner = cars[1].seat.slot;
    round.life = 2;
    v.balls = std::slice::from_ref(&round);
    assert!(!shoot(&v));
    let mut ours = round;
    ours.owner = cars[0].seat.slot;
    v.balls = std::slice::from_ref(&ours);
    assert!(shoot(&v));
    let special = Ball {
        kind: Some(ShotKind::Double),
        ..ours
    };
    v.balls = std::slice::from_ref(&special);
    assert!(!shoot(&v));
    let close = [cars[0], car(2, 2, 1.65, 1.5)];
    let mut v = view(&map, &close);
    v.sight.ammo = 1;
    assert!(shoot(&v));
}

#[test]
fn burst_needs_more_than_a_second_shell_on_the_same_line() {
    let map = OPEN.parse().unwrap();
    let cars = [car(1, 1, 1., 1.5), car(2, 2, 2.1, 1.5)];
    let mut v = view(&map, &cars);
    v.sight.ammo = 4;
    let mut first = ball::outgoing(&v);
    first.pos = add(first.pos, mul(first.v, 12.));
    first.life -= 12;
    let check = |v: &View| fire::trigger(v, &[Point::default(); 8], Some(v.tick - 12), &mut 0);
    v.balls = std::slice::from_ref(&first);
    let repeated = check(&v);
    assert!(!repeated.press, "{}", repeated.reason);
    assert_eq!(repeated.reason, "already covered or unconfirmed");
    // A shell that expires before reaching the target no longer holds the lane.
    let spent = Ball { life: 1, ..first };
    v.balls = std::slice::from_ref(&spent);
    assert!(check(&v).press);
    for unrelated in &[
        Ball {
            owner: cars[1].seat.slot,
            ..first
        },
        Ball {
            kind: Some(ShotKind::Double),
            ..first
        },
    ] {
        v.balls = std::slice::from_ref(unrelated);
        assert!(check(&v).press);
    }
}

#[test]
fn allow_a_fast_finisher_or_a_second_angle_across_a_moving_opponent() {
    let map = OPEN.parse().unwrap();
    for (offset_y, old_bearing, age, expected) in [
        (0., -12i32, 6u16, "close conversion"),
        (0.14, 32, 12, "cut another exit"),
    ] {
        let mut cars = [car(1, 1, 1., 1.5), car(2, 2, 1.65, 1.5 + offset_y)];
        cars[1].h = Heading(1 << 30);
        let mut v = view(&map, &cars);
        v.sight.ammo = 4;
        let mut motion = [Point::default(); 8];
        motion[1] = cars[1].h.direction(s::FORWARD_SPEED);
        v.me.h = Heading((old_bearing as u32) << 24);
        let mut prior = ball::outgoing(&v);
        prior.pos = add(prior.pos, mul(prior.v, f64::from(age)));
        prior.life -= age;
        v.me.h = cars[0].h;
        v.balls = std::slice::from_ref(&prior);
        let order = fire::trigger(&v, &motion, Some(v.tick - u64::from(age)), &mut 0);
        assert!(order.press);
        assert_eq!(order.reason, expected);
    }
}

#[test]
fn screen_only_if_the_chosen_move_uses_the_contested_space() {
    let map = OPEN.parse().unwrap();
    for (dy, bearing, accepted) in [(-0.2, 144, true), (-0.3, 128, false)] {
        let mut cars = [car(1, 1, 1., 1.5), car(2, 2, 1.9, 1.5 + dy)];
        cars[1].h = Heading(bearing << 24);
        let mut v = view(&map, &cars);
        let mut motion = [Point::default(); 8];
        motion[1] = cars[1].h.direction(s::FORWARD_SPEED);
        let order = fire::trigger(&v, &motion, None, &mut 0);
        let screen = order.screen.expect("a shell can interrupt this approach");
        let idle = (Drive::Stop, Turn::None);
        assert!(!screen.used_by(&v, idle, &[idle; 23]));
        let mut bot = Avalon::default();
        bot.previous[1] = Some(sub(cars[1].pos, motion[1]));
        let (keys, thought) = bot.decide(&v);
        assert_eq!(keys.fire, Some(accepted));
        assert_eq!(
            thought.fire_reason,
            if accepted {
                "screen reposition"
            } else {
                "no safe reposition"
            }
        );
        v.sight.ammo = 3;
        assert!(!fire::trigger(&v, &motion, None, &mut 0).press);
    }
}

#[test]
fn corner_requires_sustained_crossfire_a_wall_and_a_safe_move() {
    const DOOR: &str =
        "+-+-+-+-+-+\n|> . . . <|\n+-+-+ +-+-+\n|. . . . .|\n+ + + + + +\n|. . . . .|\n+-+-+-+-+-+";
    let map = DOOR.parse().unwrap();
    let cars = [
        car(1, 1, 2.5, 1.5),
        car(2, 2, 0.5, 1.5),
        car(3, 2, 4.5, 1.5),
    ];
    let mut v = view(&map, &cars);
    let mut t = tactics::Tactics::default();
    let mut goal = route::mission(
        &v,
        &[Point::default(); 8],
        &[OpponentRead::default(); 8],
        None,
        false,
        false,
    );
    for i in 0..8 {
        v.tick = i;
        t.navigate(&v, &mut goal, false);
        if i < 7 {
            assert_ne!(goal.task, "turn behind wall");
        }
    }
    assert_eq!(goal.task, "turn behind wall");
    let corner = car(1, 1, 2.5, 0.5).pos;
    assert_eq!(goal.point, corner);
    let still = (Drive::Stop, Turn::None);
    assert!(!tactics::improves(
        &v,
        corner,
        (still, &[], Some(6)),
        (still, &[], None)
    ));
    assert!(!tactics::improves(
        &v,
        corner,
        (still, &[], None),
        (still, &[], None)
    ));
    let mut one = view(&map, &cars[..2]);
    let mut t = tactics::Tactics::default();
    for i in 0..15 {
        one.tick = i;
        let mut g = route::mission(
            &one,
            &[Point::default(); 8],
            &[OpponentRead::default(); 8],
            None,
            false,
            false,
        );
        t.navigate(&one, &mut g, false);
        assert_ne!(g.task, "turn behind wall");
    }
}

#[test]
fn stockpile_respects_fifo_nonstacking_and_live_engagement() {
    let map = OPEN.parse().unwrap();
    let cars = [car(1, 1, 2.5, 1.5), car(2, 2, 3.7, 1.5)];
    let mut v = view(&map, &cars);
    let choose = |v: &View, firing| {
        route::mission(
            v,
            &[Point::default(); 8],
            &[OpponentRead::default(); 8],
            None,
            firing,
            false,
        )
        .supply
    };
    let near = CrateDrop {
        item: Item::Laser,
        center: car(3, 1, 2.1, 1.5).pos,
    };
    let boxes = [near];
    v.crates = &boxes;
    v.weapon_slots_used = 2;
    assert_eq!(choose(&v, false), Some(near));
    v.weapon_slots_used = 3;
    assert_eq!(choose(&v, false), None);
    for (item, buff) in [(Item::Shield, true), (Item::Speed, false)] {
        let b = [CrateDrop { item, ..near }];
        let mut test = view(&map, &cars);
        test.weapon_slots_used = 3;
        test.crates = &b;
        assert_eq!(choose(&test, false), Some(b[0]));
        if buff {
            test.me.shield = 300;
        } else {
            test.me.boost = 300;
        }
        assert_eq!(choose(&test, false), None);
    }
    let far = [CrateDrop {
        item: Item::Shield,
        center: car(3, 1, 0.5, 0.5).pos,
    }];
    v.crates = &far;
    assert_eq!(choose(&v, true), None);
}

#[test]
fn mine_withdrawal_orients_the_rear_and_does_not_chain_blast_an_ally() {
    let map = OPEN.parse().unwrap();
    let mut cars = [car(1, 1, 2., 1.5), car(2, 2, 1., 1.5), car(3, 1, 4.5, 2.5)];
    cars[0].weapon = Some(Item::Mine);
    let mut v = view(&map, &cars);
    let g = route::mission(
        &v,
        &[Point::default(); 8],
        &[OpponentRead::default(); 8],
        None,
        false,
        false,
    );
    assert_eq!(g.task, "mine withdrawal");
    assert!(dist(g.point, cars[1].pos) > dist(v.me.pos, cars[1].pos));
    assert!(shoot(&v));
    let old = Ball {
        id: 10,
        owner: v.me.seat.slot,
        pos: cars[2].pos,
        v: Point::default(),
        life: 1000,
        kind: Some(ShotKind::Mine),
        deadly: true,
    };
    let mines = [
        old,
        Ball {
            id: 11,
            pos: cars[1].pos,
            ..old
        },
    ];
    v.balls = &mines;
    assert!(!shoot(&v));
}

#[test]
fn off_axis_missile_has_a_confirmed_acquisition_after_arming() {
    let m = OPEN.parse().unwrap();
    let mut cars = [car(1, 1, 0.5, 1.5), car(2, 2, 4., 0.6)];
    cars[0].weapon = Some(Item::Missile);
    cars[0].h = Heading((-16i32 as u32) << 24);
    let mut v = view(&m, &cars);
    v.tick = 48;
    let mut horizon = 0;
    let o = fire::trigger(&v, &[Point::default(); 8], None, &mut horizon);
    assert!(o.press);
    assert_eq!(o.reason, "seeker acquisition");
    assert!(horizon >= s::SEEK_DELAY as usize);
    cars[1].shield = 300;
    let mut v = view(&m, &cars);
    v.tick = 48;
    assert!(!shoot(&v));
}

// Independent passes of the same geometric threat on a real map. No opponent
// keys, script name or classifier labels enter the observation stream.
fn observe_passes(dodge: bool, shield: bool, gap: bool, reuse_id: bool) -> OpponentRead {
    let map = OPEN.parse().unwrap();
    let mut book = opponents::Book::default();
    let mut result = OpponentRead::default();
    for t in 0..110u64 {
        let age = t % 60;
        if gap && age == 5 {
            continue;
        }
        let mut cars = [car(1, 1, 0.6, 1.3), car(2, 2, 3.2, 1.3)];
        cars[1].h = Heading::bearing(64);
        if shield {
            cars[1].shield = 60;
        }
        let mut motion = [Point::default(); 8];
        if dodge && age >= 7 {
            motion[1] = cars[1].h.direction(s::FORWARD_SPEED);
            cars[1].pos = add(cars[1].pos, mul(motion[1], (age - 6).min(32) as f64));
        }
        let mut v = view(&map, &cars);
        let id = if reuse_id { 1 } else { 1 + t / 60 };
        let mut shell = ball::outgoing(&v);
        shell.id = id;
        shell.pos = add(
            Point {
                x: 2 * CELL,
                y: cars[0].pos.y,
            },
            mul(shell.v, age as f64),
        );
        let rounds = [shell];
        v.balls = &rounds;
        v.tick = t;
        let last = t.checked_sub(if gap && age == 6 { 2 } else { 1 });
        result = book.observe(&v, &motion, last)[1];
        if t == 48 {
            assert_eq!(result.evasion, 0., "one trial is not a read");
        }
    }
    result
}
#[test]
fn read_times_observed_evasion_and_separates_it_from_a_censored_window() {
    let fast = observe_passes(true, false, false, false);
    assert_eq!(fast.dodge_samples, 2);
    assert!(fast.evasion > 0.25);
    assert!(matches!(fast.last_response_ticks, Some(1..=8)));
    assert_eq!(fast.late_windows, 0);
    let slow = observe_passes(false, false, false, false);
    assert_eq!(slow.dodge_samples, 2);
    assert!(slow.evasion < -0.25);
    assert_eq!((slow.last_response_ticks, slow.late_windows), (None, 2));
    assert_eq!(observe_passes(false, true, false, false).dodge_samples, 0);
    assert_eq!(observe_passes(true, false, true, false).dodge_samples, 0);
    assert_eq!(observe_passes(true, false, false, true).dodge_samples, 1);
}
#[test]
fn read_grades_first_contact_not_silence_and_forgets_old_evidence() {
    let map = OPEN.parse().unwrap();
    let mut c = [car(1, 1, 2., 1.5), car(2, 2, 3.0, 1.5), car(3, 2, 3., 2.5)];
    c[1].h = Heading::bearing(128);
    let mut book = opponents::Book::default();
    let sample = |book: &mut opponents::Book, c: &[Car], t: u64, fresh: bool| {
        let mut v = view(&map, c);
        v.tick = t;
        let shooter_view = View { me: c[1], ..v };
        let mut b = ball::outgoing(&shooter_view);
        b.id = t;
        b.life = s::LIFE - if fresh { 1 } else { 2 };
        b.pos = add(b.pos, b.v);
        let rounds = [b];
        v.balls = &rounds;
        book.observe(&v, &[Point::default(); 8], Some(t - 1))
    };
    assert_eq!(sample(&mut book, &c, 50, true)[1].shooting, 0.);
    assert_eq!(sample(&mut book, &c, 52, true)[1].shot_samples, 1); // one burst
    let reads = sample(&mut book, &c, 100, true);
    assert!(reads[1].shooting > 0.3);
    assert_eq!(reads[2].shot_samples, 0);
    assert_eq!(sample(&mut book, &c, 3600, false)[1].shooting, 0.);
    // Clear, short friendly-first contact. Do not reward the victim behind it.
    let mut bad = opponents::Book::default();
    c[2].pos = car(3, 2, 2.38, 1.5).pos;
    sample(&mut bad, &c, 50, true);
    assert!(sample(&mut bad, &c, 100, true)[1].shooting < -0.3);
    c[1].h = Heading::bearing(64);
    assert_eq!(sample(&mut bad, &c, 150, true)[1].shot_samples, 2); // ambiguous miss
}
#[test]
fn unopposed_exposure_requires_distinct_windows_and_room_to_move() {
    let map = OPEN.parse().unwrap();
    let mut c = [car(1, 1, 1., 1.5), car(2, 2, 2.7, 1.5)];
    let mut book = opponents::Book::default();
    let mut result = OpponentRead::default();
    for t in 0..170u64 {
        c[0].h = if (20..80).contains(&t) || t >= 104 {
            Heading(0)
        } else {
            Heading::bearing(64)
        };
        let mut v = view(&map, &c);
        v.tick = t;
        result = book.observe(&v, &[Point::default(); 8], t.checked_sub(1))[1];
        if t == 79 {
            assert_eq!(result.exposed_windows, 1);
            assert_eq!(result.tempo(), 0.);
        }
    }
    assert_eq!(result.exposed_windows, 2);
    assert!(result.tempo() > 0.25);
    let walled = "+-+-+-+-+-+-+\n|> .|. . . <|\n+ + + + + + +\n|. .|. . . .|\n+ + + + + + +\n|. . . . . .|\n+-+-+-+-+-+-+".parse().unwrap();
    let mut book = opponents::Book::default();
    for t in 0..120u64 {
        let mut v = view(&walled, &c);
        v.tick = t;
        v.sight.enemy_ahead = true; // might refer to another car
        assert_eq!(
            book.observe(&v, &[Point::default(); 8], t.checked_sub(1))[1].exposed_windows,
            0
        );
    }
}
#[test]
fn vulnerable_open_bearing_can_break_a_target_tie_but_not_cancel_crossfire() {
    let map = OPEN.parse().unwrap();
    let mut c = [
        car(1, 1, 1.5, 1.5),
        car(2, 2, 1.5, 2.4),
        car(3, 2, 2.5, 1.5),
    ];
    let mut reads = [OpponentRead::default(); 8];
    let goal = |c: &[Car], reads: &[OpponentRead; 8], ammo| {
        let mut v = view(&map, c);
        v.sight.ammo = ammo;
        route::mission(&v, &[Point::default(); 8], reads, None, false, false)
    };
    assert_eq!(goal(&c, &reads, 5).enemy.seat.slot, SlotId(2));
    reads[2].evasion = -0.55;
    let g = goal(&c, &reads, 5);
    assert_eq!(g.enemy.seat.slot, SlotId(3));
    assert_eq!(g.relief_from, Some(SlotId(2)));
    assert_eq!(g.tempo, 0.);
    assert_eq!(goal(&c, &reads, 0).relief_from, None);
    c[2].shield = 10;
    assert_eq!(goal(&c, &reads, 5).relief_from, None);
    c[2].shield = 0;
    let mut duel = [c[0], c[2]];
    assert!(goal(&duel, &reads, 5).tempo > 0.);
    assert_eq!(goal(&duel, &reads, 1).tempo, 0.);
    reads[2].shooting = 0.7;
    duel[1].h = Heading::bearing(128);
    assert!(goal(&duel, &reads, 5).tempo < 0.); // easy to hit does not mean safe to rush
}

#[test]
fn predictable_patrol_is_an_opening_only_during_its_blind_phase() {
    let map = OPEN.parse().unwrap();
    let mut cars = [car(1, 1, 1., 1.6), car(2, 2, 3.1, 1.3)];
    let mut enemy = cars[1].tank();
    (enemy.drive, enemy.turn) = (Drive::Forward, Turn::Right);
    let mut book = opponents::Book::default();
    let mut motion = [Point::default(); 8];
    let mut pressed = false;
    let mut respected = false;
    for t in 0..239u64 {
        if t == 230 {
            enemy.turn = Turn::None;
        }
        let next = enemy.proposed_pose(&map, false);
        motion[1] = sub(next.0, enemy.center);
        (enemy.center, enemy.heading) = next;
        (cars[1].pos, cars[1].h) = next;
        let mut v = view(&map, &cars);
        v.tick = t;
        let r = book.observe(&v, &motion, t.checked_sub(1));
        let g = route::mission(&v, &motion, &r, None, false, false);
        if t < 120 || t >= 230 {
            assert_eq!(g.tempo, 0.);
        }
        if r[1].consecutive_laps >= 2 {
            if err(cars[1].h, sub(cars[0].pos, cars[1].pos)).abs() < 1.2 {
                assert_eq!(g.tempo, 0.);
                respected = true;
            } else if g.tempo > 0. {
                pressed = true;
            }
        }
    }
    assert!(pressed && respected);
}
