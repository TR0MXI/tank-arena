use super::*;
use super::{
    ball::{self, Leg},
    route::Goal,
};
pub(crate) type Act = (Drive, Turn);
const IDLE: Act = (Drive::Stop, Turn::None);

/// Last plan plus observed movement. A submitted key is not evidence that a
/// blocked tank moved, or that a late decision was accepted.
#[derive(Default)]
pub(super) struct MotionHistory {
    prior: Option<Act>,
    tail: Vec<Act>,
    pose: Option<(Point, Heading)>,
    steps: [Option<(Point, Act)>; 6], // newest first
}
impl MotionHistory {
    pub(super) fn observe(&mut self, v: &View, last_tick: Option<u64>) {
        let old = self.pose.replace((v.me.pos, v.me.h));
        if let Some((p, h)) = old
            && last_tick.and_then(|t| t.checked_add(1)) == Some(v.tick)
        {
            self.steps.rotate_right(1);
            self.steps[0] = Some((
                p,
                (
                    if p == v.me.pos {
                        Drive::Stop
                    } else {
                        v.sight.drive
                    },
                    if h == v.me.h {
                        Turn::None
                    } else {
                        v.sight.turn
                    },
                ),
            ));
        } else {
            self.steps.fill(None);
            self.tail.clear();
        }
    }

    pub(super) fn commit(&mut self, act: Act, tail: Vec<Act>) {
        self.prior = Some(act);
        self.tail = tail;
    }

    fn settle(&self, pos: Point, wanted: Act) -> Act {
        fn backtrack<A: Copy + PartialEq>(
            axis: impl Iterator<Item = A>,
            wanted: A,
            rest: A,
        ) -> bool {
            if wanted == rest {
                return false;
            }
            let mut motion = axis.filter(|&a| a != rest);
            motion.next().is_some_and(|a| a != wanted) && motion.any(|a| a == wanted)
        }
        // Only act on sustained local motion. One backstep at the end of a
        // charge, or reversing cleanly out of a dead end, is not a wobble.
        if !self.steps.iter().all(|sample| {
            sample.is_some_and(|(p, _)| {
                let d = sub(pos, p);
                d.x.abs().max(d.y.abs()) <= CELL / 12
            })
        }) {
            return wanted;
        }
        let nearby = self.steps.iter().flatten().map(|(_, a)| *a);
        (
            if backtrack(nearby.clone().map(|a| a.0), wanted.0, Drive::Stop) {
                Drive::Stop
            } else {
                wanted.0
            },
            if backtrack(nearby.map(|a| a.1), wanted.1, Turn::None) {
                Turn::None
            } else {
                wanted.1
            },
        )
    }
}

const ACTIONS: [Act; 9] = [
    (Drive::Forward, Turn::None),
    (Drive::Forward, Turn::Left),
    (Drive::Forward, Turn::Right),
    (Drive::Stop, Turn::Left),
    (Drive::Stop, Turn::Right),
    (Drive::Reverse, Turn::None),
    (Drive::Reverse, Turn::Left),
    (Drive::Reverse, Turn::Right),
    (Drive::Stop, Turn::None),
];
#[derive(Clone, Copy)]
struct Node {
    plan: [Act; 180],
    car: Tank,
    first: Act,
    act: Act,
    danger: f64,
    effort: f64,
    score: f64,
    hit: Option<usize>,
    hit_id: Option<u64>,
}
// Same map and fixed-point pose recur across beam branches, especially against walls.
// Only reuse the static proposed pose; traffic, time, ammo and shells are never cached.
#[derive(Clone, Copy)]
struct MoveEntry {
    from: (Point, Heading),
    action: Act,
    boost: bool,
    to: (Point, Heading),
}
const CACHE_BITS: u32 = 9;
const MOVE_SLOTS: usize = 1 << CACHE_BITS;
struct Moves<'a> {
    map: &'a ArenaMap,
    slots: [Option<MoveEntry>; MOVE_SLOTS],
}
impl<'a> Moves<'a> {
    fn new(map: &'a ArenaMap) -> Self {
        Self {
            map,
            slots: [None; MOVE_SLOTS],
        }
    }
    fn proposed(&mut self, mut car: Tank, action: Act, boost: bool) -> (Point, Heading) {
        let from = (car.center, car.heading);
        if action == (Drive::Stop, Turn::None) {
            return from;
        }
        // Hashing only selects a slot: equality on the complete key is mandatory.
        let hash = (car.center.x as u32).wrapping_mul(0x9e3779b9)
            ^ (car.center.y as u32).wrapping_mul(0x85ebca6b)
            ^ car.heading.0.wrapping_mul(0xc2b2ae35)
            ^ ((action.0 as u32 * 6 + action.1 as u32 * 2 + u32::from(boost))
                .wrapping_mul(0x27d4eb2d));
        let slot = &mut self.slots[(hash >> (u32::BITS - CACHE_BITS)) as usize];
        if let Some(e) = slot
            && e.from == from
            && e.action == action
            && e.boost == boost
        {
            return e.to;
        }
        (car.drive, car.turn) = action;
        let to = car.proposed_pose(self.map, boost);
        *slot = Some(MoveEntry {
            from,
            action,
            boost,
            to,
        });
        to
    }
}

// A rejection box only. It includes the entire swept segment plus *both* the
// rotated body and the near-miss margin; inside it the original tests still run.
#[derive(Clone, Copy)]
struct Swept {
    leg: Leg,
    lo: Point,
    hi: Point,
}
impl Swept {
    fn new(leg: Leg) -> Self {
        let reach = s::TANK_REACH.max(SIZES.half_length + 22 * PX) + leg.r;
        let p = leg.p;
        let q = add(p, leg.d);
        Self {
            leg,
            lo: Point {
                x: p.x.min(q.x) - reach,
                y: p.y.min(q.y) - reach,
            },
            hi: Point {
                x: p.x.max(q.x) + reach,
                y: p.y.max(q.y) + reach,
            },
        }
    }
    #[inline]
    fn near(&self, p: Point, travel: i32) -> bool {
        p.x + travel >= self.lo.x
            && p.x - travel <= self.hi.x
            && p.y + travel >= self.lo.y
            && p.y - travel <= self.hi.y
    }
}
fn travel(m: Car, t: usize) -> i32 {
    // Every rounded per-axis displacement is bounded by this integer speed.
    // Blocking, turning and laser lock can only reduce the distance covered.
    let boosted = m.boost.min(t as u64) as i32;
    let speed = s::FORWARD_SPEED.max(s::REVERSE_SPEED);
    (t as i32 - boosted) * speed + boosted * (speed * (100 + s::BOOST_PERCENT) / 100)
}
#[derive(Default)]
struct Traffic {
    poses: [(Point, Heading); s::MAX_SLOTS],
    len: usize,
}
impl Traffic {
    fn blocked(&self, (p, h): (Point, Heading)) -> bool {
        self.poses[..self.len].iter().any(|&(q, k)| {
            (p.x - q.x).abs() <= 2 * s::TANK_REACH
                && (p.y - q.y).abs() <= 2 * s::TANK_REACH
                && s::overlap((p, h), (q, k))
        })
    }
}
struct Scene<'a> {
    v: &'a View<'a>,
    goal: &'a Goal,
    shots: Vec<Vec<Swept>>,
    possible: Vec<Vec<Leg>>,
    traffic: [Traffic; 13],
    lock: usize,
    normal_ready_in: u16,
    moves: Moves<'a>,
}
fn advance(scn: &mut Scene, n: &mut Node, a: Act, from: usize, to: usize) {
    let v = scn.v;
    let m = v.me;
    let old = n.act;
    n.act = a;
    n.plan[from..to].fill(a);
    n.effort += if old != a { 0.042 } else { 0. };
    for t in from + 1..=to {
        let boost = m.boost >= t as u64;
        let p = n.car.center;
        let old_h = n.car.heading;
        let a = if t <= scn.lock {
            (Drive::Stop, Turn::None)
        } else {
            a
        };
        let (mut q, mut h) = scn.moves.proposed(n.car, a, boost);
        if (q, h) != (p, old_h) && scn.traffic[t.min(12)].blocked((q, h)) {
            q = p;
            h = old_h;
        }
        n.car.center = q;
        n.car.heading = h;
        if a.0 != Drive::Stop && p == q {
            n.effort += 0.013;
        }
        if m.shield < t as u64 && n.hit.is_none() {
            let mut tight: f64 = 0.;
            for shot in &scn.shots[t] {
                if !shot.near(q, 0) {
                    continue;
                }
                let l = &shot.leg;
                // Two nominally safe actions are not equal if one skims the shell with no
                // adjustment room. A new input/body block next tick can erase that margin.
                if t <= 55 {
                    let dx = (q.x - l.p.x - l.d.x / 2).abs();
                    let dy = (q.y - l.p.y - l.d.y / 2).abs();
                    let r = SIZES.half_length + l.r + 22 * PX;
                    if dx < r && dy < r {
                        let d = (dx as f64).hypot(dy as f64);
                        tight = tight.max((1. - d / r as f64).max(0.));
                    }
                }

                if ball::touches(*l, &n.car) {
                    n.hit = Some(t);
                    n.hit_id = Some(l.id);
                    n.danger += 2000. / (1. + t as f64 / 30.);
                    break;
                }
            }
            n.effort += tight * 0.8;
        }
        // A nearby aimed enemy CAN shoot, but has not necessarily fired. Do not veto
        // a winning move on that hypothesis; cost time spent inside a plausible line.
        if t <= 35
            && m.shield < t as u64
            && scn.possible[t].iter().any(|l| ball::touches(*l, &n.car))
        {
            n.effort += 0.24;
        }
        // Early pose matters even when a long tail is safe. This preserves immediate initiative.
        if t <= 24 && t % 4 == 0 {
            n.effort +=
                0.08 * dist(q, scn.goal.point) + 0.045 * err(h, sub(scn.goal.point, q)).abs();
        }
    }
    let g = scn.goal;
    let p = n.car.center;
    let d = dist(p, g.point);
    let aim = err(n.car.heading, sub(g.point, p)).abs();
    let pressure = if g.combat {
        let range = dist(p, g.enemy.pos);
        let facing = err(g.enemy.h, sub(p, g.enemy.pos)).abs();
        let exposed = if facing < 0.38 && range < 2.1 && m.shield < to as u64 {
            0.55 * (1. - facing / 0.38)
        } else {
            0.
        };
        if to < usize::from(scn.normal_ready_in) {
            // Keep the opponent in reach, but don't park nose-to-nose while a borrowed
            // slot is still in flight. Once it returns, ordinary attack scoring resumes.
            0.55 * aim + 0.45 * (range - 1.35).abs() + 2.4 * (1.15 - range).max(0.) + exposed
        } else {
            // Favour closing to a playable distance, rather than merely facing a
            // distant opponent. Inside that band leave room for steering and evasion.
            let approach = match m.weapon {
                None | Some(Item::Double) => (range - 1.35).max(0.),
                Some(Item::Shotgun) => 1.1 * (range - 1.45).max(0.),
                Some(Item::Gatling) => 0.55 * (range - 2.2).max(0.),
                _ => 0.,
            };
            // Spend a live shield on a firing position, but price the remaining
            // trip before treating the destination as protected.
            let shield_push = if m.shield > to as u64 + 24
                && g.enemy.shield + 18 < m.shield
                && m.weapon != Some(Item::Mine)
            {
                0.65 * (range - 1.1).max(0.)
            } else {
                0.
            };
            // Only the combat spacing cost changes. No discount on a real
            // projectile, no override of route-to-cover, reload or shot vetoes.
            let read_spacing = if g.tempo > 0. {
                g.tempo * (range - 0.95).max(0.)
            } else {
                -g.tempo * (1.8 * (1.85 - range).max(0.) + 1.5 * exposed)
            };
            1.35 * aim
                + read_spacing
                + shield_push
                + 0.32 * d
                + approach
                + exposed
                + if range < 0.55 && aim > 0.9 { 1.0 } else { 0. }
        }
    } else {
        let rear = if g.task == "mine withdrawal" {
            0.90 * (std::f64::consts::PI - err(n.car.heading, sub(g.enemy.pos, p)).abs())
        } else {
            0.
        };
        1.45 * d + 0.42 * aim * (d / 0.75).min(1.) + rear
    };
    n.score = n.effort + n.danger + pressure;
}
// Replay a complete candidate from the observed pose, using the same traffic,
// shell, wall and objective costs as the search. Costs are independent of chunking.
fn replay(scn: &mut Scene, mut n: Node, plan: &[Act], horizon: usize, log: &mut Thought) -> Node {
    n.first = plan[0];
    let mut at = 0;
    while at < horizon {
        let action = plan.get(at).copied().unwrap_or(IDLE);
        let mut end = at + 1;
        while end < horizon && plan.get(end).copied().unwrap_or(IDLE) == action {
            end += 1;
        }
        advance(scn, &mut n, action, at, end);
        log.nodes += 1;
        at = end;
    }
    n
}
pub(crate) fn choose(
    v: &View,
    g: &Goal,
    motion: &[Point; 8],
    history: &MotionHistory,
    lock: usize,
    fire: bool,
    log: &mut Thought,
) -> (Act, Vec<Act>) {
    let m = v.me;
    let near = v.balls.iter().any(|b| dist(b.pos, m.pos) < 2.2);
    let deep = near || v.balls.iter().any(|b| b.kind == Some(ShotKind::Missile));
    let horizon = if deep {
        180
    } else if !v.balls.is_empty() || fire {
        90
    } else {
        30
    };
    let mut shots = vec![vec![]; horizon + 1];
    let mut threat: Option<(usize, u64)> = None;
    for &b in v.balls.iter().chain(
        (fire
            && m.weapon != Some(Item::Mine)
            && !(m.weapon == Some(Item::Frag)
                && v.balls
                    .iter()
                    .any(|b| b.owner == m.seat.slot && b.kind == Some(ShotKind::Frag))))
        .then(|| ball::outgoing(v))
        .iter(),
    ) {
        ball::trace(v, b, horizon, |t, l| {
            if l.owner != m.seat.slot || l.armed {
                let shot = Swept::new(l);
                if !shot.near(m.pos, travel(m, t)) {
                    return true;
                }
                if threat.is_none_or(|(a, _)| t < a) && ball::touches(l, &m.tank()) {
                    threat = Some((t, l.id));
                }
                shots[t].push(shot);
            }
            true
        });
    }
    log.hazard = threat.map(|x| x.1);
    let traffic = std::array::from_fn(|t| {
        let mut row = Traffic::default();
        // Pose 12 is deliberately held for the remaining horizon, not just tick 12.
        let reach = travel(m, if t == 12 { horizon } else { t }) + 2 * s::TANK_REACH;
        for c in v.cars.iter().filter(|c| c.seat.slot != m.seat.slot) {
            let d = mul(motion[ix(c.seat.slot)], t as f64);
            let p = if d != Point::default() && s::clearance(v.map, add(c.pos, d), c.h) {
                add(c.pos, d)
            } else {
                c.pos
            };
            if (m.pos.x - p.x).abs() <= reach && (m.pos.y - p.y).abs() <= reach {
                row.poses[row.len] = (p, c.h);
                row.len += 1;
            }
        }
        row
    });
    let mut possible = vec![vec![]; 36];
    if v.me.shield < 20 {
        for &c in v.cars.iter().filter(|c| {
            c.seat.team != m.seat.team
                && c.weapon.is_none()
                && dist(c.pos, m.pos) < 2.0
                && err(c.h, sub(m.pos, c.pos)).abs() < 0.62
        }) {
            if v.balls
                .iter()
                .filter(|b| b.owner == c.seat.slot && b.kind.is_none())
                .count()
                >= s::AMMO as usize
            {
                continue;
            }
            let b = Ball {
                id: u64::MAX - 1,
                owner: c.seat.slot,
                pos: add(c.pos, c.h.direction(SIZES.muzzle)),
                v: c.h
                    .direction(SIZES.shell_speed * if c.boost > 0 { 130 } else { 100 } / 100),
                life: s::LIFE,
                kind: None,
                deadly: false,
            };
            // The second scenario pays the real turn time before a rounded discrete
            // bearing can fire. Shooting ourselves does not disable the opposing trigger.
            let angle = err(c.h, sub(m.pos, c.pos));
            let step = if c.boost > 6 {
                Heading::STEP as u64 * 130 / 100
            } else {
                Heading::STEP as u64
            } as u32;
            let ticks = (angle / Heading(step).radians()).round().clamp(-6., 6.) as i32;
            for (delay, h) in [
                (0, c.h),
                (
                    ticks.unsigned_abs().max(1) as usize,
                    Heading(c.h.0.wrapping_add((ticks as i64 * step as i64) as u32)),
                ),
            ] {
                if !s::clearance(v.map, c.pos, h) {
                    continue;
                }
                let mut ray = b;
                ray.pos = add(c.pos, h.direction(SIZES.muzzle));
                ray.v = h.direction(norm(b.v) as i32);
                ball::trace(v, ray, 29, |t, l| {
                    if l.armed {
                        return false;
                    }
                    possible[t + delay].push(l);
                    true
                });
            }
        }
    }
    // An observed expiry is an upper bound on the next free ordinary slot.
    // Score a future pose with the ammo it will have then, not today's empty HUD.
    let normal_ready_in = if m.weapon.is_some() || v.sight.ammo > u8::from(fire) {
        0
    } else {
        v.next_refill()
    };
    let mut scn = Scene {
        moves: Moves::new(v.map),
        normal_ready_in,
        possible,
        v,
        goal: g,
        shots,
        traffic,
        lock: if fire && m.weapon == Some(Item::Laser) {
            s::LASER_LOCK as usize
        } else {
            lock
        },
    };
    let nominal = history.prior.unwrap_or((v.sight.drive, v.sight.turn));
    let previous = &history.tail;
    let base = Node {
        plan: [(Drive::Stop, Turn::None); 180],
        car: m.tank(),
        first: nominal,
        act: nominal,
        danger: 0.,
        score: 0.,
        effort: 0.,
        hit: None,
        hit_id: None,
    };
    let mut beam = vec![base];
    let mut start = 0;
    let mut best = base;
    let stages = if horizon == 30 {
        vec![6, 16, 30]
    } else {
        vec![6, 16, 32, 60, horizon]
    };
    // Only intermediate stages need a ranked beam. At the horizon retain the
    // first minimum (the stable sort's tie-break) without materializing or
    // sorting dozens of complete plans just to discard them.
    let mut next = Vec::with_capacity(if deep { 45 } else { 27 });
    for end in stages {
        let mut terminal: Option<Node> = None;
        for n in &beam {
            for a in ACTIONS {
                if start >= 32
                    && a != n.act
                    && a != (Drive::Stop, Turn::None)
                    && a != (n.act.0, Turn::None)
                {
                    continue;
                }
                let mut c = *n;
                if start == 0 {
                    c.first = a;
                }
                advance(&mut scn, &mut c, a, start, end);
                log.nodes += 1;
                if end != horizon {
                    next.push(c);
                } else if terminal
                    .as_ref()
                    .is_none_or(|n| c.score.total_cmp(&n.score).is_lt())
                {
                    terminal = Some(c);
                }
            }
        }
        if let Some(winner) = terminal {
            best = winner;
            break;
        }
        next.sort_by(|a, b| a.score.total_cmp(&b.score));
        best = next[0];
        beam.clear();
        for c in next.drain(..) {
            if !beam.iter().any(|n| {
                n.first == c.first
                    && dist(n.car.center, c.car.center) < 0.08
                    && err(n.car.heading, c.car.heading.direction(CELL)).abs() < 0.2
            }) {
                beam.push(c);
            }
            if beam.len() == if deep { 5 } else { 3 } {
                break;
            }
        }
        start = end;
    }
    // A short beam can prune the only escape BEFORE the returning round arrives.
    // Re-open all first actions on a predicted loss, using independently completed tails.
    if best.hit.is_some() {
        for a in ACTIONS {
            for bend in [5, 14, 30] {
                let mut stem = base;
                stem.first = a;
                advance(&mut scn, &mut stem, a, 0, bend);
                if stem.hit.is_some() {
                    continue;
                }
                for tail in [
                    a,
                    (a.0, Turn::None),
                    (Drive::Forward, Turn::None),
                    (Drive::Reverse, Turn::None),
                    (Drive::Stop, Turn::None),
                ] {
                    let mut n = stem;
                    advance(&mut scn, &mut n, tail, bend, horizon);
                    log.nodes += 1;
                    if n.score < best.score {
                        best = n;
                    }
                }
            }
        }
    }
    // The first step of a freshly solved plan can look safe only because evasive
    // steering is deferred every frame. Revalidate the shifted incumbent from NOW.
    if !previous.is_empty() && threat.is_some_and(|(at, _)| at <= 90) {
        let n = replay(&mut scn, base, previous, horizon, log);
        if n.hit.is_none() && n.score <= best.score + 0.35 {
            best = n;
        }
    }
    let mut prefix_replaced = false;
    // Empty corridor: don't idle merely to arrive facing an intermediate tile centre
    // on the last tick of the search. Validate the steering prefix against real geometry.
    if !g.combat && v.balls.is_empty() && !fire && dist(m.pos, g.point) > 0.15 {
        let angle = err(m.h, sub(g.point, m.pos));
        let steer = if angle.abs() < 0.055 {
            Turn::None
        } else if angle > 0. {
            Turn::Right
        } else {
            Turn::Left
        };
        let drive = if angle.abs() < 0.8 {
            Drive::Forward
        } else {
            Drive::Stop
        };
        let a = (drive, steer);
        let mut n = base;
        advance(&mut scn, &mut n, a, 0, 6);
        log.nodes += 1;
        if (drive == Drive::Forward && dist(n.car.center, g.point) + 0.035 < dist(m.pos, g.point))
            || (steer != Turn::None
                && err(n.car.heading, sub(g.point, n.car.center)).abs() + 0.1 < angle.abs())
        {
            best.first = a;
            best.plan[..6].fill(a);
            prefix_replaced = true;
        }
    }
    // Do not hold input: the unsmoothed escape remains available on this very tick.
    if !fire && lock == 0 && best.hit.is_none() && threat.is_none_or(|(at, _)| at > 36) {
        let settled = history.settle(m.pos, best.first);
        if settled != best.first
            && scn.shots[1..=12].iter().all(Vec::is_empty)
            && !scn.possible[1..=12]
                .iter()
                .flatten()
                .any(|leg| ball::touches(*leg, &m.tank()))
        {
            // In a clear corridor the geometry-validated prefix above may have
            // replaced the beam winner's first six steps without rescoring its tail.
            if prefix_replaced {
                best = replay(&mut scn, base, &best.plan, horizon, log);
            }
            let mut plan = best.plan;
            plan[0] = settled;
            let n = replay(&mut scn, base, &plan, horizon, log);
            if n.hit.is_none() && n.score <= best.score + 0.4 {
                best = n;
            }
        }
    }
    log.hit_shell = best.hit_id;
    log.first_hit = best.hit;
    log.depth = horizon;
    (best.first, best.plan[1..horizon].to_vec())
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
