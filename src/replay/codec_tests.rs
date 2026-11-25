use super::*;
use crate::runner::Decision;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::Instant,
};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec")
}

fn unpack(path: &Path) -> Vec<Record> {
    let mut decoder = codec::Decoder::new(File::open(path).unwrap()).unwrap();
    let mut records = vec![];
    while let Some(r) = decoder.next().unwrap() {
        records.push(r);
    }
    records
}

fn pack(records: impl IntoIterator<Item = Record>) -> Vec<u8> {
    let mut bytes = vec![];
    let mut writer = codec::Encoder::new(&mut bytes, &Default::default()).unwrap();
    for r in records {
        writer.push(r).unwrap();
    }

    writer.flush().unwrap();
    bytes
}

fn problem(bytes: &[u8]) -> (ReplayProblem, u64) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("坏 记录.tankrec");
    fs::write(&path, bytes).unwrap();
    let err = Playback::open(&path)
        .err()
        .expect("invalid recording accepted");
    let e = err
        .get_ref()
        .unwrap()
        .downcast_ref::<ReplayIssue>()
        .unwrap();
    (e.problem, e.record_no)
}

#[test]
fn reject_old_text_unknown_binary_truncation_oversize_and_trailing_data() {
    assert_eq!(problem(br#"{"type":"header"}"#).0, ReplayProblem::Format);

    let mut bytes = fs::read(demo()).unwrap();
    assert_eq!(&bytes[..8], b"TANKREC\x01");
    assert_eq!(
        problem(&bytes[..bytes.len() - 1]).0,
        ReplayProblem::Truncated
    );

    let original_version = bytes[7];
    for version in [0, 2, 3, 4, 5, 6, 7, 8, 255] {
        bytes[7] = version;
        assert_eq!(problem(&bytes), (ReplayProblem::Version, 1));
    }

    bytes[7] = original_version;
    bytes[9..13].fill(255);

    assert_eq!(problem(&bytes).0, ReplayProblem::Format);

    let mut records = unpack(&demo());
    assert_eq!(
        problem(&pack(unpack(&demo()).into_iter().take(2))),
        (ReplayProblem::Truncated, 3)
    );
    records.push(Record::Keyframe {
        state: Playback::open(&demo()).unwrap().world().checkpoint(),
    });
    assert_eq!(problem(&pack(records)).0, ReplayProblem::Sequence);
}

#[test]
fn recheck_actions_across_keyframes_and_terminal_digest() {
    let mut records = unpack(&demo());
    if let Some(Record::End { digest, .. }) = records.last_mut() {
        digest[0] ^= 1;
    } else {
        panic!("missing end");
    }
    assert_eq!(problem(&pack(records)).0, ReplayProblem::StateMismatch);
    let mut records = unpack(&demo());
    let (i, row) = records
        .iter_mut()
        .enumerate()
        .find(|(_, r)| {
            matches!(
                r,
                Record::Step {
                    applied_tick: 256,
                    ..
                }
            )
        })
        .unwrap();
    let Record::Step { inputs, .. } = row else {
        unreachable!()
    };
    let Decision::Received { observed_tick, .. } = &mut inputs[0].decision else {
        panic!("expected recorded control")
    };

    *observed_tick = 0;

    assert_eq!(
        problem(&pack(records)),
        (ReplayProblem::Sequence, i as u64 + 1)
    );
}

#[test]
#[ignore = "本机 release 测量，不作为跨机器的耗时断言"]
fn compare_replay_codecs() {
    let path = demo();
    let records = unpack(&path);
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("same-input.jsonl");

    // 对照组直接把相同结构按行写成 JSON；不提供旧录像导入器，也不拿两场不同的比赛作比。
    let mut json = vec![];
    for r in &records {
        serde_json::to_writer(&mut json, r).unwrap();
        json.push(b'\n');
    }

    fs::write(&text, &json).unwrap();
    let median = |mut f: Box<dyn FnMut()>| {
        let mut samples = vec![];
        for _ in 0..31 {
            let now = Instant::now();
            f();
            samples.push(now.elapsed().as_micros());
        }

        samples.sort_unstable();
        samples[15]
    };
    let text_us = median(Box::new(|| {
        for line in BufReader::new(File::open(&text).unwrap()).lines() {
            std::hint::black_box(serde_json::from_str::<Record>(&line.unwrap()).unwrap());
        }
    }));
    let packed_us = median(Box::new(|| {
        std::hint::black_box(unpack(&path));
    }));
    let open_us = median(Box::new(|| {
        std::hint::black_box(Playback::open(&path).unwrap());
    }));
    let mut play = Playback::open(&path).unwrap();
    let seek_us = median(Box::new(|| {
        play.seek(270).unwrap();
    }));
    println!(
        "records={} json_bytes={} packed_bytes={} json_decode_us={} packed_decode_us={} verified_open_us={} seek_270_us={}",
        records.len(),
        json.len(),
        fs::metadata(path).unwrap().len(),
        text_us,
        packed_us,
        open_us,
        seek_us
    );
}

#[test]
fn seek_across_compressed_blocks_and_an_exact_end_of_block() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cross-block.tankrec");
    let records = unpack(&demo());
    let mut enc = codec::Encoder::new(File::create(&path).unwrap(), &Default::default()).unwrap();
    for (i, r) in records.into_iter().enumerate() {
        let keyframe = matches!(r, Record::Keyframe { .. });
        enc.push(r).unwrap();
        if i % 23 == 0 || keyframe {
            enc.flush().unwrap();
        }
    }

    enc.flush().unwrap();
    drop(enc);
    let mut expected = Playback::open(&demo()).unwrap();
    let mut actual = Playback::open(&path).unwrap();

    // 顺播时对齐每一步和终局；再跳到批内、批边、关键帧两侧，排除遗留的预读动作。
    while !expected.ended() {
        expected.advance().unwrap();
        actual.advance().unwrap();
        assert_eq!(actual.world().checkpoint(), expected.world().checkpoint());
        assert_eq!(actual.applied_inputs(), expected.applied_inputs());
    }

    for t in [258, 7, 256, 22, 257, 33, 31, 32, 255, 0, actual.length()] {
        expected.seek(t).unwrap();
        actual.seek(t).unwrap();
        assert_eq!(actual.world().checkpoint(), expected.world().checkpoint());
    }
}

#[test]
fn recording_does_not_accept_alternate_clocks() {
    for hz in [30, 120] {
        let mut records = unpack(&demo());
        let Record::Header(header) = &mut records[0] else {
            panic!("missing header")
        };
        header.tick_hz = hz;
        assert_eq!(problem(&pack(records)), (ReplayProblem::Configuration, 1));
    }
}

#[test]
fn cosmetic_prefix_survives_playback_and_seek_without_changing_records() {
    let demo = demo();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut skin = crate::appearance::PixelSkin::template();
    skin.set(47, 31, b'a'); // 右下角的细节无法用 2×2 块表示。
    let mut skins: SeatSkins = Default::default();
    skins[1] = Some(skin);
    let mut enc = codec::Encoder::new(tmp.as_file(), &skins).unwrap();
    for record in unpack(&demo) {
        enc.push(record).unwrap();
    }

    enc.flush().unwrap();
    let mut player = Playback::open(tmp.path()).unwrap();
    player.seek(1).unwrap();

    assert_eq!(player.skins(), &skins);
    let mut bytes = fs::read(tmp.path()).unwrap();
    assert_eq!(&bytes[..8], b"TANKREC\x01");
    assert_eq!(bytes[9 + 48 * 32 - 1], b'a');
    bytes[9] = b'?';
    assert_eq!(problem(&bytes), (ReplayProblem::Format, 1));
}

#[test]
fn author_livery_round_trips_without_the_source_script() {
    use crate::appearance::PixelSkin;

    let mut design = PixelSkin::template();
    design.set(0, 0, b'a');
    let mut skins: SeatSkins = Default::default();
    let source = format!("--[=[tank-skin\n{}\n]=]", String::from(design.clone()));
    skins[0] = Some(PixelSkin::from_script(&source).unwrap());
    assert_eq!(skins[0].as_ref(), Some(&design));

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut enc = codec::Encoder::new(tmp.as_file(), &skins).unwrap();
    for record in unpack(&demo()) {
        enc.push(record).unwrap();
    }

    enc.flush().unwrap();
    let mut player = Playback::open(tmp.path()).unwrap();
    player.seek(1).unwrap();

    assert_eq!(player.skins(), &skins);

    assert_eq!(&fs::read(tmp.path()).unwrap()[..9], b"TANKREC\x01\x01");
}

#[test]
fn replay_keeps_behavior_seed_across_seek_without_changing_physics() {
    let mut records = unpack(&demo());
    let Record::Header(h) = &mut records[0] else {
        panic!("missing header")
    };
    assert!(h.seed.is_none());
    h.seed = Some(u64::MAX);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seed.tankrec");
    fs::write(&path, pack(records)).unwrap();
    let mut p = Playback::open(&path).unwrap();
    assert_eq!(p.header.seed, Some(u64::MAX));
    for tick in [p.length(), 0, p.length() / 2] {
        p.seek(tick).unwrap();
        assert_eq!(p.world().behavior_seed(), Some(u64::MAX));
    }
}

#[test]
fn unknown_crate_types_are_rejected_before_simulation() {
    let mut records = unpack(&demo());
    let Record::Header(h) = &mut records[0] else {
        panic!("missing header")
    };
    h.item_pool = bitcode::decode(&bitcode::encode(&0x8000u16)).unwrap();
    assert_eq!(problem(&pack(records)), (ReplayProblem::Configuration, 1));
}
