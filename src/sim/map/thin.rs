use super::*;
use crate::sim::WALL_HALF as HALF;

#[derive(Debug, Clone)]
pub(in crate::sim) struct Maze {
    // 整条共线的墙只存一个矩形，连续墙段没有能使炮弹反向的内部接缝。
    horizontal: Vec<Vec<WallRect>>,
    vertical: Vec<Vec<WallRect>>,
    openings: Vec<u8>,
    width: usize,
}

const EAST: u8 = 1;
const SOUTH: u8 = 2;
const WEST: u8 = 4;
const NORTH: u8 = 8;

impl Maze {
    pub(super) fn parse(rows: &[&str]) -> Result<ArenaMap, MapError> {
        let (cols, lines) = (rows[0].len(), rows.len());
        let (w, h) = (cols / 2, lines / 2);
        if cols.is_multiple_of(2)
            || lines.is_multiple_of(2)
            || !(3..=64).contains(&w)
            || !(3..=64).contains(&h)
            || rows.iter().any(|r| r.len() != cols)
        {
            return Err(MapError::Dimensions);
        }

        let at = |x: usize, y: usize| rows[y].as_bytes()[x];
        let mut spawns = Vec::new();
        let mut openings = Vec::with_capacity(w * h);
        for y in 0..lines {
            for x in 0..cols {
                let b = at(x, y);
                let valid = match (x % 2, y % 2) {
                    (0, 0) => b == b'+',
                    (1, 0) => {
                        if (y == 0 || y + 1 == lines) && b != b'-' {
                            return Err(MapError::OpenEdge(x + 1, y + 1));
                        }
                        b == b'-' || b == b' '
                    }
                    (0, 1) => {
                        if (x == 0 || x + 1 == cols) && b != b'|' {
                            return Err(MapError::OpenEdge(x + 1, y + 1));
                        }
                        b == b'|' || b == b' '
                    }
                    _ => {
                        openings.push(
                            [(1, 0, EAST), (0, 1, SOUTH), (-1, 0, WEST), (0, -1, NORTH)]
                                .into_iter()
                                .fold(0, |mask, (dx, dy, bit)| {
                                    if at(x.wrapping_add_signed(dx), y.wrapping_add_signed(dy))
                                        == b' '
                                    {
                                        mask | bit
                                    } else {
                                        mask
                                    }
                                }),
                        );
                        if let Some(s) = spawn(b, x / 2, y / 2) {
                            spawns.push(s);
                            true
                        } else {
                            b == b'.'
                        }
                    }
                };
                if !valid {
                    return Err(MapError::Tile(x + 1, y + 1));
                }
            }
        }

        if spawns.len() < 2 {
            return Err(MapError::Spawns);
        }

        let mut seen = vec![false; w * h];
        let mut pending = VecDeque::from([0]);
        seen[0] = true;
        while let Some(i) = pending.pop_front() {
            for (bit, offset) in [
                (EAST, 1),
                (SOUTH, w as isize),
                (WEST, -1),
                (NORTH, -(w as isize)),
            ] {
                if openings[i] & bit != 0 {
                    let j = i.wrapping_add_signed(offset);
                    if !seen[j] {
                        seen[j] = true;
                        pending.push_back(j);
                    }
                }
            }
        }

        if seen.contains(&false) {
            return Err(MapError::Disconnected);
        }

        let horizontal = (0..=h)
            .map(|y| runs(y, w, |x| at(2 * x + 1, 2 * y) == b'-', false))
            .collect();
        let vertical = (0..=w)
            .map(|x| runs(x, h, |y| at(2 * x, 2 * y + 1) == b'|', true))
            .collect();
        Ok(ArenaMap {
            width: w,
            height: h,
            layout: Self {
                horizontal,
                vertical,
                openings,
                width: w,
            },
            spawns,
            source: rows.join("\n"),
        })
    }

    pub(super) fn passage(&self, x: usize, y: usize, u: usize, v: usize) -> bool {
        let direction = if u > x {
            EAST
        } else if v > y {
            SOUTH
        } else if x > u {
            WEST
        } else {
            NORTH
        };
        self.openings[y * self.width + x] & direction != 0
    }

    pub(super) fn rectangles(&self) -> impl Iterator<Item = &WallRect> {
        self.horizontal.iter().chain(&self.vertical).flatten()
    }

    /// 只访问扫掠包围盒覆盖的格线；子步不分配，也不扫描整张迷宫。
    pub(in crate::sim) fn candidates(
        &self,
        p: Point,
        end: Point,
        r: i32,
    ) -> impl Iterator<Item = &WallRect> {
        fn lines(
            axis: &[Vec<WallRect>],
            a: i32,
            b: i32,
            r: i32,
        ) -> impl Iterator<Item = &WallRect> {
            let lo = (a.min(b) - r - 2 * HALF + CELL - 1)
                .div_euclid(CELL)
                .clamp(0, axis.len() as i32);
            let hi = ((a.max(b) + r + 2 * HALF).div_euclid(CELL) + 1).clamp(lo, axis.len() as i32);
            axis[lo as usize..hi as usize].iter().flatten()
        }
        lines(&self.horizontal, p.y, end.y, r).chain(lines(&self.vertical, p.x, end.x, r))
    }
}

fn runs(line: usize, len: usize, exists: impl Fn(usize) -> bool, vertical: bool) -> Vec<WallRect> {
    let center = line as i32 * CELL;
    let mut result = Vec::new();
    let mut i = 0;
    while i < len {
        if !exists(i) {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;
        while i < len && exists(i) {
            i += 1;
        }

        let point = |along, across| {
            if vertical {
                Point {
                    x: across,
                    y: along,
                }
            } else {
                Point {
                    x: along,
                    y: across,
                }
            }
        };
        result.push(WallRect {
            min: point(start as i32 * CELL - HALF, center - HALF),
            max: point(i as i32 * CELL + HALF, center + HALF),
        });
    }

    result
}
