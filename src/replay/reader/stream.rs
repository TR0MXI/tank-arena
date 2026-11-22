//! 顺序校验器供封面、完整加载和播放共用；只有首次完整加载时才写关键帧索引。

use super::*;
use crate::runner::Decision;
use std::{
    io::{Read, Seek, SeekFrom},
    sync::Arc,
};

#[derive(Clone, Copy, Default, bitcode::Encode, bitcode::Decode)]
struct LaneCursor {
    seq: u64,
    applied: Option<u64>,
}

impl LaneCursor {
    fn accept(&mut self, seq: u64, observed: u64, applied: u64) -> bool {
        // 慢脚本可以用旧快照，但须在上一次结果封存后才能领取下一份观察。
        if self.seq.checked_add(1) != Some(seq)
            || observed > applied
            || self.applied.is_some_and(|last| observed <= last)
        {
            return false;
        }

        *self = Self {
            seq,
            applied: Some(applied),
        };
        true
    }
}

pub(super) struct Stream {
    pub(super) world: World,
    pub(super) reader: codec::Decoder,
    pub(super) applied: Vec<Applied>,
    hasher: StateHasher,
    record_no: u64,
    pub(super) ended: bool,
    lanes: [LaneCursor; MAX_SLOTS],
}

impl Stream {
    pub(super) fn begin(path: &Path, index: Option<&mut SeekIndex>) -> io::Result<(Header, Self)> {
        let mut reader =
            codec::Decoder::new(File::open(path)?).map_err(|e| ReplayIssue::decoding(1, e))?;
        let Some(Record::Header(header)) =
            reader.next().map_err(|e| ReplayIssue::decoding(1, e))?
        else {
            return Err(ReplayIssue::at(1, Problem::Format, "missing header"));
        };
        if header.tick_hz != HZ {
            return Err(ReplayIssue::at(
                1,
                Problem::Configuration,
                "recording must use 60 Hz",
            ));
        }

        let clock = header
            .time_limit_secs
            .map_or(Ok(ClockRules::unlimited()), ClockRules::with_time_limit)
            .map_err(|e| ReplayIssue::at(1, Problem::Configuration, e))?;
        let map: ArenaMap = header
            .map
            .parse()
            .map_err(|e| ReplayIssue::at(1, Problem::Configuration, e))?;
        let mut world = World::new(
            Arc::new(map),
            header.roster.iter().map(|b| b.seat).collect(),
            clock,
        )
        .map_err(|e| ReplayIssue::at(1, Problem::Configuration, e))?
        .with_behavior_seed(header.seed);
        if !header
            .roster
            .iter()
            .zip(world.tanks())
            .all(|(b, t)| b.seat == t.seat && b.spawn == t.center)
            || header.init_faults.iter().enumerate().any(|(i, (id, _))| {
                !world.tanks().iter().any(|t| t.seat.slot == *id)
                    || header.init_faults[..i].iter().any(|(s, _)| s == id)
            })
        {
            return Err(ReplayIssue::at(
                1,
                Problem::Configuration,
                "inconsistent roster",
            ));
        }

        world.forfeit(
            &header
                .init_faults
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>(),
        );
        let mut p = Self {
            world,
            reader,
            applied: vec![],
            hasher: StateHasher::default(),
            record_no: 1,
            ended: false,
            lanes: [LaneCursor::default(); MAX_SLOTS],
        };
        p.keyframe(index)?;
        Ok((header, p))
    }

    pub(super) fn check_eof(&mut self) -> io::Result<()> {
        if self
            .reader
            .next()
            .map_err(|e| ReplayIssue::decoding(self.record_no + 1, e))?
            .is_some()
        {
            return Err(ReplayIssue::at(
                self.record_no + 1,
                Problem::Sequence,
                "trailing records",
            ));
        }

        Ok(())
    }

    fn err(&self, problem: Problem, detail: impl std::fmt::Display) -> io::Error {
        ReplayIssue::at(
            self.record_no,
            problem,
            format_args!("tick {}: {detail}", self.world.tick()),
        )
    }

    fn next(&mut self) -> io::Result<Record> {
        self.record_no += 1;
        self.reader
            .next()
            .map_err(|e| ReplayIssue::decoding(self.record_no, e))?
            .ok_or_else(|| self.err(Problem::Truncated, "truncated recording"))
    }

    fn keyframe(&mut self, index: Option<&mut SeekIndex>) -> io::Result<()> {
        let Record::Keyframe { state } = self.next()? else {
            return Err(self.err(Problem::Sequence, "missing keyframe"));
        };
        if state != self.world.checkpoint() {
            return Err(self.err(Problem::StateMismatch, "keyframe mismatch"));
        }

        if let Some(index) = index {
            let (start, offset) = self.reader.position();
            for n in [
                start,
                offset,
                index.states.stream_position()?,
                self.record_no,
            ] {
                index.table.write_all(&n.to_le_bytes())?;
            }

            let bytes =
                bitcode::encode(&(state, self.lanes, self.world.cue(), *self.world.hit_ticks()));
            index
                .states
                .write_all(&(bytes.len() as u32).to_le_bytes())?;
            index.states.write_all(&bytes)?;
        }
        Ok(())
    }

    pub(super) fn consume(&mut self, index: Option<&mut SeekIndex>) -> io::Result<()> {
        match self.next()? {
            Record::Step {
                applied_tick,
                inputs,
                events,
                digest: expected,
            } => {
                if applied_tick != self.world.tick() || self.world.outcome().is_some() {
                    return Err(self.err(Problem::Sequence, "unexpected step"));
                }

                let mut live = self
                    .world
                    .tanks()
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.alive);
                let mut controls = Vec::new();
                let mut bad = Vec::new();
                for a in &inputs {
                    let Some((i, t)) = live.next() else {
                        return Err(self.err(Problem::Sequence, "extra slot"));
                    };
                    if t.seat.slot != a.slot {
                        return Err(self.err(Problem::Sequence, "slot mismatch"));
                    }

                    if let Decision::Received {
                        seq,
                        observed_tick,
                        result,
                    } = &a.decision
                    {
                        if !self.lanes[i].accept(*seq, *observed_tick, applied_tick) {
                            return Err(
                                self.err(Problem::Sequence, "invalid sequence or observation")
                            );
                        }

                        match result {
                            Ok(c) => controls.push((a.slot, *c)),
                            Err(_) => bad.push(a.slot),
                        }
                    }
                }

                if live.next().is_some() {
                    return Err(self.err(Problem::Sequence, "missing slot"));
                }

                self.applied = inputs;
                self.world.step(&controls, &bad);
                if self.world.events() != events
                    || self.hasher.digest(&self.world.checkpoint()) != expected
                {
                    return Err(self.err(Problem::StateMismatch, "events or state digest mismatch"));
                }

                if self.world.tick() != applied_tick && self.world.tick().is_multiple_of(STRIDE) {
                    self.keyframe(index)?;
                }
            }
            Record::End {
                tick,
                outcome,
                digest: expected,
            } => {
                if matches!(
                    outcome,
                    Outcome::Stopped | Outcome::HostOverload | Outcome::WorkerLost
                ) {
                    self.world.invalidate(outcome);
                }
                if self.world.tick() != tick
                    || self.world.outcome() != Some(outcome)
                    || self.hasher.digest(&self.world.checkpoint()) != expected
                {
                    return Err(self.err(Problem::StateMismatch, "terminal state mismatch"));
                }

                self.ended = true;
            }
            _ => return Err(self.err(Problem::Sequence, "unexpected record")),
        }
        Ok(())
    }

    pub(super) fn restore(&mut self, index: &mut SeekIndex, target: u64) -> io::Result<()> {
        index.table.seek(SeekFrom::Start(target / STRIDE * 32))?;
        let mut buf = [0; 8];
        let mut cursor = [0; 4];
        for n in &mut cursor {
            index.table.read_exact(&mut buf)?;
            *n = u64::from_le_bytes(buf);
        }

        self.reader.restore(cursor[0], cursor[1])?;
        index.states.seek(SeekFrom::Start(cursor[2]))?;
        self.record_no = cursor[3];
        self.applied.clear();
        let mut len = [0; 4];
        index.states.read_exact(&mut len)?;
        let mut bytes = vec![0; u32::from_le_bytes(len) as usize];
        index.states.read_exact(&mut bytes)?;
        let (c, lanes, cue, hits) = bitcode::decode(&bytes).map_err(invalid)?;
        self.world.restore(c);
        self.world.restore_cues(cue, hits);
        self.lanes = lanes;
        self.ended = false;
        Ok(())
    }
}
