use super::*;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// 仅从裁判复制，恢复时只使用本次逐步校验后生成的本地索引，不信任导入文件的状态。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub(crate) struct Checkpoint {
    tick: u64,
    tanks: Vec<Tank>,
    shells: Vec<Shell>,
    events: Vec<Event>,
    next_shell: u64,
    outcome: Option<Outcome>,
}

impl World {
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            tick: self.tick,
            tanks: self.tanks.clone(),
            shells: self.shells().to_vec(),
            events: self.events.clone(),
            next_shell: self.next_shell,
            outcome: self.outcome,
        }
    }

    pub(crate) fn restore(&mut self, c: Checkpoint) {
        self.tick = c.tick;
        self.tanks = c.tanks;

        // 索引属于本局固定名单；只从已验证的关键帧重建弹槽，不把派生数据写入回放。
        self.shells = Shells::from_rounds(c.shells);
        self.events = c.events;
        self.next_shell = c.next_shell;
        self.outcome = c.outcome;
    }
}
