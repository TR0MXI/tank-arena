use super::*;
use crate::runner::SlotStats;
use std::{fs, sync::Arc};

fn arena() -> (World, Vec<Entrant>) {
    let bots = (0..2)
        .map(|i| Entrant {
            name: format!("bot {i}"),
            seat: Seat {
                slot: SlotId(i + 1),
                team: TeamId(i),
            },
            script: Ok(crate::bot::Script::from_source(
                "idle",
                "return function(_) return nil end",
            )
            .unwrap()),
        })
        .collect::<Vec<_>>();
    let clock = ClockRules::with_time_limit(10).unwrap();
    (
        World::new(
            Arc::new(SWITCHYARD.parse().unwrap()),
            bots.iter().map(|e| e.seat).collect(),
            clock,
        )
        .unwrap(),
        bots,
    )
}

fn assert_clean(dir: &Path) {
    assert!(
        fs::read_dir(dir).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part"))
    );
}

#[test]
fn full_queue_does_not_block_world_or_lose_independent_result() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("match.tankrec");
    let (mut rec, spool) = Recorder::channel(path.clone());
    let (mut world, bots) = arena();
    rec.start(&world, &bots, &[]);

    // 在不调度消费者的情况下耗尽真正的有界队列，不依靠休眠触发竞争。
    while world.outcome().is_none() {
        let tick = world.tick();
        world.step(&[], &[]);
        rec.step(tick, &world, &[]);
        if tick == QUEUE_CAPACITY as u64 {
            assert!(matches!(
                rec.status.status(),
                RecordStatus::Incomplete(SaveFailure::QueueFull)
            ));
        }
    }

    assert_eq!(world.outcome(), Some(Outcome::Timeout));
    assert_eq!(rec.status.high_water(), QUEUE_CAPACITY);
    spool.run();

    rec.summary(
        &[
            SlotStats {
                missed: 75,
                completed: 225,
                ..SlotStats::default()
            },
            SlotStats::default(),
        ],
        1,
        2,
    );
    rec.finish(&world);

    assert!(matches!(
        rec.status.status(),
        RecordStatus::Incomplete(SaveFailure::QueueFull)
    ));
    assert!(!path.exists());
    let RecordStatus::Saved(summary) = rec.status.result_status() else {
        panic!("missing independent result")
    };

    let data: serde_json::Value =
        serde_json::from_reader(fs::File::open(summary).unwrap()).unwrap();
    assert_eq!(data["header"]["roster"][1]["name"], "bot 1");
    assert_eq!(data["header"]["tick_hz"], HZ);
    assert_eq!(data["metrics"]["queue_high_water"], QUEUE_CAPACITY);
    assert_eq!(data["metrics"]["slots"][0]["missed_fraction"], 0.25);
    assert_eq!(
        data["metrics"]["slots"][1]["missed_fraction"],
        serde_json::Value::Null
    );
    assert_eq!(data["result"]["tick"], 600);
    assert_eq!(data["result"]["outcome"], "Timeout");
    assert_clean(tmp.path());
}

#[test]
fn result_publish_failure_does_not_hide_a_valid_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut rec, status) = Recorder::new(tmp.path());
    let occupied = rec.path.with_extension("result.json");
    fs::write(&occupied, b"do not overwrite").unwrap();
    let (mut world, bots) = arena();
    rec.start(&world, &bots, &[]);
    world.stop();
    rec.finish(&world);

    assert!(
        matches!(status.result_status(), RecordStatus::Incomplete(SaveFailure::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(occupied).unwrap(), b"do not overwrite");
    let RecordStatus::Saved(path) = status.status() else {
        panic!("completed replay hidden")
    };

    let mut playback = Playback::open(&path).unwrap();
    playback.seek(playback.length()).unwrap();

    assert_eq!(playback.world().outcome(), Some(Outcome::Stopped));
    assert_clean(tmp.path());
}

#[test]
fn both_outputs_report_a_broken_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("not-a-directory");
    fs::write(&root, b"x").unwrap();
    let (mut rec, status) = Recorder::new(&root);
    let (mut world, bots) = arena();
    rec.start(&world, &bots, &[]);
    world.stop();
    rec.finish(&world);
    for output in [status.status(), status.result_status()] {
        assert!(
            matches!(output, RecordStatus::Incomplete(SaveFailure::Io(_))),
            "{output:?}"
        );
    }

    assert_eq!(fs::read(root).unwrap(), b"x");
}

#[test]
fn partial_disk_write_leaves_no_visible_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("duel.tankrec");
    let result = atomic_file::publish(&path, |file| {
        file.write_all(b"first chunk")?;
        file.flush()?;
        Err(io::ErrorKind::StorageFull.into())
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::StorageFull);
    assert!(!path.exists());
    assert_clean(tmp.path());
}

#[test]
fn abandoning_a_recorder_joins_its_writer_and_removes_partial_output() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut rec, status) = Recorder::new(tmp.path());
    let (world, bots) = arena();
    rec.start(&world, &bots, &[]);
    drop(rec);

    assert!(matches!(status.status(), RecordStatus::Incomplete(_)));
    assert!(matches!(
        status.result_status(),
        RecordStatus::Incomplete(SaveFailure::Interrupted)
    ));
    assert_eq!(tmp.path().read_dir().unwrap().count(), 0);
}

#[test]
fn seed_survives_recording_and_is_independent_of_layout() {
    let (w, bots) = arena();
    for seed in [None, Some(0), Some(u64::MAX)] {
        let dir = tempfile::tempdir().unwrap();
        let mut w = w.clone().with_behavior_seed(seed);
        let (mut rec, status) = Recorder::new(dir.path());
        rec.start(&w, &bots, &[]);
        w.stop();
        rec.finish(&w);
        let RecordStatus::Saved(path) = status.status() else {
            panic!("recording failed")
        };

        let p = Playback::open(&path).unwrap();
        assert_eq!(p.header.seed, seed);
        assert_eq!(p.world().behavior_seed(), seed);
        assert_eq!(p.header.map, w.map().source());
    }
}

#[test]
fn header_uses_the_frozen_package_digest_not_the_current_files_or_just_the_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut world, mut bots) = arena();
    let entry = tmp.path().join("bot.luau");
    let module = tmp.path().join("decision.luau");
    fs::write(&entry, "return require('./decision')").unwrap();
    fs::write(&module, "return function() end").unwrap();
    bots[0] = Entrant::from_file(bots[0].seat, &entry);
    let frozen = bots[0].script.as_ref().unwrap().blake3();
    fs::write(module, "return function() return {fire=true} end").unwrap();
    let (mut recorder, status) = Recorder::new(tmp.path());
    recorder.start(&world, &bots, &[]);
    world.stop();
    recorder.finish(&world);
    let RecordStatus::Saved(path) = status.status() else {
        panic!("missing replay")
    };
    let header = Playback::open(&path).unwrap().header;
    assert_eq!(
        header.roster[0].script_blake3.as_deref(),
        Some(frozen.to_hex().as_str())
    );
    assert_eq!(
        header.roster[1].script_blake3.as_deref(),
        Some(
            blake3::hash(bots[1].script.as_ref().unwrap().text().as_bytes())
                .to_hex()
                .as_str()
        )
    );
}
