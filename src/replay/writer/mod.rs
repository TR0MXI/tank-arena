mod atomic_file;
mod status;
#[cfg(test)]
mod tests;

use super::*;
use crate::runner::{Entrant, MatchLog};
pub use status::{RecordStatus, Recording, SaveFailure};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

const QUEUE_CAPACITY: usize = 128;

struct Spool {
    skins: SeatSkins,
    rx: Receiver<(Record, ItemPool)>,
    status: Recording,
    path: PathBuf,
}

impl Spool {
    fn run(self) {
        let saved = atomic_file::publish(&self.path, |out| {
            let mut records = self.rx.iter();
            let (first, pool) = records
                .next()
                .ok_or_else(|| io::Error::from(io::ErrorKind::Interrupted))?;
            let mut enc = codec::Encoder::with_items(out, &self.skins, pool)?;
            for record in std::iter::once(first).chain(records.map(|(r, _)| r)) {
                self.status.0.queued.fetch_sub(1, Ordering::Relaxed);
                let finished = matches!(record, Record::End { .. });
                enc.push(record)?;
                if finished {
                    return enc.flush();
                }
            }

            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "recording interrupted",
            ))
        });
        self.status
            .0
            .replay
            .finish(saved.map(|()| self.path).map_err(Into::into));
    }
}

pub struct Recorder {
    tx: Option<SyncSender<(Record, ItemPool)>>,
    pool: ItemPool,
    worker: Option<JoinHandle<()>>,
    status: Recording,
    path: PathBuf,
    header: Option<Header>,
    hasher: StateHasher,
    metrics: serde_json::Value,
}

impl Recorder {
    pub fn new(dir: &Path) -> (Self, Recording) {
        Self::with_skins(dir, Default::default())
    }

    pub fn with_skins(dir: &Path, skins: SeatSkins) -> (Self, Recording) {
        static SERIAL: AtomicU64 = AtomicU64::new(0);

        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let path = dir.join(format!(
            "match-{stamp}-{}-{serial}.{EXTENSION}",
            std::process::id()
        ));
        let (mut recorder, mut spool) = Self::channel(path);
        spool.skins = skins;
        match thread::Builder::new()
            .name(format!("arena-record-{serial}"))
            .spawn(move || spool.run())
        {
            Ok(worker) => recorder.worker = Some(worker),
            Err(e) => {
                recorder.tx = None;
                recorder.status.0.replay.finish(Err(e.into()));
            }
        }
        let status = recorder.status.clone();
        (recorder, status)
    }

    fn channel(path: PathBuf) -> (Self, Spool) {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let status = Recording::default();
        let spool = Spool {
            skins: Default::default(),
            path: path.clone(),
            status: status.clone(),
            rx,
        };
        (
            Self {
                tx: Some(tx),
                pool: ItemPool::NONE,
                worker: None,
                status,
                path,
                header: None,
                hasher: StateHasher::default(),
                metrics: serde_json::Value::Null,
            },
            spool,
        )
    }

    fn send(&mut self, r: Record) {
        let Some(tx) = &self.tx else { return };

        let counts = &self.status.0;
        let n = counts.queued.fetch_add(1, Ordering::Relaxed) + 1;
        match tx.try_send((r, self.pool)) {
            Ok(()) => {
                counts
                    .peak
                    .fetch_max(n.min(QUEUE_CAPACITY), Ordering::Relaxed);
            }
            Err(e) => {
                counts.queued.fetch_sub(1, Ordering::Relaxed);
                counts.replay.finish(Err(match e {
                    TrySendError::Full(_) => SaveFailure::QueueFull,
                    TrySendError::Disconnected(_) => SaveFailure::Interrupted,
                }));
                self.tx = None;
            }
        }
    }

    fn close(&mut self) {
        self.tx = None;
        if let Some(h) = self.worker.take()
            && h.join().is_err()
        {
            self.status.0.replay.finish(Err(SaveFailure::Interrupted));
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.close();
        self.status.0.result.finish(Err(SaveFailure::Interrupted));
    }
}

impl MatchLog for Recorder {
    fn start(&mut self, w: &World, bots: &[Entrant], faults: &[(SlotId, Fault)]) {
        self.pool = w.items().pool;
        let header = Header {
            seed: w.behavior_seed(),
            map: w.map().source().into(),
            tick_hz: HZ,
            time_limit_secs: w.clock().seconds(),
            roster: bots
                .iter()
                .zip(w.tanks())
                .map(|(b, t)| BotInfo {
                    seat: b.seat,
                    name: b.name.clone(),
                    spawn: t.center,
                    script_blake3: b.fingerprint().map(|hash| hash.to_hex().to_string()),
                })
                .collect(),
            init_faults: faults.into(),
        };
        self.header = Some(header.clone());
        self.send(Record::Header(header));
        self.send(Record::Keyframe {
            state: w.checkpoint(),
        });
    }

    fn step(&mut self, tick: u64, w: &World, inputs: &[Applied]) {
        if self.tx.is_none() {
            return;
        }

        let state = w.checkpoint();
        let digest = self.hasher.digest(&state, w.items());
        self.send(Record::Step {
            applied_tick: tick,
            inputs: inputs.into(),
            events: w.events().into(),
            digest,
        });
        if w.tick() != tick && w.tick().is_multiple_of(STRIDE) {
            self.send(Record::Keyframe { state });
        }
    }

    fn summary(&mut self, stats: &[crate::runner::SlotStats], late: u64, cold: u64) {
        let slots: Vec<_> = stats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut row = serde_json::json!(s);
                row["slot_id"] = serde_json::json!(
                    self.header
                        .as_ref()
                        .and_then(|h| h.roster.get(i))
                        .map(|b| b.seat.slot)
                );
                row["missed_fraction"] = serde_json::json!(s.missed_fraction());
                row
            })
            .collect();
        self.metrics = serde_json::json!({
            "slots": slots,
            "cold_us": cold,
            "late_p95_us_upper": late,
        });
    }

    fn finish(&mut self, w: &World) {
        let outcome = w.outcome().expect("finished match");
        let hash = self.hasher.digest(&w.checkpoint(), w.items());
        let result = serde_json::json!({
            "tick": w.tick(),
            "outcome": outcome,
            "digest": blake3::Hash::from_bytes(hash).to_hex().as_str(),
        });
        self.send(Record::End {
            tick: w.tick(),
            outcome,
            digest: hash,
        });
        self.close();

        // 结束记录入队时也可能形成峰值，必须在关闭写线程后再取最终水位。
        self.metrics["queue_high_water"] = self.status.high_water().into();

        // 赛果不是回放的一部分：即使写盘队列中途失效，也独立尝试发布赛果。
        let path = self.path.with_extension("result.json");
        let result_json = serde_json::json!({
            "header": self.header,
            "result": result,
            "tanks": w.tanks(),
            "metrics": self.metrics,
        });
        let result =
            atomic_file::publish(&path, |out| Ok(serde_json::to_writer(out, &result_json)?));
        self.status
            .0
            .result
            .finish(result.map(|()| path).map_err(Into::into));
    }
}
