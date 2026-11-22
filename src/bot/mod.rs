mod control;
mod source;
mod view;

use crate::sim::{ArenaMap, ClockRules, ControlUpdate, Seat, Shell, Sight, SlotId, Tank, World};
use bitcode::{Decode, Encode};
use mlua::{Error, Function, Lua, Result, Value, VmState, chunk::ChunkMode};
use serde::{Deserialize, Serialize};
pub use source::{Script, SourceError};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const VM_BYTES: usize = 4 * 1024 * 1024;
const CHECKPOINTS_PER_CALL: u32 = 10_000;

#[derive(Debug, thiserror::Error)]
#[error("VM checkpoint budget exhausted")]
struct Exhausted;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub enum FaultKind {
    Load,
    Runtime,
    Action,
    Budget,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Fault {
    pub kind: FaultKind,
    pub message: String,
}

impl Fault {
    pub fn classify(e: Error, loading: bool) -> Self {
        let kind = if e.downcast_ref::<Exhausted>().is_some() {
            FaultKind::Budget
        } else if e
            .chain()
            .any(|e| matches!(e.downcast_ref::<Error>(), Some(Error::MemoryError(_))))
        {
            FaultKind::Memory
        } else if e.downcast_ref::<control::BadAction>().is_some() {
            FaultKind::Action
        } else if loading {
            FaultKind::Load
        } else {
            FaultKind::Runtime
        };
        Self {
            kind,
            message: e.to_string().chars().take(512).collect(),
        }
    }
}

pub(crate) struct BotConfig<'a> {
    pub behavior_seed: Option<u32>,
    pub map: &'a ArenaMap,
    pub clock: ClockRules,
    pub seat: Seat,
}

pub struct LuauBot {
    _lua: Lua,
    function: Function,
    view: view::Observation,
    remaining: Rc<Cell<u32>>,
}

impl LuauBot {
    /// 为工具和测试同步创建指定席位的机器人；正式比赛在工作线程上运行相同的虚拟机。
    pub fn for_world(script: &Script, world: &World, slot: SlotId) -> Result<Self> {
        let tank = world
            .tank(slot)
            .ok_or_else(|| Error::runtime("unknown bot slot"))?;
        Self::load(
            script,
            BotConfig {
                map: world.map(),
                clock: world.clock(),
                seat: tank.seat,
                behavior_seed: world.pilot_seed(slot),
            },
            Arc::default(),
        )
    }

    pub(crate) fn load(
        source: &Script,
        config: BotConfig<'_>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let lua = Lua::new();
        lua.set_compiler(
            mlua::chunk::Compiler::new()
                .set_optimization_level(2)
                .set_type_info_level(1),
        );
        #[cfg(feature = "luau-native")]
        lua.enable_jit(true); // 不支持原生编译的处理器由 mlua 回退到字节码。

        lua.set_memory_limit(VM_BYTES)?;
        for key in [
            "os",
            "io",
            "require",
            "debug",
            "loadstring",
            "collectgarbage",
            "print",
            "warn",
        ] {
            lua.globals().raw_set(key, mlua::Nil)?;
        }

        lua.sandbox(true)?;

        let remaining = Rc::new(Cell::new(CHECKPOINTS_PER_CALL));
        let budget = Rc::clone(&remaining);
        lua.set_interrupt(move |_| {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::runtime("match closed"));
            }

            budget.set(
                budget
                    .get()
                    .checked_sub(1)
                    .ok_or_else(|| Error::external(Exhausted))?,
            );
            Ok(VmState::Continue)
        });

        let view = view::Observation::new(&lua, &config)?;
        let function = lua
            .load(source.text())
            .set_name(source.name())
            .set_mode(ChunkMode::Text)
            .call::<Function>(())?;
        Ok(Self {
            _lua: lua,
            function,
            view,
            remaining,
        })
    }

    pub fn decide(&mut self, world: &World) -> Result<ControlUpdate> {
        let sight = world
            .sight(self.view.slot)
            .ok_or_else(|| Error::runtime("unknown bot slot"))?;
        self.control(
            world.tick(),
            sight,
            world.tanks(),
            world.shells(),
            |_, _| {},
        )
    }

    pub(crate) fn control(
        &mut self,
        tick: u64,
        sight: Sight,
        tanks: &[Tank],
        shells: &[Shell],
        measure: impl FnOnce(Instant, Duration),
    ) -> Result<ControlUpdate> {
        let observation = self.view.update(tick, sight, tanks, shells)?;
        self.remaining.set(CHECKPOINTS_PER_CALL);

        // 只测脚本调用；构造观察表、解析更新以及等待信箱不混入此数。
        let start = Instant::now();
        let result = self.function.call::<Value>(observation);
        measure(start, start.elapsed());

        if self.remaining.get() == 0 {
            return Err(Error::external(Exhausted));
        }

        control::parse(result?)
    }
}

#[cfg(test)]
mod strategy_tests;
