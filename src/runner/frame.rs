use crate::{bot::Fault, sim::*};
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum Decision {
    NoResult,
    Received {
        seq: u64,
        observed_tick: u64,
        result: Result<ControlUpdate, Fault>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct Applied {
    pub slot: SlotId,
    pub decision: Decision,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SlotStats {
    pub p50_us_upper: u64,
    pub p95_us_upper: u64,
    pub enemy_hits: u64,
    pub friendly_hits: u64,
    pub self_hits: u64,
    pub completed: u64,
    pub missed: u64,
    pub last: Option<Decision>,
    pub last_us: u64,
    pub wake_us: u64,
    pub age: u64,
    pub fault: Option<Fault>,
}

impl SlotStats {
    /// 只以该席存活期间的结算机会作分母。初始化就弃权没有采样，不伪报 0% 缺席。
    pub fn missed_fraction(&self) -> Option<f64> {
        let opportunities = u128::from(self.completed) + u128::from(self.missed);
        (opportunities > 0).then(|| self.missed as f64 / opportunities as f64)
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub tick: u64,
    pub tanks: Vec<Tank>,
    pub shells: Vec<Shell>,
    pub items: Items,
    pub events: Vec<Event>,
    pub cue: Option<Event>,
    pub hit_ticks: [Option<u64>; MAX_SLOTS],
    pub outcome: Option<Outcome>,
    pub stats: Vec<SlotStats>,
    pub late_us: u64,
    pub(super) sight: Vec<Sight>,
}

impl Frame {
    pub(super) fn new(world: &World, stats: &[SlotStats], late_us: u64) -> Self {
        Self {
            tick: world.tick(),
            tanks: world.tanks().into(),
            shells: world.shells().into(),
            items: world.items().clone(),
            events: world.events().into(),
            cue: world.cue(),
            hit_ticks: *world.hit_ticks(),
            outcome: world.outcome(),
            sight: world.sights().collect(),
            stats: stats.into(),
            late_us,
        }
    }
}
