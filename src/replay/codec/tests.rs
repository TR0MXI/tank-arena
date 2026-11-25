use super::*;
use crate::{replay::Header, sim::Event};
use std::{fs::File, io::Write};

fn step(tick: u64) -> Record {
    Record::Step {
        applied_tick: tick,
        inputs: vec![],
        events: vec![],
        digest: [0; 32],
    }
}

fn header(map: String) -> Record {
    Record::Header(Header {
        seed: None,
        item_pool: crate::sim::ItemPool::NONE,
        map,
        tick_hz: 60,
        time_limit_secs: None,
        roster: vec![],
        init_faults: vec![],
    })
}

fn decoder(raw: &[u8]) -> Decoder {
    let zip = zstd::bulk::compress(raw, 3).unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for bytes in [
        MAGIC.as_slice(),
        &[0],
        &(raw.len() as u32).to_le_bytes(),
        &(zip.len() as u32).to_le_bytes(),
        &zip,
    ] {
        file.write_all(bytes).unwrap();
    }

    Decoder::new(File::open(file.path()).unwrap()).unwrap()
}

fn frame(batch: &Batch) -> Vec<u8> {
    let bytes = bitcode::encode(batch);
    [(bytes.len() as u32).to_le_bytes().as_slice(), &bytes].concat()
}

#[test]
fn reject_invalid_batch_framing_and_layout() {
    let mut b = Batch::default();
    assert!(decoder(&frame(&b)).next().is_err());
    b.0[1] = Some(step(1));
    assert!(decoder(&frame(&b)).next().is_err());
    b.0[0] = Some(header(String::new()));

    assert!(decoder(&frame(&b)).next().is_err());
    b.0[0] = Some(step(0));
    let valid = frame(&b);
    assert!(decoder(&valid).next().unwrap().is_some());
    let n = (valid.len() - LENGTH) as u32;
    for length in [0, n - 1, n + 1, u32::MAX] {
        let mut raw = valid.clone();
        raw[..LENGTH].copy_from_slice(&length.to_le_bytes());
        assert!(decoder(&raw).next().is_err());
    }

    let mut trailing = valid;
    trailing.push(0);
    trailing[..LENGTH].copy_from_slice(&(n + 1).to_le_bytes());

    assert!(decoder(&trailing).next().is_err());
    assert!(decoder(&[0, 0, 0]).next().is_err());
}

#[test]
fn full_batches_tail_and_byte_budget_are_independent() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut enc = Encoder::new(tmp.as_file(), &Default::default()).unwrap();
    for tick in 0..35 {
        enc.push(step(tick)).unwrap();
    }

    let mut big = step(35);
    let Record::Step { events, .. } = &mut big else {
        unreachable!()
    };

    events.resize(
        TARGET / size_of::<Event>() + 1,
        Event::Forfeit(crate::sim::SlotId(1)),
    );
    enc.push(big).unwrap();
    enc.push(step(36)).unwrap();
    enc.flush().unwrap();
    let mut dec = Decoder::new(tmp.reopen().unwrap()).unwrap();

    // 检查实际文件里的批边界：一整批、三条尾批、独立的大记录以及最后一条。
    let raw = std::fs::read(tmp.path()).unwrap();
    let mut blocks = &raw[MAGIC.len() + 1..];
    let mut counts = vec![];
    while !blocks.is_empty() {
        let size = u32::from_le_bytes(blocks[..4].try_into().unwrap()) as usize;
        let zipped = u32::from_le_bytes(blocks[4..8].try_into().unwrap()) as usize;
        let plain = zstd::bulk::decompress(&blocks[8..8 + zipped], size).unwrap();
        blocks = &blocks[8 + zipped..];
        let mut rest = plain.as_slice();
        while !rest.is_empty() {
            let n = u32::from_le_bytes(rest[..LENGTH].try_into().unwrap()) as usize;
            let (value, tail) = rest[LENGTH..].split_at(n);
            counts.push(
                bitcode::decode::<Batch>(value)
                    .unwrap()
                    .0
                    .into_iter()
                    .flatten()
                    .count(),
            );
            rest = tail;
        }
    }

    assert_eq!(counts, [32, 3, 1, 1]);
    for tick in 0..37 {
        assert!(
            matches!(dec.next().unwrap(), Some(Record::Step { applied_tick, .. }) if applied_tick == tick)
        );
    }

    assert!(dec.next().unwrap().is_none());
    assert!(enc.push(header("x".repeat(MAX_BLOCK))).is_err());
}
