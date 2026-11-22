use super::*;

fn seats() -> Vec<Entrant> {
    let src = Script::from_source(
        "stateful",
        "local n=0; return function(o) n+=1; assert(n<=o.tick+1); return nil end",
    )
    .unwrap();
    (0..8)
        .map(|i| Entrant {
            seat: Seat {
                slot: SlotId(i + 1),
                team: TeamId(i % 2),
            },
            name: "stateful".into(),
            script: Ok(src.clone()),
        })
        .collect()
}

#[test]
fn rematches_release_vm_callbacks_map_and_snapshots() {
    for _ in 0..3 {
        let map = Arc::new(SWITCHYARD.parse().unwrap());
        let old_map = Arc::downgrade(&map);
        let live = LiveMatch::start(map, ClockRules::unlimited(), seats(), ()).unwrap();
        let end = Instant::now() + Duration::from_secs(4);
        while live.latest().stats.iter().any(|s| s.completed == 0) {
            assert!(!live.finished() && Instant::now() < end);
            thread::sleep(Duration::from_millis(2));
        }

        // 每个虚拟机的中断闭包持有该局取消令牌；仅保留 Weak，不延长任何资源的生命期。
        let token = Arc::downgrade(&live.cancel);
        assert!(token.strong_count() > MAX_SLOTS);
        let snapshot = Arc::downgrade(&live.latest());
        let display = Arc::downgrade(&live.latest);
        live.stop();
        live.join().unwrap();

        assert!(token.upgrade().is_none() && old_map.upgrade().is_none());
        assert!(snapshot.upgrade().is_none() && display.upgrade().is_none());
    }
}

#[test]
fn host_unwind_wakes_sleeping_workers_before_scope_join() {
    struct BrokenLog;
    impl MatchLog for BrokenLog {
        fn start(&mut self, _: &World, _: &[Entrant], _: &[(SlotId, Fault)]) {
            panic!("injected host error after VM init");
        }
    }
    let live = LiveMatch::start(
        Arc::new(SWITCHYARD.parse().unwrap()),
        ClockRules::unlimited(),
        seats(),
        BrokenLog,
    )
    .unwrap();
    let token = Arc::downgrade(&live.cancel);
    let (done, rx) = mpsc::channel();
    thread::spawn(move || done.send(live.join().is_err()).unwrap());

    assert!(
        rx.recv_timeout(Duration::from_secs(4))
            .expect("match teardown deadlocked")
    );
    assert!(token.upgrade().is_none());
}
