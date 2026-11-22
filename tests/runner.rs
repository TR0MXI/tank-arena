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
    let live = game(&[("error('load')", 0), ("while true do end", 1)]);
    wait_for(|| live.finished());
    let f = live.latest();
    assert_eq!((f.tick, f.outcome), (0, Some(Outcome::AllForfeited)));
    assert_eq!(f.stats[1].fault.as_ref().unwrap().kind, FaultKind::Budget);
    live.join().unwrap();
}

#[test]
fn faults_are_distinct_and_other_teammates_keep_playing() {
    let idle = "return function(_) return nil end";
    let live = game(&[
        ("not valid luau !!!", 0),
        ("return function(_) error('decision failed') end", 0),
        ("return function(_) return { drive='jump' } end", 0),
        ("return function(_) while true do end end", 0),
        (
            "return function(_) local a=table.create(400000, 1); return a[1] end",
            0,
        ),
        (idle, 0),
        (idle, 1),
        (idle, 1),
    ]);
    wait_for(|| {
        live.latest()
            .stats
            .iter()
            .take(5)
            .all(|s| s.fault.is_some())
            || live.finished()
    });
    let f = live.latest();
    assert_eq!(f.outcome, None);
    for (i, expected) in [
        FaultKind::Load,
        FaultKind::Runtime,
        FaultKind::Action,
        FaultKind::Budget,
        FaultKind::Memory,
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(f.stats[i].fault.as_ref().map(|e| &e.kind), Some(expected));
        assert!(!f.tanks[i].alive);
    }

    assert!(f.tanks[5..].iter().all(|t| t.alive));
    live.stop();
    live.join().unwrap();
}
