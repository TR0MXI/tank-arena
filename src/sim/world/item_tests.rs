use super::*;

const ONE: SlotId = SlotId(1);
fn arena() -> World {
    let map = "+-+-+-+-+-+-+-+-+-+-+\n|> . . . . . . . . .|\n+ + + + + + + + + + +\n|. . . . . . . . . .|\n+ + + + + + + + + + +\n|. . . . . . . . . <|\n+-+-+-+-+-+-+-+-+-+-+";
    World::new(
        Arc::new(map.parse().unwrap()),
        vec![
            Seat {
                slot: ONE,
                team: TeamId(1),
            },
            Seat {
                slot: SlotId(2),
                team: TeamId(2),
            },
        ],
        ClockRules::unlimited(),
    )
    .unwrap()
    .with_behavior_seed(Some(11))
    .with_items(ItemPool::ALL)
}
fn give(w: &mut World, i: Item) {
    w.items.crates.push(CrateDrop {
        item: i,
        center: w.tanks[0].center,
    });
    w.step(&[], &[]);
}
fn fire(w: &mut World, down: bool) {
    w.step(
        &[(
            ONE,
            ControlUpdate {
                fire: Some(down),
                ..Default::default()
            },
        )],
        &[],
    );
}
#[test]
fn drops_only_from_selected_pool_and_never_exceed_limit() {
    let mut pool = ItemPool::NONE;
    pool.toggle(Item::Speed);
    let mut w = arena().with_items(pool);
    let mut off = arena().with_items(ItemPool::NONE);
    let mut copy = w.clone();
    for _ in 0..1600 {
        w.step(&[], &[]);
        copy.step(&[], &[]);
        off.step(&[], &[]);
        assert_eq!(copy.items, w.items);
    }
    assert_eq!(off.items, Items::default());
    assert_eq!(w.items.crates.len(), MAX_CRATES);
    for (i, c) in w.items.crates.iter().enumerate() {
        assert_eq!(c.item, Item::Speed);
        assert!(!w.items.crates[..i].iter().any(|a| a.center == c.center));
        assert!(
            !w.tanks
                .iter()
                .any(|t| body(c.center, Point::default(), t, 32 * PX).is_some())
        );
    }
}
#[test]
fn shotgun_uses_its_own_magazine_and_weapon_queue_is_fifo() {
    let mut w = arena();
    give(&mut w, Item::Shotgun);
    give(&mut w, Item::Double);
    give(&mut w, Item::Missile);
    give(&mut w, Item::Laser);
    assert_eq!(
        w.items.gear(ONE).queue,
        [Item::Shotgun, Item::Double, Item::Missile]
    );
    assert_eq!(w.items.crates.len(), 1);
    fire(&mut w, true);
    assert_eq!(w.shells().len(), usize::from(PELLETS));
    assert_eq!(w.remaining_ammo(ONE), Some(2));
    assert_eq!(w.shells.ammo(ONE), AMMO);
    fire(&mut w, false);
    fire(&mut w, true);
    assert_eq!(w.remaining_ammo(ONE), Some(2));
    // Exercise seek restoration while the number of pellets exceeds the five ordinary slots.
    let c = w.checkpoint();
    let mut recovered = arena();
    recovered.restore_items(w.items.clone());
    recovered.restore(c);
    assert_eq!(recovered.shells.ammo(ONE), AMMO);
    assert_eq!(recovered.shells(), w.shells());
    for _ in 0..2 {
        for _ in 0..HZ {
            fire(&mut w, false);
        }
        fire(&mut w, true);
    }
    assert_eq!(w.items.gear(ONE).weapon(), Some(Item::Double));
    assert_eq!(w.remaining_ammo(ONE), Some(10));
    fire(&mut w, false);
    fire(&mut w, true);
    let y = w.shells().last().unwrap().center.y;
    fire(&mut w, false);
    fire(&mut w, true);
    assert_ne!(w.shells().last().unwrap().center.y, y);
}
#[test]
fn speed_applies_to_motion_turning_and_normal_shots_then_expires() {
    let mut w = arena();
    give(&mut w, Item::Speed);
    let (x, h) = (w.tanks[0].center.x, w.tanks[0].heading);
    w.step(
        &[(
            ONE,
            ControlUpdate {
                drive: Some(Drive::Forward),
                fire: Some(true),
                turn: None,
            },
        )],
        &[],
    );
    assert_eq!(w.tanks[0].center.x - x, FORWARD_SPEED * 130 / 100);
    assert_eq!(w.shells()[0].velocity.x, SIZES.shell_speed * 130 / 100);
    w.step(
        &[(
            ONE,
            ControlUpdate {
                drive: Some(Drive::Stop),
                turn: Some(Turn::Right),
                fire: None,
            },
        )],
        &[],
    );
    assert_eq!(
        w.tanks[0].heading.0 - h.0,
        (u64::from(Heading::STEP) * 130 / 100) as u32
    );
    w.shells = Shells::from_rounds(vec![], &w.items);
    while w.tick < BOOST_TICKS {
        w.step(&[], &[]);
    }
    let h = w.tanks[0].heading;
    w.step(&[], &[]);
    assert_eq!(w.tanks[0].heading.0.wrapping_sub(h.0), Heading::STEP);
}
#[test]
fn remote_frag_mine_and_gatling_have_distinct_triggers() {
    let mut frag = arena();
    give(&mut frag, Item::Frag);
    fire(&mut frag, true);
    assert_eq!(frag.items.gear(ONE).remote, Some(0));
    fire(&mut frag, false);
    fire(&mut frag, true);
    for _ in 1..FUSE {
        frag.step(&[], &[]);
    }
    assert_eq!(frag.shells().len(), usize::from(FRAGMENTS));
    assert!(
        frag.items
            .projectiles
            .values()
            .all(|p| p.kind == ShotKind::Shard)
    );
    assert_eq!(frag.items.gear(ONE).weapon(), None);
    let mut mine = arena();
    give(&mut mine, Item::Mine);
    fire(&mut mine, true);
    for _ in 0..MINE_ARM {
        mine.step(&[], &[]);
    }
    assert_eq!(mine.items.projectiles[&0].fuse, None);
    mine.tanks[1].center = mine.shells()[0].center;
    for _ in 0..FUSE {
        mine.step(&[], &[]);
    }
    assert_eq!(mine.shells().len(), usize::from(FRAGMENTS));
    let mut gatling = arena();
    give(&mut gatling, Item::Gatling);
    fire(&mut gatling, true);
    for _ in 1..GATLING_WARMUP {
        gatling.step(&[], &[]);
    }
    assert!(gatling.shells().is_empty());
    gatling.step(&[], &[]);
    for _ in 0..GATLING_INTERVAL {
        gatling.step(&[], &[]);
    }
    assert_eq!(gatling.shells().len(), 2);
    fire(&mut gatling, false);
    for _ in 0..GATLING_INTERVAL {
        gatling.step(&[], &[]);
    }
    assert_eq!(gatling.remaining_ammo(ONE), Some(18));
}
#[test]
fn shield_reflects_and_laser_leaves_a_ricochet_guide_and_trail() {
    let mut w = arena();
    w.tanks[1].center = Point {
        x: 2 * CELL,
        y: CELL / 2,
    };
    w.items.crates.push(CrateDrop {
        item: Item::Shield,
        center: w.tanks[1].center,
    });
    give(&mut w, Item::Laser);
    assert!(w.items.gear(SlotId(2)).shield_until > 0);
    assert!(!laser_guide(w.map(), &w.tanks[0], w.tanks()).is_empty());
    fire(&mut w, true);
    assert!(!w.items.beams.is_empty());
    for _ in 0..8 {
        w.step(&[], &[]);
        if w.events().iter().any(|e| matches!(e, Event::Bounce { .. })) {
            assert!(w.tanks[1].alive);
            assert!(w.shells()[0].velocity.x < 0);
            return;
        }
    }
    panic!("shield did not reflect laser");
}
#[test]
fn missile_starts_guidance_only_after_delay() {
    let mut w = arena();
    give(&mut w, Item::Missile);
    fire(&mut w, true);
    w.tanks[0].center = Point {
        x: 8 * CELL,
        y: 2 * CELL,
    };
    while w.items.projectiles[&0].age < SEEK_DELAY - 1 {
        w.step(&[], &[]);
    }
    assert_eq!(w.shells()[0].velocity.y, 0);
    w.tanks[1].center = Point {
        x: w.shells()[0].center.x,
        y: CELL + CELL / 2,
    };
    w.step(&[], &[]);
    assert!(w.shells()[0].velocity.y > 0);
    assert!(w.items.projectiles[&0].can_hit_owner());
}

#[test]
fn seeding_before_or_after_selecting_a_pool_agrees() {
    let first = arena()
        .with_items(ItemPool::NONE)
        .with_behavior_seed(Some(100))
        .with_items(ItemPool::ALL);
    let second = arena()
        .with_items(ItemPool::ALL)
        .with_behavior_seed(Some(100));
    assert_eq!(first.items(), second.items());
}
