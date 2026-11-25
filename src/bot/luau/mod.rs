mod control;
mod modules;
mod source;
mod view;

use crate::sim::{
    ArenaMap, ClockRules, ControlUpdate, Items, Seat, Shell, Sight, SlotId, Tank, World,
};
use bitcode::{Decode, Encode};
use mlua::{Error, Function, Lua, Result, Value, VmState, chunk::ChunkMode};
use serde::{Deserialize, Serialize};
pub use source::{Script, SourceError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub enum FaultKind {
    Load,
    Runtime,
    Action,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Fault {
    pub kind: FaultKind,
    pub message: String,
}

impl Fault {
    pub fn classify(e: Error, loading: bool) -> Self {
        let kind = if e
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
    lua: Lua,
    function: Function,
    view: view::Observation,
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

        for key in [
            "os",
            "io",
            "debug",
            "loadstring",
            "collectgarbage",
            "print",
            "warn",
        ] {
            lua.globals().raw_set(key, mlua::Nil)?;
        }

        modules::install(&lua, source)?;
        lua.sandbox(true)?;

        // 不限制脚本工作量，但停止比赛时仍须能中断初始化和决策中的循环。
        lua.set_interrupt(move |_| {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::runtime("match closed"));
            }
            Ok(VmState::Continue)
        });

        let view = view::Observation::new(&lua, &config)?;
        let function = lua
            .load(source.text())
            .set_name(source.name())
            .set_mode(ChunkMode::Text)
            .call::<Function>(())?;
        Ok(Self {
            lua,
            function,
            view,
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
            world.items(),
            |_, _| {},
        )
    }

    pub(crate) fn control(
        &mut self,
        tick: u64,
        sight: Sight,
        tanks: &[Tank],
        shells: &[Shell],
        items: &Items,
        measure: impl FnOnce(Instant, Duration),
    ) -> Result<ControlUpdate> {
        let observation = self
            .view
            .update(&self.lua, tick, sight, tanks, shells, items)?;

        // 只测脚本调用；构造观察表、解析更新以及等待信箱不混入此数。
        let start = Instant::now();
        let result = self.function.call::<Value>(observation);
        measure(start, start.elapsed());

        control::parse(result?)
    }
}
