use super::ball::{outgoing, trace};
use super::*;
pub(crate) struct Order {
    pub press: bool,
    pub reason: &'static str,
    pub screen: Option<Box<lanes::Screen>>,
}
impl From<(bool, &'static str)> for Order {
    fn from((press, reason): (bool, &'static str)) -> Self {
        Self {
            press,
            reason,
            screen: None,
        }
    }
}
pub(crate) fn trigger(
    v: &View,
    motion: &[Point; 8],
    last: Option<u64>,
    ball_ticks: &mut usize,
) -> Order {
    let m = v.me;
    let own = m.seat.slot;
    let g = m.weapon;
    let nearest = v
        .cars
        .iter()
        .filter(|c| c.seat.team != m.seat.team)
        .map(|c| dist(c.pos, m.pos))
        .fold(f64::INFINITY, f64::min);
    if !v.sight.can_fire {
        return (false, "release/reload").into();
    }
    if g == Some(Item::Mine) {
        let chase = v.cars.iter().any(|c| {
            c.seat.team != m.seat.team
                && dist(c.pos, m.pos) < 2.
                && err(m.h, sub(c.pos, m.pos)).abs() > 1.9
                && s::wall(v.map, m.pos, sub(c.pos, m.pos), 0).is_none()
        });
        let safe = !v
            .cars
            .iter()
            .any(|c| c.seat.team == m.seat.team && c.seat.slot != own && dist(c.pos, m.pos) < 1.2);
        // A third deployed mine starts the oldest mine's fuse. Do not trigger it
        // beside an ally or ourselves. The exact enemy inputs/fuses are unknown.
        let mines = v
            .balls
            .iter()
            .filter(|b| b.owner == own && b.kind == Some(ShotKind::Mine));
        let replacement_safe = mines.clone().count() < 2
            || mines.min_by_key(|b| b.id).is_none_or(|b| {
                v.cars.iter().all(|c| {
                    c.seat.team != m.seat.team
                        || dist(c.pos, b.pos) > 1.35
                        || s::wall(v.map, c.pos, sub(b.pos, c.pos), 0).is_some()
                })
            });
        let free_rear = s::wall(
            v.map,
            m.pos,
            m.h.direction(-45 * PX),
            ShotKind::Mine.radius(),
        )
        .is_none();
        return (
            chase
                && safe
                && free_rear
                && replacement_safe
                && last.is_none_or(|t| v.tick.saturating_sub(t) > 45),
            "rear trap",
        )
            .into();
    }
    if g == Some(Item::Frag)
        && let Some(b) = v
            .balls
            .iter()
            .find(|b| b.owner == own && b.kind == Some(ShotKind::Frag))
    {
        let mut end = b.pos;
        trace(v, *b, s::FUSE as usize, |_, l| {
            end = add(l.p, l.d);
            true
        });
        // The button starts the fuse; it does not explode at today's poses.
        // Keep allies safe in both the brake and coasting responses.
        let clear = |p: Point| dist(p, end) < 1.35 && s::wall(v.map, p, sub(end, p), 0).is_none();
        let coast = |c: &Car| {
            let dp = mul(motion[ix(c.seat.slot)], s::FUSE as f64);
            let p = add(c.pos, dp);
            if s::wall(v.map, c.pos, dp, SIZES.half_width).is_none() && s::clearance(v.map, p, c.h)
            {
                p
            } else {
                c.pos
            }
        };
        return (
            v.cars.iter().any(|c| {
                c.seat.team != m.seat.team && c.shield < u64::from(s::FUSE) && clear(coast(c))
            }) && !v
                .cars
                .iter()
                .any(|c| c.seat.team == m.seat.team && (clear(c.pos) || clear(coast(c)))),
            "remote fuse",
        )
            .into();
    }
    let b = outgoing(v);
    let mut votes = [0u8; 8];
    let mut arrivals = [[usize::MAX; 3]; 8];
    let mut veto = false;
    // A full rack can afford one opening round down a clear lane. Track its
    // actual first contact (including allies) instead of firing at a sight flag.
    let opening = g.is_none() && v.sight.ammo == s::AMMO && v.sight.enemy_ahead;
    let horizon = if g == Some(Item::Laser) {
        48
    } else if opening {
        150
    } else {
        94
    };
    // Three distinct responses: coast, brake now, brake after a short commitment. Only
    // credit the FIRST body on each trajectory. A friendly first contact vetoes the shot.
    let mut done = [false; 3];
    trace(v, b, horizon, |tick, l| {
        *ball_ticks = (*ball_ticks).max(tick);
        // The response only changes tank centres, never this ray. Bounce legs
        // are still visited in order, and each model stops at its own first hit.
        let end = add(l.p, l.d);
        let reach = s::TANK_REACH + l.r;
        let near = |p: Point| {
            p.x >= l.p.x.min(end.x) - reach
                && p.x <= l.p.x.max(end.x) + reach
                && p.y >= l.p.y.min(end.y) - reach
                && p.y <= l.p.y.max(end.y) + reach
        };
        for (model, coast) in [0, 9, 32].into_iter().enumerate() {
            if done[model] {
                continue;
            }
            let mut first: Option<(f64, Car)> = None;
            for c in v.cars {
                if c.seat.slot == own && !l.armed {
                    continue;
                }
                let dp = mul(motion[ix(c.seat.slot)], tick.min(coast) as f64);
                let projected = add(c.pos, dp);
                // The old coast model selects exactly one of these two poses.
                // No need to cast the coast against walls if neither is in reach.
                if !near(c.pos) && !near(projected) {
                    continue;
                }
                let mut t = c.tank();
                if dp != Point::default() && s::wall(v.map, c.pos, dp, SIZES.half_width).is_none() {
                    t.center = projected;
                }
                if let Some(h) = s::body_hit(l.p, l.d, &t, l.r) {
                    let d = norm(h);
                    if first.is_none_or(|(n, _)| d < n) {
                        first = Some((d, *c));
                    }
                }
            }
            if let Some((_, c)) = first {
                done[model] = true;
                if c.seat.team == m.seat.team || c.shield >= tick as u64 {
                    veto = true;
                } else {
                    votes[ix(c.seat.slot)] += 1;
                    arrivals[ix(c.seat.slot)][model] = tick;
                }
            }
        }
        !done.iter().all(|&hit| hit)
    });
    if veto {
        return (false, "friendly/shield line").into();
    }
    // Price the time to hit the SAME opponent across supported responses. One fast
    // optimistic branch is not evidence that all the other interceptions arrive soon.
    let target = (0..8)
        .max_by_key(|&i| {
            (
                votes[i],
                std::cmp::Reverse(*arrivals[i].iter().min().unwrap()),
            )
        })
        .unwrap();
    let support = votes[target];
    let mut hits = arrivals[target];
    hits.sort_unstable();
    let earliest = if g.is_none() && v.sight.ammo == 1 && support == 3 {
        hits[2]
    } else {
        hits[0]
    };
    let scarce = g.is_none() && v.sight.ammo <= 2;
    let quality = support >= 2 && earliest <= if scarce { 35 } else { 75 }
        || support >= 1 && earliest <= 17
        || opening && support == 3 && hits[2] <= horizon;
    let tracking = g == Some(Item::Gatling)
        && v.cars.iter().any(|c| {
            c.seat.team != m.seat.team
                && c.shield < 12
                && dist(c.pos, m.pos) < 2.8
                && err(m.h, sub(c.pos, m.pos)).abs() < 0.27
                && s::wall(v.map, m.pos, sub(c.pos, m.pos), 0).is_none()
        });
    if (quality || tracking)
        && matches!(g, Some(Item::Shotgun | Item::Gatling))
        && v.cars
            .iter()
            .any(|c| c.seat.team == m.seat.team && c.seat.slot != own)
    {
        let width = if g == Some(Item::Shotgun) { 0.15 } else { 0.03 };
        for frac in [-1., -0.5, 0.5, 1.] {
            let spread =
                Heading(m.h.0.wrapping_add(
                    (frac * width * 4294967296. / std::f64::consts::TAU) as i64 as u32,
                ));
            let mut edge = b;
            edge.v = spread.direction(norm(b.v) as i32);
            let mut unsafe_lane = false;
            trace(v, edge, earliest.min(40), |_, l| {
                unsafe_lane = v.cars.iter().any(|c| {
                    c.seat.team == m.seat.team
                        && c.seat.slot != own
                        && super::ball::touches(l, &c.tank())
                });
                !unsafe_lane
            });
            if unsafe_lane {
                return (false, "ally in spread").into();
            }
        }
    }
    if g == Some(Item::Gatling) {
        return (
            quality || tracking,
            if quality || tracking {
                "sustain gatling"
            } else {
                "no gatling lane"
            },
        )
            .into();
    }
    if !quality {
        if g == Some(Item::Missile)
            && v.tick.is_multiple_of(12)
            && (3.0..5.5).contains(&nearest)
            && seeker_lane(v, motion, b, ball_ticks)
        {
            return (true, "seeker acquisition").into();
        }
        if g.is_none()
            && v.sight.ammo >= 4
            && nearest < 2.0
            && last.is_none_or(|t| v.tick.saturating_sub(t) >= 24)
            && let Some(order) = lanes::positional(v, motion, b)
        {
            return order;
        }
        return (false, "no robust line").into();
    }
    // A round about to expire will release its slot even on a miss. Don't keep
    // the last button press in reserve as though all five slots were newly spent.
    let next_refill = v.next_refill();
    let refill_soon = next_refill <= 22;
    // A single optimistic reaction is not a close-range conversion. If missing
    // means several seconds unarmed, wait for two fast contacts or a point-blank one.
    if g.is_none()
        && v.sight.ammo == 1
        && next_refill > 2 * s::HZ as u16
        && hits[0] > 6
        && hits[1] > 13
    {
        return (false, "keep a close reply").into();
    }
    // With several ordinary rounds already out and no refill due, do not
    // spend another slow slot on a distant, speculative conversion. Keep
    // advancing to shorten the intercept instead of draining the magazine.
    if g.is_none() && v.sight.ammo < s::AMMO && !refill_soon {
        let budget = usize::from(v.sight.ammo) * 15;
        if hits[1] > budget && earliest > 17 {
            return (false, "close before follow-up").into();
        }
    }
    if v.sight.ammo == 1 && g.is_none() && !refill_soon && nearest > 1.8 {
        return (false, "last slot reserve").into();
    }
    // A scarce shot can occupy a slot for the entire ten-second lifetime. Model the
    // miss branch, including late wall returns; do not plan enemy movement ten seconds out.
    if g.is_none() && v.sight.ammo == 1 && !refill_soon && earliest > 13 {
        let mut returns = false;
        let mut life = 0;
        trace(v, b, b.life as usize, |tick, l| {
            life = tick;
            *ball_ticks = (*ball_ticks).max(tick);
            if tick > earliest && super::ball::touches(l, &m.tank()) {
                returns = true;
            }
            true
        });
        if support < 3 || earliest > 22 || (returns && next_refill > 60 && life > 90) {
            return (false, "wait for close conversion").into();
        }
    }
    let delay = if earliest <= 14 {
        2
    } else if scarce {
        24
    } else {
        11
    };
    if last.is_some_and(|t| v.tick.saturating_sub(t) < delay) {
        return (false, "cadence").into();
    }
    if g.is_none()
        && last.is_some_and(|t| v.tick.saturating_sub(t) <= 24)
        && let Some(enemy) = v.cars.iter().find(|c| ix(c.seat.slot) == target)
        && v.balls.iter().any(|p| p.owner == own && p.kind.is_none())
    {
        return lanes::follow_up(v, motion, *enemy, b, earliest);
    }
    (true, "intercept").into()
}

// A homing missile only acquires after two seconds. The ordinary 94-tick
// intercept test cannot see that phase. Require a first hostile hull on both
// brake and short-coast responses, vetoing own/ally/shield interception.
fn seeker_lane(v: &View, motion: &[Point; 8], b: Ball, ball_ticks: &mut usize) -> bool {
    let mut hit = [None; 2];
    let mut blocked = false;
    trace(v, b, s::SEEK_DELAY as usize + 48, |t, l| {
        *ball_ticks = (*ball_ticks).max(t);
        for (i, h) in hit.iter_mut().enumerate().filter(|(_, h)| h.is_none()) {
            let mut first = None;
            for c in v.cars.iter().filter(|c| c.seat.slot != b.owner || l.armed) {
                let mut car = c.tank();
                let dp = mul(motion[ix(c.seat.slot)], t.min(9 * i) as f64);
                if dp != Point::default()
                    && s::wall(v.map, c.pos, dp, SIZES.half_width).is_none()
                    && s::clearance(v.map, add(c.pos, dp), c.h)
                {
                    car.center = add(c.pos, dp);
                }
                if let Some(d) = s::body_hit(l.p, l.d, &car, l.r)
                    && first.is_none_or(|(x, _)| norm(d) < x)
                {
                    first = Some((norm(d), c));
                }
            }
            if let Some((_, c)) = first {
                if c.seat.team == v.me.seat.team
                    || c.shield >= t as u64
                    || t < usize::from(s::SEEK_DELAY)
                {
                    blocked = true;
                    break;
                }
                *h = Some(c.seat.slot);
            }
        }
        !blocked && hit.iter().any(Option::is_none)
    });
    !blocked && hit[0].is_some() && hit[0] == hit[1]
}
