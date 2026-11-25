use super::*;
use std::collections::VecDeque;
#[derive(Clone, Copy)]
pub(crate) struct Goal {
    pub point: Point,
    pub enemy: Car,
    pub combat: bool,
    pub task: &'static str,
    pub supply: Option<CrateDrop>,
    pub tempo: f64,
    pub relief_from: Option<SlotId>,
}
fn cell(p: Point, w: usize, h: usize) -> usize {
    (p.x / CELL).clamp(0, w as i32 - 1) as usize + w * (p.y / CELL).clamp(0, h as i32 - 1) as usize
}
fn center(i: usize, w: usize) -> Point {
    Point {
        x: (i % w) as i32 * CELL + CELL / 2,
        y: (i / w) as i32 * CELL + CELL / 2,
    }
}
fn field(map: &ArenaMap, start: usize) -> Vec<(u16, usize)> {
    let (w, h) = map.size();
    let mut path = vec![(u16::MAX, start); w * h];
    path[start] = (0, start);
    let mut q = VecDeque::from([start]);
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % w, i / w);
        for (a, b) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if !map.passage((x, y), (a, b)) {
                continue;
            }
            let j = a + b * w;
            if path[j].0 == u16::MAX {
                path[j] = (path[i].0 + 1, i);
                q.push_back(j)
            }
        }
    }
    path
}
pub(crate) fn seeker(v: &View, p: Point, owner: SlotId) -> Option<(Point, Seat)> {
    let (w, h) = v.map.size();
    let start = cell(p, w, h);
    let path = field(v.map, start);
    let c = v.cars.iter().min_by(|a, b| {
        let rank = |c: &Car| (path[cell(c.pos, w, h)].0, c.seat.slot == owner);
        rank(a)
            .cmp(&rank(b))
            .then_with(|| dist(p, a.pos).total_cmp(&dist(p, b.pos)))
    })?;
    let mut end = cell(c.pos, w, h);
    let aim = if path[end].0 <= 1 {
        c.pos
    } else {
        while path[end].1 != start {
            end = path[end].1
        }
        center(end, w)
    };
    Some((aim, c.seat))
}
pub(crate) fn mission(
    v: &View,
    velocity: &[Point; 8],
    reads: &[OpponentRead; 8],
    incumbent: Option<CrateDrop>,
    firing: bool,
    stalled: bool,
) -> Goal {
    let m = v.me;
    let (w, h) = v.map.size();
    let start = cell(m.pos, w, h);
    let path = field(v.map, start);
    let cost = |p: Point| {
        let d = path[cell(p, w, h)].0 as f64;
        0.75 * d + 0.25 * dist(m.pos, p)
    };
    let visible = |p: Point| s::wall(v.map, m.pos, sub(p, m.pos), SIZES.shell_radius).is_none();
    // Keep the original geometry rank. Only break a crossfire toward a vulnerable
    // *current* line, not a weak opponent in a different room or behind a shield.
    let enemies = || v.cars.iter().filter(|c| c.seat.team != m.seat.team);
    let mut lanes = [false; 8];
    let mut ranks = [0.; 8];
    for e in enemies() {
        let i = ix(e.seat.slot);
        let sightline = visible(e.pos);
        ranks[i] =
            cost(e.pos) - if sightline { 1.6 } else { 0. } + if e.shield > 24 { 2. } else { 0. };
        lanes[i] = sightline && dist(m.pos, e.pos) < 4.;
    }
    // Even a good shooter can have a readable patrol. Exploit its *current*
    // blind phase only; a finished or interrupted turn earns no discount.
    let opening = |e: &Car| {
        let i = ix(e.seat.slot);
        if reads[i].consecutive_laps < 2
            || !lanes[i]
            || dist(m.pos, e.pos) > 3.2
            || e.shield > 0
            || e.weapon.is_some()
            || err(m.h, sub(e.pos, m.pos)).abs() > 0.90
            || err(e.h, sub(m.pos, e.pos)).abs() < 1.55
        {
            0.
        } else {
            0.48
        }
    };
    let plain = *enemies()
        .min_by(|a, b| ranks[ix(a.seat.slot)].total_cmp(&ranks[ix(b.seat.slot)]))
        .expect("living opponent");
    let crossfire = lanes.iter().filter(|x| **x).count() >= 2;
    let armed = v.sight.ammo > 0 && m.weapon != Some(Item::Mine);
    if crossfire && armed {
        for e in enemies() {
            let i = ix(e.seat.slot);
            let weak = reads[i].weakness().max(opening(e));
            let accurate = reads[i].shooting;
            if !lanes[i] || e.shield > 0 || (weak < 0.18 && accurate < 0.25) {
                continue;
            }
            let aim = err(m.h, sub(e.pos, m.pos)).abs();
            if aim > 1.4 {
                continue;
            }
            // Removing the sole gun on one bearing buys more space than removing
            // one of several on nearly the same bearing. Never rank through traffic.
            let line = sub(e.pos, m.pos);
            if v.cars.iter().any(|o| {
                o.seat != e.seat
                    && o.seat != m.seat
                    && s::body_hit(m.pos, line, &o.tank(), SIZES.shell_radius).is_some()
            }) {
                continue;
            }
            let bearing = err(m.h, line);
            let alone = !enemies().any(|o| {
                o.seat != e.seat
                    && lanes[ix(o.seat.slot)]
                    && (err(m.h, sub(o.pos, m.pos)) - bearing).cos() > 0.84
            });
            // A proven gun on its own bearing can also be the *right* first
            // elimination, even when not weak. Only if a short shot is already
            // within reach; no speculative rush at that gun through the others.
            let urgent = if alone
                && aim < 0.48
                && dist(m.pos, e.pos) < 2.45
                && err(e.h, sub(m.pos, e.pos)).abs() < 1.25
            {
                accurate.max(0.)
            } else {
                0.
            };
            ranks[i] -= (weak * (if alone { 1.5 } else { 0.65 })).max(urgent) / (1. + aim);
        }
    }
    let enemy = *enemies()
        .min_by(|a, b| ranks[ix(a.seat.slot)].total_cmp(&ranks[ix(b.seat.slot)]))
        .unwrap();
    let read = reads[ix(enemy.seat.slot)];
    let mut tempo = read.tempo();
    if opening(&enemy) > 0. {
        tempo = tempo.max(opening(&enemy) - read.shooting.max(0.) * 0.8);
    }
    let range = dist(m.pos, enemy.pos);
    let facing = err(enemy.h, sub(m.pos, enemy.pos)).abs();
    if tempo.abs() < 0.18
        || !armed
        || enemy.shield > 0
        || (tempo > 0.
            && (crossfire || v.sight.ammo < 2 || (range < 1.5 && facing < 0.45 && m.shield < 20)))
        || (tempo < 0. && (facing > 1.4 || (firing && range < 1.35)))
    {
        tempo = 0.;
    }
    let mut goal = enemy.pos;
    let mut task = "flank";
    let mut combat = visible(goal);
    if combat {
        task = "engage";
        let lead = add(
            goal,
            mul(
                velocity[ix(enemy.seat.slot)],
                (dist(m.pos, goal) * 20.).min(22.),
            ),
        );
        if visible(lead) {
            goal = lead;
        }
    }
    // A box normally spawns at least four cells from *everyone*. Comparing its
    // distance only to the nearest enemy made the previous pilot almost never collect one.
    let mut supply = None;
    let target_range = dist(m.pos, enemy.pos);
    let mut best = 0.;
    for &c in v.crates {
        let d = if s::wall(
            v.map,
            m.pos,
            sub(c.center, m.pos),
            SIZES.half_width + 5 * PX,
        )
        .is_none()
        {
            dist(m.pos, c.center)
        } else {
            cost(c.center) + 0.4
        };
        let eta = d * if m.boost > 45 { 31. } else { 40. };
        let loaded = m.weapon.is_some();
        let value = match c.item {
            Item::Shield if m.shield > eta as u64 => continue,
            Item::Speed if m.boost > eta as u64 => continue,
            Item::Shield => 8.4,
            Item::Speed => 6.6,
            _ if v.weapon_slots_used >= 3 => continue,
            Item::Laser | Item::Shotgun => 7.5,
            Item::Mine => 5.5,
            _ => 7.0,
        };
        // A rear trap replaces the forward gun; a distant one is not a
        // good reload when we are already contesting a nearby opponent.
        if c.item == Item::Mine
            && d > 2.5
            && target_range < 2.7
            && !stalled
            && (!combat || err(m.h, sub(enemy.pos, m.pos)).abs() < 1.9)
        {
            continue;
        }
        let armed = !matches!(c.item, Item::Speed | Item::Shield);
        // Preserve a conversion already on the barrel, don't race a rival that has
        // a much shorter unblocked approach, and don't compete with a closer ally.
        if (firing || (combat && target_range < 1.0 && v.sight.ammo > 0)) && d > 1.15 {
            continue;
        }
        // A far box cannot solve an immediate duel, particularly when the
        // opponent is inside one hull turn. Keep a nearby pickup in consideration.
        let wingman = v.cars.iter().any(|a| {
            a.seat.team == m.seat.team
                && a.seat.slot != m.seat.slot
                && dist(a.pos, enemy.pos) < 4.0
                && (a.shield > 36
                    || a.weapon.is_some()
                    || v.balls
                        .iter()
                        .filter(|b| b.owner == a.seat.slot && b.kind.is_none())
                        .count()
                        < 4)
        });
        let resupply = stalled
            || (wingman && target_range > 1.7)
            || (incumbent == Some(c) && target_range > 2.2);
        if !resupply
            && d > 2.2
            && (combat && target_range < 1.7
                || target_range < 2.5
                    && d > target_range + 1.8
                    && (m.weapon.is_some() || v.sight.ammo >= 2))
        {
            continue;
        }
        if !resupply
            && d > 2.2
            && !combat
            && target_range < 2.1
            && v.balls.iter().any(|b| dist(b.pos, m.pos) < 1.5)
        {
            continue;
        }
        if v.cars.iter().any(|e| {
            e.seat.slot != m.seat.slot
                && dist(e.pos, c.center) + if e.seat.team == m.seat.team { 0.6 } else { 1.5 } < d
                && s::wall(v.map, e.pos, sub(c.center, e.pos), SIZES.half_width).is_none()
        }) {
            continue;
        }
        let pressure = if combat {
            (2.7 - target_range).max(0.)
        } else {
            0.
        };
        let scarce = !loaded && v.sight.ammo <= 1 && v.next_refill() > 45;
        let score = value + if stalled { 3.5 } else { 0. }
            - d
            - pressure * if scarce { 0.50 } else { 1.35 }
            - if loaded && armed {
                1.6 + v.weapon_slots_used as f64
            } else {
                0.
            }
            + if scarce { 1.7 } else { 0. }
            + if incumbent == Some(c) { 1.2 } else { 0. };
        if score > best {
            best = score;
            supply = Some(c);
        }
    }
    if let Some(c) = supply {
        goal = c.center;
        combat = false;
        task = c.item.key();
    }
    // If the deployment split the team into separate duels, move toward a firing
    // position within supporting distance. Do not telepathically share a target or keys.
    if let Some(ally) = v
        .cars
        .iter()
        .filter(|c| c.seat.team == m.seat.team && c.seat.slot != m.seat.slot)
        .min_by(|a, b| dist(m.pos, a.pos).total_cmp(&dist(m.pos, b.pos)))
    {
        if dist(ally.pos, m.pos) > 3.25
            && dist(enemy.pos, m.pos) > 1.0
            && !v.sight.enemy_ahead
            && task == "flank"
            && v.cars.iter().any(|e| {
                e.seat.team != m.seat.team
                    && dist(e.pos, ally.pos) < 3.2
                    && s::wall(v.map, ally.pos, sub(e.pos, ally.pos), SIZES.shell_radius).is_none()
            })
        {
            let point = (0..w * h)
                .map(|i| center(i, w))
                .filter(|&p| dist(p, ally.pos) + 0.55 < dist(m.pos, ally.pos))
                .min_by(|&a, &b| {
                    let rank = |p: Point| {
                        0.58 * cost(p)
                            + 0.83 * dist(p, ally.pos)
                            + 0.35 * dist(p, enemy.pos)
                            + if s::wall(v.map, p, sub(enemy.pos, p), SIZES.shell_radius).is_none()
                            {
                                0.
                            } else {
                                0.65
                            }
                    };
                    rank(a).total_cmp(&rank(b))
                });
            if let Some(p) = point {
                combat = false;
                task = "support lane";
                goal = p;
            }
        }
    }
    if supply.is_none() && combat && m.weapon == Some(Item::Mine) && target_range < 2.8 {
        let x = start % w;
        let y = start / w;
        let escape = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ]
        .into_iter()
        .filter(|&n| v.map.passage((x, y), n))
        .map(|(x, y)| center(x + y * w, w))
        .filter(|&p| {
            dist(p, enemy.pos) > target_range + 0.35
                && v.cars
                    .iter()
                    .all(|c| c.seat.slot == m.seat.slot || dist(c.pos, p) > 0.65)
        })
        .min_by(|&a, &b| {
            let score = |p| err(m.h, sub(p, m.pos)).abs() * 0.3 - dist(p, enemy.pos);
            score(a).total_cmp(&score(b))
        });
        if let Some(exit) = escape {
            task = "mine withdrawal";
            combat = false;
            goal = exit;
        }
    }
    // When a clear centre-to-centre line never becomes a usable muzzle angle,
    // move off the wall instead of aiming indefinitely from the same footprint.
    if stalled && combat && supply.is_none() && !firing && target_range > 1.2 {
        let (x, y) = (start % w, start / w);
        if let Some(p) = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ]
        .into_iter()
        .filter(|&n| v.map.passage((x, y), n))
        .map(|(x, y)| center(x + y * w, w))
        .filter(|&p| {
            v.cars
                .iter()
                .all(|c| c.seat.slot == m.seat.slot || dist(c.pos, p) > 0.8)
        })
        .min_by(|&a, &b| {
            let rank = |p| {
                dist(p, m.pos)
                    + 0.4 * dist(p, enemy.pos)
                    + if s::wall(v.map, p, sub(enemy.pos, p), SIZES.shell_radius).is_some() {
                        0.8
                    } else {
                        0.
                    }
            };
            rank(a).total_cmp(&rank(b))
        }) {
            goal = p;
            combat = false;
            task = "reset firing angle";
        }
    }
    if !combat {
        let mut i = cell(goal, w, h);
        let mut points = vec![goal];
        while i != start {
            points.push(center(i, w));
            i = path[i].1;
        }
        points.push(center(start, w));
        goal = *points
            .iter()
            .find(|&&p| s::wall(v.map, m.pos, sub(p, m.pos), SIZES.half_width + 5 * PX).is_none())
            .unwrap_or(points.last().unwrap());
    }
    Goal {
        point: goal,
        enemy,
        combat,
        task,
        supply,
        tempo,
        relief_from: (plain.seat != enemy.seat).then_some(plain.seat.slot),
    }
}
