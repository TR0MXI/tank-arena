use super::*;
#[derive(Clone, Copy)]
pub(crate) struct Leg {
    pub id: u64,
    pub owner: SlotId,
    pub p: Point,
    pub d: Point,
    pub r: i32,
    pub armed: bool,
}
// Integrate one observed shot once; reuse its time slices across all movement branches.
// No knowledge of unseen controls, future pickups, spread RNG or a remote fuse.
pub(crate) fn trace(view: &View, b: Ball, limit: usize, mut emit: impl FnMut(usize, Leg) -> bool) {
    let (mut p, mut vel, mut armed) = (b.pos, b.v, b.deadly);
    let r = b.kind.map_or(SIZES.shell_radius, ShotKind::radius);
    let end = limit.min(b.life as usize);
    let mut expires_at = end;
    let mut recent = [-100i32; 5];
    let mut bounce = 0;
    for t in 1..=end {
        if t > expires_at {
            break;
        }
        if b.kind == Some(ShotKind::Missile) {
            let age = s::MISSILE_LIFE.saturating_sub(b.life) as usize + t;
            if age >= s::SEEK_DELAY as usize {
                armed = true;
                if (age - s::SEEK_DELAY as usize).is_multiple_of(6)
                    && let Some((goal, _)) = super::route::seeker(view, p, b.owner)
                {
                    let a = (vel.y as f64).atan2(vel.x as f64);
                    let g = sub(goal, p);
                    let to = (g.y as f64).atan2(g.x as f64);
                    let diff = (to - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                        - std::f64::consts::PI;
                    let (sn, cs) = (a + diff.clamp(-0.5, 0.5)).sin_cos();
                    vel = Point {
                        x: (cs * SIZES.shell_speed as f64).round() as i32,
                        y: (sn * SIZES.shell_speed as f64).round() as i32,
                    };
                }
            }
        }
        let mut d = vel;
        for _ in 0..5 {
            let wall = if d == Point::default() {
                None
            } else {
                s::reflect_surface(
                    view.map,
                    p,
                    d,
                    vel,
                    r,
                    view.cars
                        .iter()
                        .filter(|c| {
                            b.kind != Some(ShotKind::Mine)
                                && c.shield >= t as u64
                                && (c.seat.slot != b.owner || armed)
                        })
                        .map(|c| c.pos),
                )
            };
            let used = wall.map_or(d, |w| w.0);
            if !emit(
                t,
                Leg {
                    id: b.id,
                    owner: b.owner,
                    p,
                    d: used,
                    r,
                    armed,
                },
            ) {
                return;
            }
            p = add(p, used);
            let Some((_, reflected, remaining)) = wall else {
                break;
            };
            if b.kind == Some(ShotKind::Shard) {
                return;
            }
            if b.kind == Some(ShotKind::Mine) {
                vel = Point::default();
                break;
            }
            armed = true;
            if b.kind == Some(ShotKind::Pellet) {
                expires_at = expires_at.min(t + 41);
            }
            if t as i32 - recent[bounce % 5] <= 2 {
                return;
            }
            recent[bounce % 5] = t as i32;
            bounce += 1;
            vel = reflected;
            d = remaining;
            if d == Point::default() {
                break;
            }
        }
        if b.kind == Some(ShotKind::Mine) {
            let speed = norm(vel);
            if speed > 0. {
                vel = mul(
                    vel,
                    (1. - (800 * PX / (s::HZ * s::HZ) as i32) as f64 / speed).max(0.),
                )
            }
        }
    }
}
pub(crate) fn touches(l: Leg, t: &Tank) -> bool {
    (l.owner != t.seat.slot || l.armed) && s::body_hit(l.p, l.d, t, l.r).is_some()
}
pub(crate) fn outgoing(v: &View) -> Ball {
    let m = v.me;
    let w = m.weapon;
    let (k, o, sp) = match w {
        Some(Item::Laser) => (Some(ShotKind::Laser), 50, 3600),
        Some(Item::Double) => (Some(ShotKind::Double), 45, 360),
        Some(Item::Shotgun) => (Some(ShotKind::Pellet), 49, 650),
        Some(Item::Gatling) => (Some(ShotKind::Gatling), 60, 550),
        Some(Item::Missile) => (Some(ShotKind::Missile), 50, 360),
        Some(Item::Frag) => (Some(ShotKind::Frag), 45, 360),
        _ => (None, 50, 360),
    };
    let mut p = add(m.pos, m.h.direction(o * PX));
    if w == Some(Item::Double) {
        p = add(
            p,
            Heading(m.h.0.wrapping_add(1 << 30)).direction(if v.sight.ammo.is_multiple_of(2) {
                -9 * PX
            } else {
                9 * PX
            }),
        );
    }
    let speed = sp * PX / s::HZ as i32
        * if m.boost > 0 && (w.is_none() || w == Some(Item::Double)) {
            130
        } else {
            100
        }
        / 100;
    Ball {
        id: u64::MAX,
        owner: m.seat.slot,
        pos: p,
        v: m.h.direction(speed),
        life: match w {
            Some(Item::Laser) => s::LASER_LIFE,
            Some(Item::Shotgun | Item::Gatling) => 120,
            Some(Item::Frag) => s::FRAG_LIFE,
            Some(Item::Double) => s::DOUBLE_LIFE,
            _ => s::LIFE,
        },
        kind: k,
        deadly: false,
    }
}
