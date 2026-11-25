//! Short, revocable moves between neighbouring wall corners. No multi-tick controls:
//! the normal collision / projectile search still owns each individual move.
use super::{route::Goal, *};
type Cell = (usize, usize);
fn tile(p: Point) -> Cell {
    ((p.x / CELL) as usize, (p.y / CELL) as usize)
}
fn center((x, y): Cell) -> Point {
    Point {
        x: x as i32 * CELL + CELL / 2,
        y: y as i32 * CELL + CELL / 2,
    }
}
fn neighbors(map: &ArenaMap, (x, y): Cell) -> impl Iterator<Item = Cell> + '_ {
    [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ]
    .into_iter()
    .filter(move |&j| map.passage((x, y), j))
}
fn clear(v: &View, p: Point, q: Point, r: i32) -> bool {
    s::wall(v.map, p, sub(q, p), r).is_none()
}
#[derive(Default)]
struct Lines {
    mask: u8,
    aimed: f64,
    nearest: f64,
}
fn lines(v: &View, p: Point) -> Lines {
    let mut l = Lines {
        nearest: 99.,
        ..Default::default()
    };
    for e in v.cars.iter().filter(|c| c.seat.team != v.me.seat.team) {
        let d = dist(e.pos, p);
        if d > 4. || !clear(v, e.pos, p, SIZES.shell_radius) {
            continue;
        }
        l.mask |= 1 << ix(e.seat.slot);
        l.nearest = l.nearest.min(d);
        l.aimed += (1. - err(e.h, sub(p, e.pos)).abs() / 0.8).max(0.) * (1.3 - 0.15 * d);
    }
    l
}
fn occupied(v: &View, p: Point) -> bool {
    v.cars
        .iter()
        .any(|c| c.seat.slot != v.me.seat.slot && dist(p, c.pos) < 0.60)
}
// Search only for a *possible* second angle beyond this corner. The next
// waypoint is picked again from fresh poses; no tail of this route is binding.
fn opening(v: &View, anchor: Point, previous: Cell, here: Cell) -> Option<(f64, Point)> {
    let mut best = None;
    let mut stack = vec![(here, previous, 0u8, center(here), 0.0)];
    while let Some((at, back, depth, first, cost)) = stack.pop() {
        for next in neighbors(v.map, at).filter(|n| *n != back && *n != here && *n != previous) {
            let p = center(next);
            if occupied(v, p) {
                continue;
            }
            let l = lines(v, p);
            let first = if depth == 0 { p } else { first };
            if l.mask.count_ones() == 1 && l.nearest > 0.85 && neighbors(v.map, next).count() >= 2 {
                let e = v
                    .cars
                    .iter()
                    .find(|e| l.mask & (1 << ix(e.seat.slot)) != 0)
                    .unwrap();
                let a = sub(anchor, e.pos);
                let b = sub(p, e.pos);
                let dot = a.x as f64 * b.x as f64 + a.y as f64 * b.y as f64;
                if dot < 0.90 * norm(a) * norm(b) {
                    let score = cost + 0.35 * l.nearest + l.aimed;
                    if best.is_none_or(|(x, _)| score < x) {
                        best = Some((score, first));
                    }
                }
            } else if l.mask == 0 && depth < 2 {
                stack.push((next, at, depth + 1, first, cost + 0.55));
            }
        }
    }
    best
}
#[derive(Clone, Copy)]
struct Hop {
    dest: Point,
    from: Cell,
    anchor: Point,
    best: f64,
    progress: u64,
    exposed: u8,
    until: u64,
    peek: bool,
    began: u64,
}
#[derive(Default)]
pub(super) struct Tactics {
    hop: Option<Hop>,
    pressure: u8,
    check: u64,
}
impl Tactics {
    fn apply(v: &View, g: &mut Goal, h: Hop) {
        let p = v.me.pos;
        g.point = if clear(v, p, h.dest, SIZES.half_width + 5 * PX) {
            h.dest
        } else {
            center(tile(p))
        };
        g.combat = false;
        g.task = if h.peek {
            "peek from wall"
        } else {
            "turn behind wall"
        };
    }
    fn set(&mut self, v: &View, g: &mut Goal, dest: Point, peek: bool) {
        let d = dist(v.me.pos, dest);
        let h = Hop {
            dest,
            from: tile(v.me.pos),
            anchor: v.me.pos,
            best: d,
            progress: v.tick,
            exposed: 0,
            until: v.tick + (d * 48.).ceil() as u64 + 62,
            peek,
            began: v.tick,
        };
        self.hop = Some(h);
        Self::apply(v, g, h);
    }
    pub(super) fn started(&self, t: u64) -> Option<Point> {
        self.hop.filter(|h| h.began == t).map(|h| h.dest)
    }
    pub(super) fn abort(&mut self, t: u64) {
        self.hop = None;
        self.pressure = 0;
        self.check = t + 24;
    }
    pub(super) fn navigate(&mut self, v: &View, g: &mut Goal, shoot: bool) {
        let m = v.me;
        if !matches!(g.task, "engage" | "flank")
            || m.shield > 36
            || v.cars.iter().filter(|c| c.seat.team != m.seat.team).count() < 2
        {
            if self.hop.is_some() {
                self.abort(v.tick)
            } else {
                self.pressure = 0;
            }
            return;
        }
        if self.hop.is_none() && v.tick < self.check {
            return;
        }
        let now = lines(v, m.pos);
        let n = now.mask.count_ones();
        let snipe = shoot && g.combat && (dist(m.pos, g.enemy.pos) < 1.5 || n < 2);
        if let Some(mut h) = self.hop {
            let d = dist(m.pos, h.dest);
            if d < 0.27 {
                self.abort(v.tick);
                if !h.peek
                    && n <= 1
                    && !shoot
                    && (m.weapon.is_some() || v.sight.ammo > 0 || v.next_refill() < 30)
                {
                    let peek = opening(v, h.anchor, h.from, tile(m.pos));
                    if let Some((_, p)) = peek {
                        self.set(v, g, p, true);
                    }
                }
                return;
            }
            if d + 0.07 < h.best {
                h.best = d;
                h.progress = v.tick;
            }
            // Do not U-turn in an open doorway on one bad snapshot. Check the
            // destination repeatedly; a near shot or a blocked exit still interrupts.
            if v.tick.is_multiple_of(6) {
                h.exposed = if lines(v, h.dest).mask.count_ones() >= 2 {
                    h.exposed.saturating_add(1)
                } else {
                    0
                };
            }
            if snipe
                || v.tick >= h.until
                || v.tick.saturating_sub(h.progress) > 42
                || h.exposed >= 4
                || v.tick.is_multiple_of(6) && occupied(v, h.dest)
            {
                self.abort(v.tick);
                return;
            }
            self.hop = Some(h);
            Self::apply(v, g, h);
            return;
        }
        self.pressure = if n >= 2 {
            self.pressure.saturating_add(1)
        } else {
            0
        };
        if self.pressure < 8 || snipe {
            return;
        }
        self.check = v.tick + 8;
        // An active bank shot can already be holding this corner. Do not
        // abandon a real near-term intercept in favour of speculative cover.
        for &b in v.balls.iter().filter(|b| b.owner == m.seat.slot) {
            let mut hits = false;
            ball::trace(v, b, 40, |_, leg| {
                if let Some((_, c)) = v
                    .cars
                    .iter()
                    .filter(|c| c.seat.slot != leg.owner || leg.armed)
                    .filter_map(|c| {
                        s::body_hit(leg.p, leg.d, &c.tank(), leg.r).map(|d| (norm(d), c))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                {
                    hits = c.seat.team != m.seat.team && c.shield == 0;
                    false
                } else {
                    true
                }
            });
            if hits {
                return;
            }
        }
        let start = tile(m.pos);
        let mut best = None;
        for j in neighbors(v.map, start) {
            let p = center(j);
            let end = lines(v, p);
            let count = end.mask.count_ones();
            if count != 0 || occupied(v, p) || neighbors(v.map, j).count() < 2 {
                continue;
            }
            // Losing a line by crossing the arbitrary range boundary is not cover.
            if !v.cars.iter().any(|e| {
                now.mask & (1 << ix(e.seat.slot)) != 0 && !clear(v, e.pos, p, SIZES.shell_radius)
            }) {
                continue;
            }
            let mid = add(m.pos, mul(sub(p, m.pos), 0.5));
            let mid_l = lines(v, mid);
            if mid_l.mask.count_ones() > n || mid_l.nearest < 0.75 {
                continue;
            }
            let reopen = opening(v, m.pos, start, j);
            // A usable side angle is better than merely hiding in a pocket.
            let rank = 0.24 * mid_l.aimed + 0.45 * dist(m.pos, p)
                - 0.18 * neighbors(v.map, j).count() as f64
                + reopen.map_or(2.8, |(cost, _)| 0.5 * cost);
            if best.is_none_or(|(s, _)| rank < s) {
                best = Some((rank, p));
            }
        }
        if let Some((_, p)) = best {
            self.set(v, g, p, false);
        }
    }
}

// Assess the actual control sequences, not just an attractive destination cell.
// Static enemy poses are only a short-horizon comparison. Real shell hits as
// predicted by the movement planner take precedence over this visibility proxy.
fn exposure_cost(v: &View, first: planner::Act, tail: &[planner::Act]) -> (f64, Point) {
    let mut tank = v.me.tank();
    let mut score = 0.;
    let mut end = tank.center;
    for (i, (drive, turn)) in std::iter::once(first)
        .chain(tail.iter().copied())
        .take(48)
        .enumerate()
    {
        tank.drive = drive;
        tank.turn = turn;
        let next = tank.proposed_pose(v.map, v.me.boost > i as u64);
        if !v
            .cars
            .iter()
            .any(|c| c.seat.slot != v.me.seat.slot && s::overlap(next, (c.pos, c.h)))
        {
            (tank.center, tank.heading) = next;
        }
        if (i + 1) % 6 == 0 {
            let x = lines(v, tank.center);
            score += f64::from(x.mask.count_ones().saturating_sub(1)) + 0.40 * x.aimed;
            end = tank.center;
        }
    }
    (score, end)
}
pub(super) fn improves(
    v: &View,
    dest: Point,
    chosen: (planner::Act, &[planner::Act], Option<usize>),
    direct: (planner::Act, &[planner::Act], Option<usize>),
) -> bool {
    match (chosen.2, direct.2) {
        (Some(a), Some(b)) => return a > b,
        (Some(_), None) => return false,
        (None, Some(_)) => return true,
        _ => {}
    }
    let (a, q) = exposure_cost(v, chosen.0, chosen.1);
    let (b, _) = exposure_cost(v, direct.0, direct.1);
    dist(q, dest) + 0.15 < dist(v.me.pos, dest) && a + 0.9 < b
}
