use super::ArenaMap;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use thiserror::Error;

pub const MAX_SLOTS: usize = super::ROSTER_CAPACITY;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Encode, Decode,
)]
pub struct SlotId(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Encode, Decode,
)]
pub struct TeamId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Seat {
    pub slot: SlotId,
    pub team: TeamId,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("choose 2 to 8 slots")]
    SlotCount,
    #[error("tank ID must be between 1 and 8")]
    SlotRange,
    #[error("duplicate slot ID")]
    Duplicate,
    #[error("assign at least two teams")]
    Teams,
    #[error("not enough spawn points for the roster")]
    Spawns,
    #[error("enter a whole number of seconds (at least 10)")]
    Seconds,
    #[error("time limit overflows the match tick counter")]
    Overflow,
}

#[derive(Clone, Copy, Debug)]
pub struct ClockRules(Option<NonZeroU64>);

impl ClockRules {
    pub const fn unlimited() -> Self {
        Self(None)
    }

    pub fn with_time_limit(seconds: u64) -> Result<Self, ConfigError> {
        if seconds < 10 {
            return Err(ConfigError::Seconds);
        }

        let ticks = seconds
            .checked_mul(u64::from(super::HZ))
            .ok_or(ConfigError::Overflow)?;
        Ok(Self(NonZeroU64::new(ticks)))
    }

    pub fn seconds(self) -> Option<u64> {
        self.ticks().map(|ticks| ticks / u64::from(super::HZ))
    }

    pub fn ticks(self) -> Option<u64> {
        self.0.map(NonZeroU64::get)
    }
}

impl std::str::FromStr for ClockRules {
    type Err = ConfigError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ConfigError::Seconds);
        }
        Self::with_time_limit(text.parse().map_err(|_| ConfigError::Seconds)?)
    }
}

impl Default for ClockRules {
    fn default() -> Self {
        Self::with_time_limit(60).expect("valid default time limit")
    }
}

/// 排序一次，之后始终按席位 ID 寻址。改队不影响身份或出生顺序。
#[derive(Clone, Debug)]
pub struct Roster(Vec<Seat>);

impl Roster {
    pub fn new(mut seats: Vec<Seat>, map: &ArenaMap) -> Result<Self, ConfigError> {
        if !(2..=MAX_SLOTS).contains(&seats.len()) {
            return Err(ConfigError::SlotCount);
        }
        if map.spawns().len() < seats.len() {
            return Err(ConfigError::Spawns);
        }
        if seats
            .iter()
            .any(|s| !(1..=MAX_SLOTS as u32).contains(&s.slot.0))
        {
            return Err(ConfigError::SlotRange);
        }

        seats.sort_unstable_by_key(|s| s.slot);
        if seats.windows(2).any(|p| p[0].slot == p[1].slot) {
            return Err(ConfigError::Duplicate);
        }
        if seats.iter().all(|s| s.team == seats[0].team) {
            return Err(ConfigError::Teams);
        }

        Ok(Self(seats))
    }

    pub fn seats(&self) -> &[Seat] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    #[default]
    Stop,
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
/// 一次控制采样只更新给出的字段；省略的按键（包括扳机）保持原状态。
pub struct ControlUpdate {
    pub drive: Option<Drive>,
    pub turn: Option<Turn>,
    pub fire: Option<bool>,
}
