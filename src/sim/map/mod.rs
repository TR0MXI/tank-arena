mod generate;
mod thin;

use super::CELL;
use bitcode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use thiserror::Error;

pub const MAP_LIMIT: usize = 16 * 1024;
pub const SWITCHYARD: &str = include_str!("../../../assets/maps/switchyard.map");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 一圈 2^32 个相位；地图坐标向右、向下为正。加减可自然跨过正东方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct Heading(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub center: Point,
    pub heading: Heading,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MapError {
    #[error("generated maps require 4..=32 cells on each axis")]
    GenerationSize,
    #[error("map exceeds 16 KiB")]
    TooLarge,
    #[error("expected a rectangular ASCII map, 3..=64 cells on each axis")]
    Dimensions,
    #[error("unknown tile at column {0}, row {1}")]
    Tile(usize, usize),
    #[error("map is not enclosed at column {0}, row {1}")]
    OpenEdge(usize, usize),
    #[error("map needs at least two spawn points")]
    Spawns,
    #[error("maze has disconnected floor cells")]
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallRect {
    pub min: Point,
    pub max: Point,
}

/// 地图只解析一次；裁判与渲染共享同一组世界坐标墙面。
#[derive(Clone, Debug)]
pub struct ArenaMap {
    width: usize,
    height: usize,
    pub(super) layout: thin::Maze,
    spawns: Vec<Spawn>,
    source: String,
}

fn spawn(byte: u8, x: usize, y: usize) -> Option<Spawn> {
    let angle = match byte {
        b'>' => 0,
        b'v' => 64,
        b'<' => 128,
        b'^' => 192,
        b'1' => 32,
        b'3' => 96,
        b'7' => 224,
        b'9' => 160,
        _ => return None,
    };
    Some(Spawn {
        center: Point {
            x: x as i32 * CELL + CELL / 2,
            y: y as i32 * CELL + CELL / 2,
        },
        heading: Heading::bearing(angle),
    })
}

impl std::str::FromStr for ArenaMap {
    type Err = MapError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() > MAP_LIMIT {
            return Err(MapError::TooLarge);
        }

        let rows: Vec<_> = text.lines().collect();
        if rows.is_empty() {
            return Err(MapError::Dimensions);
        }

        thin::Maze::parse(&rows)
    }
}

impl ArenaMap {
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn spawns(&self) -> &[Spawn] {
        &self.spawns
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn wall_rectangles(&self) -> impl Iterator<Item = WallRect> + '_ {
        self.layout.rectangles().copied()
    }

    /// 相邻格心间能否直接穿行，不把内部墙的一侧误判为整格实心。
    pub fn passage(&self, (x, y): (usize, usize), (u, v): (usize, usize)) -> bool {
        if x >= self.width
            || u >= self.width
            || y >= self.height
            || v >= self.height
            || x.abs_diff(u) + y.abs_diff(v) != 1
        {
            return false;
        }

        self.layout.passage(x, y, u, v)
    }
}
