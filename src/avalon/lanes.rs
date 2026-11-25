//! Marginal value of one ordinary arena round. Compare the same legal responses
//! with and without it; a second round on an already lost route is not another hit.
use super::{ball, fire::Order, planner::Act, *};

const BRANCHES: usize = 5; // brake, drive, reverse, bend left, bend right
const NEVER: u8 = u8::MAX;
type Times = [u8; BRANCHES];

fn step(v: &View, car: &mut Tank, act: Act, boosted: bool) {
    (car.drive, car.turn) = act;
    let pose = car.proposed_pose(v.map, boosted);
    if !v
        .cars
        .iter()
        .any(|c| c.seat.slot != car.seat.slot && s::overlap(pose, (c.pos, c.h)))
    {
        (car.center, car.heading) = pose;
    }
}

pub(crate) struct Screen {
    from: Point,
    approach: [Point; 24],
    intercept: usize,
}
impl Screen {
    pub(super) fn used_by(&self, v: &View, first: Act, tail: &[Act]) -> bool {
        let mut car = v.me.tank();
        // A moving tank alone isn't evidence that a shot bought space. In the
        // no-shot response, the two paths must contest the same patch at the
        // same time, *after* the candidate could have interrupted the approach.
        for (i, act) in std::iter::once(first)
            .chain(tail.iter().copied())
            .take(self.approach.len())
            .enumerate()
        {
            step(v, &mut car, act, v.me.boost > i as u64);
            let gap = dist(self.approach[i], car.center);
            if i + 1 >= self.intercept
                && dist(car.center, v.me.pos) >= 0.18
                && gap <= 0.72
                && gap + 0.25 < dist(self.from, car.center)
            {
                return true;
            }
        }
        false
    }
}

struct Lanes {
    old: Times,
    new: Times,
    paths: Vec<[Tank; BRANCHES]>,
    unsafe_line: bool,
}
impl Lanes {
    fn added(&self, i: usize) -> bool {
        self.new[i] != NEVER
            && (self.old[i] == NEVER || self.new[i].saturating_add(5) < self.old[i])
    }
    // A complementary route must be physically distinct, not a second label for
    // the same tank stopped against a wall. Compare positions at the same time.
    fn splits(&self) -> bool {
        (0..BRANCHES).any(|i| {
            self.old[i] == NEVER
                && self.added(i)
                && (0..BRANCHES).any(|j| {
                    let t = usize::from(self.new[i]);
                    self.old[j] != NEVER
                        && !self.added(j)
                        && dist(self.paths[t][i].center, self.paths[t][j].center) >= 0.30
                })
        })
    }
}

fn compare(v: &View, motion: &[Point; 8], enemy: Car, candidate: Ball, limit: usize) -> Lanes {
    let mut row = [enemy.tank(); BRANCHES];
    let mut paths = Vec::with_capacity(limit + 1);
    paths.push(row);
    let vel = motion[ix(enemy.seat.slot)];
    let reverse = norm(vel) > 1. && err(enemy.h, vel).abs() > std::f64::consts::FRAC_PI_2;
    let coast = if reverse {
        Drive::Reverse
    } else {
        Drive::Forward
    };
    for t in 1..=limit {
        if t <= 32 {
            for (tank, (drive, turn)) in row[1..].iter_mut().zip([
                (Drive::Forward, Turn::None),
                (Drive::Reverse, Turn::None),
                (coast, Turn::Left),
                (coast, Turn::Right),
            ]) {
                step(
                    v,
                    tank,
                    (drive, if t <= 7 { turn } else { Turn::None }),
                    enemy.boost >= t as u64,
                );
            }
        }
        paths.push(row);
    }
    let contacts = |b: Ball| {
        let mut hits = [NEVER; BRANCHES];
        let mut unsafe_line = false;
        ball::trace(v, b, limit, |t, l| {
            // Stop this round at the first other body on each still-live branch.
            let block = v
                .cars
                .iter()
                .filter(|c| c.seat.slot != enemy.seat.slot && (c.seat.slot != b.owner || l.armed))
                .filter_map(|c| {
                    s::body_hit(l.p, l.d, &c.tank(), l.r).map(|d| (norm(d), c.seat.team))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            for (i, h) in hits.iter_mut().enumerate().filter(|(_, h)| **h == NEVER) {
                if let Some(d) = s::body_hit(l.p, l.d, &paths[t][i], l.r)
                    && enemy.shield < t as u64
                    && block.is_none_or(|(at, _)| norm(d) < at)
                {
                    *h = t as u8;
                } else if block.is_some_and(|(_, team)| team == v.me.seat.team) {
                    unsafe_line = true;
                }
            }
            block.is_none() && hits.contains(&NEVER)
        });
        (hits, unsafe_line)
    };
    let (new, unsafe_line) = contacts(candidate);
    let mut old = [NEVER; BRANCHES];
    if new.iter().any(|&t| t != NEVER) && !unsafe_line {
        for &b in v
            .balls
            .iter()
            .filter(|b| b.owner == v.me.seat.slot && b.kind.is_none())
        {
            // No ricochet can travel farther than an unbroken path of equal duration.
            if norm(sub(b.pos, enemy.pos))
                > (norm(b.v) + (s::FORWARD_SPEED * 13 / 10) as f64) * limit as f64 + CELL as f64
            {
                continue;
            }
            for (prior, hit) in old.iter_mut().zip(contacts(b).0) {
                *prior = (*prior).min(hit);
            }
        }
    }
    Lanes {
        old,
        new,
        paths,
        unsafe_line,
    }
}

pub(super) fn follow_up(v: &View, motion: &[Point; 8], enemy: Car, b: Ball, eta: usize) -> Order {
    let lanes = compare(v, motion, enemy, b, (eta + 9).clamp(30, 90));
    if lanes.unsafe_line {
        return (false, "blocked follow-up").into();
    }
    let close = lanes.new.iter().filter(|&&t| t <= 12).count() >= 3
        && (0..BRANCHES).any(|i| lanes.added(i) && lanes.new[i] <= 12);
    if close {
        return (true, "close conversion").into();
    }
    if lanes.splits() {
        return (true, "cut another exit").into();
    }
    if !(0..BRANCHES).any(|i| lanes.added(i))
        || (lanes.old.iter().any(|&t| t != NEVER)
            && (0..BRANCHES).filter(|&i| lanes.added(i)).count() == 1)
    {
        return (false, "already covered or unconfirmed").into();
    }
    (true, "new intercept").into()
}

pub(super) fn positional(v: &View, motion: &[Point; 8], b: Ball) -> Option<Order> {
    let enemy = *v
        .cars
        .iter()
        .filter(|c| c.seat.team != v.me.seat.team && c.shield == 0)
        .min_by(|a, b| dist(a.pos, v.me.pos).total_cmp(&dist(b.pos, v.me.pos)))?;
    let range = dist(enemy.pos, v.me.pos);
    if range >= 2. || err(v.me.h, sub(enemy.pos, v.me.pos)).abs() > 0.7 {
        return None;
    }
    let velocity = motion[ix(enemy.seat.slot)];
    let approaching = dist(add(enemy.pos, mul(velocity, 8.)), v.me.pos) + 0.07 < range;
    if !approaching
        && !v
            .balls
            .iter()
            .any(|p| p.owner == v.me.seat.slot && p.kind.is_none())
    {
        return None; // No ongoing approach to screen and no prior shot to complement.
    }
    let lanes = compare(v, motion, enemy, b, 32);
    if lanes.unsafe_line {
        return None;
    }
    if lanes.splits() && (0..BRANCHES).filter(|&i| lanes.added(i)).count() >= 2 {
        return Some((true, "cut another exit").into());
    }
    // Only buy a screen against an observed approach. If the selected movement
    // later fails to use the room, the caller replans with the trigger released.
    if !approaching {
        return None;
    }
    let i = (1..BRANCHES)
        .filter(|&i| lanes.added(i) && lanes.new[i] <= 24)
        .min_by(|&i, &j| {
            dist(lanes.paths[24][i].center, v.me.pos)
                .total_cmp(&dist(lanes.paths[24][j].center, v.me.pos))
        })?;
    let approach = std::array::from_fn(|t| lanes.paths[t + 1][i].center);
    (dist(approach[23], v.me.pos) + 0.22 < range).then(|| Order {
        press: true,
        reason: "screen reposition",
        screen: Some(Box::new(Screen {
            from: enemy.pos,
            approach,
            intercept: usize::from(lanes.new[i]),
        })),
    })
}
