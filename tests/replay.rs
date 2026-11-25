use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tank_arena::{
    bot::{Fault, FaultKind, Script},
    replay::*,
    runner::*,
    sim::*,
};

fn record(dir: &Path, n: usize, ticks: u64) -> PathBuf {
    record_with_items(dir, n, ticks, ItemPool::NONE)
}
fn record_with_items(dir: &Path, n: usize, ticks: u64, pool: ItemPool) -> PathBuf {
    let seats: Vec<_> = (0..n)
        .map(|i| Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId((i % 2) as u32),
        })
        .collect();
    let bots = seats
        .iter()
        .map(|&seat| Entrant {
            seat,
            name: "idle".into(),
            pilot: Ok(Pilot::Luau(
                Script::from_source("idle", "return function(_) return nil end").unwrap(),
            )),
        })
        .collect::<Vec<_>>();
    let mut w = World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        seats,
        ClockRules::unlimited(),
    )
    .unwrap()
    .with_behavior_seed(Some(55))
    .with_items(pool);
    let (mut r, status) = Recorder::new(dir);
    r.start(&w, &bots, &[]);
    for tick in 0..ticks {
        // 仅提交一次迟到数步的结果，此后跨过两个关键帧都不再提交。
        let update = ControlUpdate {
            turn: Some(Turn::Right),
            ..Default::default()
        };
        let controls = [(SlotId(1), update)];
        let applied: Vec<_> = w
            .tanks()
            .iter()
            .filter(|t| t.alive)
            .map(|t| Applied {
                slot: t.seat.slot,
                decision: if tick == 259 && t.seat.slot == SlotId(2) {
                    Decision::Received {
                        seq: 1,
                        observed_tick: 255,
                        result: Err(Fault {
                            kind: FaultKind::Action,
                            message: "invalid drive".into(),
                        }),
                    }
                } else if tick == 250 && t.seat.slot == SlotId(1) {
                    Decision::Received {
                        seq: 1,
                        observed_tick: 248,
                        result: Ok(update),
                    }
                } else {
                    Decision::NoResult
                },
            })
            .collect();
        w.step(
            if tick == 250 { &controls } else { &[] },
            if tick == 259 { &[SlotId(2)] } else { &[] },
        );
        r.step(tick, &w, &applied);
        if tick % 16 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    w.stop();
    r.finish(&w);
    let RecordStatus::Saved(path) = status.status() else {
        panic!("{:?}", status.status())
    };
    assert!(path.with_extension("result.json").is_file());
    path
}

#[test]
fn seek_over_keyframes_without_scripts() {
    let d = tempfile::tempdir().unwrap();
    for n in [5, 8] {
        let p = record(d.path(), n, 530);
        let mut play = Playback::open(&p).unwrap();
        assert_eq!(play.header.tick_hz, HZ);
        let spawn = play.world().tanks()[0].heading.0;
        for t in [511, 260, 250, 251, 259, 256, 0, 529, 530] {
            play.seek(t).unwrap();
            let tank = play.world().tanks()[0];
            assert_eq!(tank.turn, if t > 250 { Turn::Right } else { Turn::None });
            assert_eq!(
                tank.heading.0,
                spawn.wrapping_add((t.saturating_sub(250) as u32).wrapping_mul(Heading::STEP))
            );
            assert_eq!(play.world().tanks().len(), n);
            assert_eq!(play.world().tick(), t);
            assert_eq!(play.world().tanks()[1].alive, t < 260);
            assert_eq!(play.world().hit_ticks()[1], None, "forfeits do not explode");
        }

        assert!(play.ended());
        assert_eq!(play.world().outcome(), Some(Outcome::Stopped));
    }
}

#[test]
fn demo_contains_real_bounce_and_hit() {
    let mut p = Playback::open(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/first-duel.tankrec")
            .as_path(),
    )
    .unwrap();

    // 反弹射手打出第一发后持续按住扳机；分别从关键帧两侧恢复。
    for (tick, held) in [
        (256, true),
        (0, false),
        (257, true),
        (255, true),
        (0, false),
    ] {
        p.seek(tick).unwrap();
        assert_eq!(p.world().tanks()[0].fire_down, held);
    }

    let mut bounce = None;
    while !p.ended() {
        p.advance().unwrap();
        for e in p.world().events() {
            match e {
                Event::Bounce { shell, .. } => bounce = Some(*shell),
                Event::Hit { shell, kind, .. } => {
                    assert_eq!(Some(*shell), bounce);
                    assert_eq!(*kind, HitKind::Enemy);
                }
                _ => {}
            }
        }
    }

    assert_eq!(p.world().outcome(), Some(Outcome::Winner(TeamId(0))));
}

#[test]
fn recorded_team_matches_restore_owner_and_winner() {
    for (name, n) in [("teams-2v3", 5), ("teams-4v4", 8)] {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}.tankrec"));
        let mut p = Playback::open(&path).unwrap();
        assert_eq!(p.header.roster.len(), n);
        let mut friendly = false;
        while !p.ended() {
            p.advance().unwrap();
            friendly |= p.world().events().iter().any(|e| {
                matches!(
                    e,
                    Event::Hit {
                        owner: SlotId(1),
                        target: SlotId(2),
                        kind: HitKind::Friendly,
                        ..
                    }
                )
            });
        }

        assert!(friendly);
        assert_eq!(p.world().outcome(), Some(Outcome::Winner(TeamId(2))));
    }
}

#[test]
fn destruction_ticks_survive_skipped_frames_and_backward_seeks() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/teams-4v4.tankrec");
    let mut p = Playback::open(&path).unwrap();
    let mut expected = [None; MAX_SLOTS];
    while !p.ended() {
        p.advance().unwrap();
        let w = p.world();
        for e in w.events() {
            if let Event::Hit { target, .. } = e {
                let i = w
                    .tanks()
                    .iter()
                    .position(|t| t.seat.slot == *target)
                    .unwrap();
                expected[i] = Some(w.tick());
            }
        }

        assert_eq!(w.hit_ticks(), &expected);
    }

    assert!(expected.iter().flatten().count() >= 2);
    for &hit in expected.iter().flatten() {
        for tick in [p.length(), hit - 1, hit, 0] {
            p.seek(tick).unwrap();
            assert_eq!(
                *p.world().hit_ticks(),
                expected.map(|t| t.filter(|&h| h <= tick))
            );
        }
    }
}

#[test]
fn bundled_pursuit_replays_two_moving_and_firing_bots() {
    use std::collections::{BTreeMap, BTreeSet};

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pursuit.tankrec");
    let mut replay = Playback::open(&path).unwrap();
    assert_eq!(
        replay
            .header
            .roster
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["sentry", "starter"]
    );
    let mut visits = BTreeMap::<SlotId, BTreeSet<(i32, i32)>>::new();
    let mut shooters = BTreeSet::new();
    while !replay.ended() {
        replay.advance().unwrap();
        for t in replay.world().tanks() {
            visits
                .entry(t.seat.slot)
                .or_default()
                .insert((t.center.x / CELL, t.center.y / CELL));
        }

        for e in replay.world().events() {
            if let Event::Fired { owner, .. } = e {
                shooters.insert(*owner);
            }
        }
    }

    assert!(visits.values().all(|cells| cells.len() >= 3));
    assert_eq!(shooters.len(), 2);
    assert!(replay.world().events().iter().any(|e| matches!(
        e,
        Event::Hit {
            kind: HitKind::Enemy,
            ..
        }
    )));
    assert_eq!(replay.world().outcome(), Some(Outcome::Winner(TeamId(1))));
}

#[test]
#[ignore = "本机 release 长局基准，不将耗时用作测试断言"]
fn long_replay_open_and_seek() {
    use std::{hint::black_box, time::Instant};
    const TICKS: u64 = 7_800;

    let dir = tempfile::tempdir().unwrap();
    let path = record(dir.path(), 8, TICKS);
    let mut play = Playback::open(&path).unwrap();
    assert_eq!(play.length(), TICKS);
    let mut open = Vec::new();
    for _ in 0..31 {
        let start = Instant::now();
        black_box(Playback::open(&path).unwrap());
        open.push(start.elapsed().as_micros());
    }

    // 固定的跨关键帧跳转序列；不仅测总能命中同一缓存块的短跳。
    let mut seek = Vec::new();
    for i in 0..1024 {
        let tick = i * 4_871 % (TICKS + 1);
        let start = Instant::now();
        play.seek(tick).unwrap();
        seek.push(start.elapsed().as_micros());
        assert_eq!(play.world().tick(), tick);
    }

    open.sort_unstable();
    seek.sort_unstable();
    println!(
        "ticks={TICKS} bytes={} open_p50_us={} seek_p50_us={} seek_p99_us={} seek_max_us={}",
        path.metadata().unwrap().len(),
        open[15],
        seek[512],
        seek[1013],
        seek[1023]
    );
}

#[test]
fn staged_match_is_only_archived_by_explicit_save() {
    let root = tempfile::tempdir().unwrap();
    let library_dir = root.path().join("demo");
    for save in [false, true] {
        let stage = tempfile::tempdir().unwrap();
        let rec = record(stage.path(), 2, 8);
        assert!(
            tank_arena::local_files::Folder::Replays
                .scan(&library_dir)
                .unwrap()
                .is_empty()
        );
        if save {
            library::save_staged(&rec, &library_dir).unwrap();
        }
        drop(stage);

        assert!(!rec.exists());
        assert_eq!(
            tank_arena::local_files::Folder::Replays
                .scan(&library_dir)
                .unwrap()
                .len(),
            usize::from(save)
        );
    }
}

#[test]
fn thumbnail_pose_matches_playback_at_one_second() {
    for name in ["first-duel", "teams-2v3"] {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}.tankrec"));
        let (header, preview) = Playback::preview(&path).unwrap();
        let mut play = Playback::open(&path).unwrap();
        play.seek(u64::from(header.tick_hz)).unwrap();

        assert_eq!(preview.tick(), play.world().tick());
        assert_eq!(preview.tanks(), play.world().tanks());
        assert_eq!(preview.shells(), play.world().shells());
    }
}

#[test]
fn seeking_rebuilds_ammo_from_the_restored_shells() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec");
    let mut replay = Playback::open(&fixture).unwrap();
    // 这份公开样本在第 256 步的关键帧里有一颗在飞炮弹，直接恢复时也必须扣除对应弹药。
    replay.seek(256).unwrap();
    assert!(!replay.world().shells().is_empty());

    for tick in [256, replay.length(), 120, 0, 360] {
        replay.seek(tick).unwrap();
        for _ in 0..6 {
            let world = replay.world();
            for tank in world.tanks() {
                let slot = tank.seat.slot;
                let used = world.shells().iter().filter(|s| s.owner == slot).count() as u8;
                assert_eq!(world.remaining_ammo(slot), Some(AMMO - used));
                assert_eq!(world.sight(slot).unwrap().ammo, AMMO - used);
            }

            if replay.ended() {
                break;
            }

            replay.advance().unwrap();
        }
    }
}

#[test]
fn validation_can_be_interrupted_between_records() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec");
    let mut checks = 0;
    let error = Playback::open_while(&fixture, || {
        checks += 1;
        checks <= 5
    })
    .err()
    .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(checks, 6);
}

#[test]
fn crate_pool_and_drop_sequence_survive_seek_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut pool = ItemPool::NONE;
    pool.toggle(Item::Speed);
    let path = record_with_items(dir.path(), 5, 800, pool);
    let mut p = Playback::open(&path).unwrap();
    assert_eq!(p.header.item_pool, pool);
    assert_eq!(p.world().items().pool, pool);
    p.seek(800).unwrap();
    let last = p.world().items().clone();
    assert!(!last.crates.is_empty());
    assert!(last.crates.iter().all(|c| c.item == Item::Speed));
    for from in [512, 256, 0] {
        p.seek(from).unwrap();
        while !p.ended() {
            p.advance().unwrap();
        }
        assert_eq!(p.world().items(), &last);
    }
}
