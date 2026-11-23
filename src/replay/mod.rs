//! 只保存裁判实际收到的输入。读取、校验、寻址和播放都不启动 Luau 虚拟机。

mod codec;
mod issue;
pub mod library;
mod reader;
mod writer;

use crate::{bot::Fault, runner::Applied, sim::*};
use bitcode::{Decode, Encode};
pub use issue::{ReplayIssue, ReplayProblem};
pub use reader::Playback;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    path::PathBuf,
};
pub use writer::{RecordStatus, Recorder, Recording, SaveFailure};

pub const EXTENSION: &str = "tankrec";

/// 按稳定席位编号减一索引，不按当前名单位置索引。
pub type SeatSkins = [Option<crate::appearance::PixelSkin>; MAX_SLOTS];

const STRIDE: u64 = 256;

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct BotInfo {
    pub seat: Seat,
    pub name: String,
    pub script_blake3: Option<String>,
    pub spawn: Point,
}

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
pub struct Header {
    /// 开局种子。地图记录完整布局；启用道具时由此重建确定性的掉落和散布。
    pub seed: Option<u64>,
    pub map: String,
    pub tick_hz: u32,
    pub time_limit_secs: Option<u64>,
    pub roster: Vec<BotInfo>,
    pub init_faults: Vec<(SlotId, Fault)>,
}

#[derive(Debug, Serialize, Deserialize, Encode, Decode)]
enum Record {
    Header(Header),
    Step {
        applied_tick: u64,
        inputs: Vec<Applied>,
        events: Vec<Event>,
        digest: [u8; 32],
    },
    Keyframe {
        state: Checkpoint,
    },
    End {
        tick: u64,
        outcome: Outcome,
        digest: [u8; 32],
    },
}

#[derive(Default)]
struct StateHasher(bitcode::Buffer);

impl StateHasher {
    fn digest(&mut self, state: &Checkpoint, items: &Items) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(self.0.encode(state));
        if items.pool != ItemPool::NONE {
            hash.update(self.0.encode(items));
        }
        *hash.finalize().as_bytes()
    }
}

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

#[cfg(test)]
mod codec_tests;
