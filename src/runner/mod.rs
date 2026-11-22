//! 比赛线程独占裁判，窗口只读取最新完成的快照。每场创建独立的信箱和虚拟机。

mod bot_worker;
mod frame;
#[cfg(test)]
mod lifecycle_tests;
mod mailbox;
mod metrics;
mod pacer;

use crate::{
    bot::{BotConfig, Fault, FaultKind, Script},
    sim::*,
};
use bot_worker::Exchange;
pub use frame::*;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Entrant {
    pub seat: Seat,
    pub name: String,
    pub script: Result<Script, Fault>,
}

impl Entrant {
    pub fn from_script(seat: Seat, script: Script) -> Self {
        Self {
            seat,
            name: script.name().to_owned(),
            script: Ok(script),
        }
    }

    pub fn from_file(seat: Seat, path: &Path) -> Self {
        let script = Script::read(path).map_err(|e| Fault {
            kind: FaultKind::Load,
            message: format!("{}: {e}", path.display()),
        });
        let name = script.as_ref().map_or_else(
            |_| crate::catalog::tank_name(path),
            |script| script.name().to_owned(),
        );
        Self { seat, name, script }
    }
}

/// 回调位于裁判线程；实现必须有界且不等待文件 I/O。
pub trait MatchLog: Send + 'static {
    fn start(&mut self, _world: &World, _bots: &[Entrant], _faults: &[(SlotId, Fault)]) {}
    fn step(&mut self, _applied_tick: u64, _world: &World, _inputs: &[Applied]) {}
    fn summary(&mut self, _stats: &[SlotStats], _late_p95_us_upper: u64, _cold_us: u64) {}
    fn finish(&mut self, _world: &World) {}
}

impl MatchLog for () {}

type Display = Arc<Mutex<Arc<Frame>>>;

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Configuration(#[from] ConfigError),
    #[error("cannot start the match thread: {0}")]
    Thread(#[from] std::io::Error),
}

pub struct LiveMatch {
    latest: Display,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LiveMatch {
    pub fn start(
        map: Arc<ArenaMap>,
        clock: ClockRules,
        mut bots: Vec<Entrant>,
        log: impl MatchLog,
    ) -> Result<Self, StartError> {
        bots.sort_unstable_by_key(|b| b.seat.slot);
        let world = World::new(map.clone(), bots.iter().map(|e| e.seat).collect(), clock)?
            .with_behavior_seed(Some(fastrand::u64(..)));
        let stats = vec![SlotStats::default(); bots.len()];
        let latest = Arc::new(Mutex::new(Arc::new(Frame::new(&world, &stats, 0))));
        let cancel = Arc::new(AtomicBool::default());
        let display = latest.clone();
        let token = cancel.clone();
        let thread = thread::Builder::new()
            .name("arena-match".into())
            .spawn(move || run(world, &map, &bots, stats, &display, token, log))?;
        Ok(Self {
            latest,
            cancel,
            thread: Some(thread),
        })
    }

    pub fn latest(&self) -> Arc<Frame> {
        self.latest.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.thread.take().unwrap().join()
    }
}

impl Drop for LiveMatch {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn at_tick(tick: u64) -> Duration {
    let hz = u64::from(HZ);
    Duration::new(tick / hz, ((tick % hz) * 1_000_000_000 / hz) as u32)
}

fn run(
    mut world: World,
    map: &ArenaMap,
    bots: &[Entrant],
    mut stats: Vec<SlotStats>,
    display: &Mutex<Arc<Frame>>,
    cancel: Arc<AtomicBool>,
    mut log: impl MatchLog,
) {
    static IDS: AtomicU64 = AtomicU64::new(1);

    let id = IDS.fetch_add(1, Ordering::Relaxed);
    let exchange = Exchange::new(id, cancel);
    let clock = world.clock();
    let lost = AtomicBool::new(false);
    let cold = Instant::now();
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let lost = &lost;
        let ex = &exchange;
        let shutdown = bot_worker::CloseOnExit(ex);

        // 初始化与局内错误分离：只有所有席位均返回初始化结果才发布 S[0]。
        let mut handles = Vec::new();
        for (i, bot) in bots.iter().enumerate() {
            let config = BotConfig {
                map,
                clock,
                seat: bot.seat,
                behavior_seed: world.pilot_seed(bot.seat.slot),
            };
            let tx = tx.clone();
            let task = thread::Builder::new()
                .name(format!("arena-bot-{id}-{}", bot.seat.slot.0))
                .spawn_scoped(scope, move || {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        bot_worker::work(i, bot, config, ex, tx);
                    }))
                    .is_err()
                    {
                        lost.store(true, Ordering::Release);
                    }
                });
            match task {
                Ok(h) => handles.push(h),
                Err(_) => {
                    lost.store(true, Ordering::Release);
                    break;
                }
            }
        }

        drop(tx);
        let mut faults = Vec::new();
        for (i, result) in rx {
            if let Err(e) = result {
                stats[i].fault = Some(e.clone());
                faults.push((world.tanks()[i].seat.slot, e));
            }
        }

        faults.sort_by_key(|r| r.0);
        world.forfeit(&faults.iter().map(|r| r.0).collect::<Vec<_>>());
        let cold_us = cold.elapsed().as_micros() as u64;
        let mut costs: [metrics::Histogram; MAX_SLOTS] = Default::default();
        let mut lateness = metrics::Histogram::default();

        log.start(&world, bots, &faults);
        pacer::configure_clock_thread();

        let mut late_us = 0;
        let mut epoch = None;
        let mut inputs = Vec::with_capacity(MAX_SLOTS);
        let mut frames = Vec::with_capacity(MAX_SLOTS);
        let mut bad = Vec::with_capacity(MAX_SLOTS);

        loop {
            if lost.load(Ordering::Acquire) {
                world.invalidate(Outcome::WorkerLost);
            }
            if ex.cancel.load(Ordering::Acquire) {
                world.stop();
            }

            let snapshot = Arc::new(Frame::new(&world, &stats, late_us));
            *display.lock().unwrap() = snapshot.clone();
            if world.outcome().is_some() {
                break;
            }

            let mut s = ex.state.lock().unwrap();
            let published = Instant::now();
            let start = *epoch.get_or_insert(published);
            s.latest = Some((snapshot, published));
            drop(s);
            ex.changed.notify_all();

            let tick = world.tick();
            let deadline = start + at_tick(tick + 1);
            if published >= deadline {
                world.invalidate(Outcome::HostOverload);
                continue;
            }

            pacer::wait_until(deadline);
            let mut s = ex.state.lock().unwrap();
            let now = Instant::now();
            s.latest = None;
            let sealed = s.mailbox.seal(deadline);
            drop(s);

            late_us = now.saturating_duration_since(deadline).as_micros() as u64;
            lateness.add(late_us);
            if now >= start + at_tick(tick + 2) {
                world.invalidate(Outcome::HostOverload);
                continue;
            }
            if ex.cancel.load(Ordering::Acquire) || lost.load(Ordering::Acquire) {
                continue;
            }

            inputs.clear();
            frames.clear();
            bad.clear();
            for (i, answer) in sealed.into_iter().take(bots.len()).enumerate() {
                let tank = world.tanks()[i];
                if !tank.alive {
                    continue;
                }

                let slot = tank.seat.slot;
                let data = &mut stats[i];
                let decision = if let Some((ticket, a)) = answer {
                    data.completed += 1;
                    if let Some(us) = a.cost_us {
                        costs[i].add(us);
                        data.p50_us_upper = costs[i].upper(50);
                        data.p95_us_upper = costs[i].upper(95);
                        data.last_us = us;
                        data.wake_us = a.wake_us;
                    }

                    data.age = tick - ticket.observed;
                    match &a.value {
                        Ok(c) => frames.push((slot, *c)),
                        Err(e) => {
                            bad.push(slot);
                            data.fault = Some(e.clone());
                        }
                    }

                    Decision::Received {
                        seq: ticket.seq,
                        observed_tick: ticket.observed,
                        result: a.value,
                    }
                } else {
                    data.missed += 1;
                    Decision::NoResult
                };
                data.last = Some(decision.clone());
                inputs.push(Applied { slot, decision });
            }

            world.step(&frames, &bad);
            for e in world.events() {
                if let Event::Hit { owner, kind, .. } = e
                    && let Some(i) = bots.iter().position(|b| b.seat.slot == *owner)
                {
                    let st = &mut stats[i];
                    match kind {
                        HitKind::Enemy => st.enemy_hits += 1,
                        HitKind::Friendly => st.friendly_hits += 1,
                        HitKind::SelfHit => st.self_hits += 1,
                    }
                }
            }

            log.step(tick, &world, &inputs);
        }

        drop(shutdown);
        for h in handles {
            h.join().expect("worker panics caught internally");
        }

        log.summary(&stats, lateness.upper(95), cold_us);
        log.finish(&world);
    });
}
