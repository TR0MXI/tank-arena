//! 机器人目录、Luau 脚本运行时与内置原生策略。

pub(crate) mod avalon;
pub mod catalog;
mod luau;

pub(crate) use luau::BotConfig;
pub use luau::{Fault, FaultKind, LuauBot, Script, SourceError};
