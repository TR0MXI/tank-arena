//! 纯 Rust 裁判：不读取文件、时钟或渲染实体。

// Official arena and replay format always have eight seats.
const ROSTER_CAPACITY: usize = 8;

mod balance;
mod geometry;
mod map;
mod prediction;
pub(crate) use prediction::*;
mod rules;
mod world;

pub use balance::*;
pub use map::{ArenaMap, Heading, MAP_LIMIT, MapError, Point, SWITCHYARD, Spawn, WallRect};
pub use rules::*;
pub use world::*;
