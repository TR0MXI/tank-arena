//! 纯 Rust 裁判：不读取文件、时钟或渲染实体。

mod balance;
mod geometry;
mod map;
mod rules;
mod world;

pub use balance::*;
pub use map::{ArenaMap, Heading, MAP_LIMIT, MapError, Point, SWITCHYARD, Spawn, WallRect};
pub use rules::*;
pub use world::*;
