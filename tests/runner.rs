use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tank_arena::{
    bot::{FaultKind, Script},
    runner::{Entrant, LiveMatch},
    sim::*,
};

fn game(scripts: &[(&str, u32)]) -> LiveMatch {
    let mut bots = Vec::new();
    for (i, &(src, t)) in scripts.iter().enumerate() {
        let script = Script::from_source(&format!("Bot {i}"), src).unwrap();
        let seat = Seat {
            slot: SlotId(i as u32 + 1),
            team: TeamId(t),
        };
        bots.push(Entrant::from_script(seat, script));
    }

    LiveMatch::start(
        Arc::new(SWITCHYARD.parse().unwrap()),
        ClockRules::unlimited(),
        bots,
        (),
    )
    .unwrap()
}

fn wait_for(test: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(4);
    while !test() {
        assert!(Instant::now() < end, "worker or match stalled");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn independent_vms_in_all_roster_shapes() {
    let src = r#"local n = 0
        return function(o)
            n += 1
            assert(n <= o.tick + 1 and o.tick_hz == 60 and o.maze ~= nil and o.me.team_id == o.team_id)
            return { drive = 'reverse', turn = 'left', fire = false }
        end"#;
    for teams in [
        vec![0, 1],
        vec![0, 0, 1, 1],
        vec![0, 0, 1, 1, 1],
        vec![0, 0, 0, 0, 1, 1, 1, 1],
        vec![0, 0, 1, 1, 2, 2],
    ] {
        let entries = teams.iter().map(|&t| (src, t)).collect::<Vec<_>>();
        let live = game(&entries);
        wait_for(|| live.latest().tick >= 3 || live.finished());
        let f = live.latest();
        assert_eq!(f.tanks.len(), teams.len());
        assert_eq!(f.outcome, None);
        assert!(f.stats.iter().all(|s| s.completed > 0 && s.fault.is_none()));
        live.stop();
        live.join().unwrap();
    }
}

#[test]
fn script_fault_only_forfeits_its_slot() {
    let live = game(&[
        ("return function(o) o.tanks[1].x=0 end", 0),
        ("return function(_) return nil end", 0),
        ("return function(_) return nil end", 1),
    ]);
    wait_for(|| live.latest().tick >= 2 || live.finished());
    let f = live.latest();
    assert_eq!(f.outcome, None);
    assert!(!f.tanks[0].alive);
    assert_eq!(f.stats[0].fault.as_ref().unwrap().kind, FaultKind::Runtime);
    assert!(f.tanks[1].alive && f.tanks[2].alive);
    live.stop();
    live.join().unwrap();
}

#[test]
fn all_init_failures_finish_at_zero() {
    let live = game(&[("error('load')", 0), ("return 42", 1)]);
    wait_for(|| live.finished());
    let f = live.latest();
    assert_eq!((f.tick, f.outcome), (0, Some(Outcome::AllForfeited)));
    assert!(
        f.stats
            .iter()
            .all(|s| s.fault.as_ref().unwrap().kind == FaultKind::Load)
    );
    live.join().unwrap();
}

#[test]
fn faults_are_distinct_and_other_teammates_keep_playing() {
    let idle = "return function(_) return nil end";
    let live = game(&[
        ("not valid luau !!!", 0),
        ("return function(_) error('decision failed') end", 0),
        ("return function(_) return { drive='jump' } end", 0),
        (idle, 0),
        (idle, 1),
    ]);
    let expected = [FaultKind::Load, FaultKind::Runtime, FaultKind::Action];
    wait_for(|| {
        live.latest()
            .stats
            .iter()
            .take(expected.len())
            .all(|s| s.fault.is_some())
            || live.finished()
    });
    let f = live.latest();
    assert_eq!(f.outcome, None);
    for (i, expected) in expected.iter().enumerate() {
        assert_eq!(f.stats[i].fault.as_ref().map(|e| &e.kind), Some(expected));
        assert!(!f.tanks[i].alive);
    }

    assert!(f.tanks[expected.len()..].iter().all(|t| t.alive));
    live.stop();
    live.join().unwrap();
}

#[test]
fn stop_interrupts_loops_in_setup_and_decision() {
    for (script, in_decision) in [
        ("while true do end", false),
        ("return function() while true do end end", true),
    ] {
        let live = game(&[(script, 0), ("return function() end", 1)]);
        if in_decision {
            wait_for(|| live.latest().tick >= 3 || live.finished());
            let frame = live.latest();
            assert!(frame.tanks[0].alive && frame.stats[0].fault.is_none());
            assert!(frame.stats[0].missed > 0);
        }
        live.stop();
        let joined = thread::spawn(move || live.join().unwrap());
        wait_for(|| joined.is_finished());
        joined.join().unwrap();
    }
}

#[test]
fn native_and_namesake_luau_submit_distinct_recorded_controls() {
    use tank_arena::{
        replay::{Playback, RecordStatus, Recorder},
        runner::Decision,
    };
    let dir = tempfile::tempdir().unwrap();
    let native_slot = SlotId(7); // sparse and deliberately out of roster order
    let builtin = Entrant::avalon(Seat {
        slot: native_slot,
        team: TeamId(1),
    });
    let script =
        Script::from_source("Avalon", "return function() return {turn='right'} end").unwrap();
    let lua_slot = SlotId(2);
    let legacy = Entrant::from_script(
        Seat {
            slot: lua_slot,
            team: TeamId(2),
        },
        script,
    );
    let expected =
        [&legacy, &builtin].map(|e| (e.name.clone(), e.fingerprint().unwrap().to_string()));
    let (rec, status) = Recorder::new(dir.path());
    let game = LiveMatch::start_with_items(
        Arc::new(SWITCHYARD.parse().unwrap()),
        ClockRules::unlimited(),
        vec![builtin, legacy],
        ItemPool::ALL,
        rec,
    )
    .unwrap();
    wait_for(|| game.latest().stats.iter().all(|s| s.completed > 0) || game.finished());
    let frame = game.latest();
    assert!(
        frame
            .stats
            .iter()
            .all(|s| s.fault.is_none() && s.completed > 0)
    );
    game.stop();
    game.join().unwrap();
    let RecordStatus::Saved(path) = status.status() else {
        panic!("missing replay")
    };
    let mut replay = Playback::open(&path).unwrap();
    for (info, (name, hash)) in replay.header.roster.iter().zip(expected) {
        assert_eq!(info.name, name);
        assert_eq!(info.script_blake3.as_deref(), Some(hash.as_str()));
    }
    let mut seen = [false; 2];
    while !replay.ended() {
        replay.advance().unwrap();
        for input in replay.applied_inputs() {
            if let Decision::Received { result: Ok(c), .. } = input.decision {
                if input.slot == native_slot {
                    assert!(c.drive.is_some() && c.turn.is_some() && c.fire.is_some());
                    seen[0] = true;
                } else if input.slot == lua_slot {
                    assert_eq!(c.turn, Some(Turn::Right));
                    assert_eq!(c.drive, None);
                    seen[1] = true;
                }
            }
        }
    }
    assert_eq!(seen, [true, true]);
}
