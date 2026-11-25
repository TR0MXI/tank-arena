//! Per-seat, per-match reads. Only completed, observable opportunities move the prior.
use super::*;

#[derive(Clone, Copy, Default)]
struct Evidence {
    mass: f64,
    total: f64,
    at: u64,
    samples: u32,
}
impl Evidence {
    fn remaining(self, t: u64) -> f64 {
        // Half weight after 15 seconds without corroboration.
        (-(t.saturating_sub(self.at) as f64) / 900.).exp2()
    }
    fn add(&mut self, t: u64, value: f64) {
        let keep = self.remaining(t).min(6. / self.mass.max(1.));
        self.total = self.total * keep + value;
        self.mass = self.mass * keep + 1.;
        self.at = t;
        self.samples = self.samples.saturating_add(1);
    }
    fn estimate(self, t: u64) -> f64 {
        let keep = self.remaining(t);
        if self.samples < 2 || self.mass * keep < 1.35 {
            return 0.;
        }
        self.total * keep / (2. + self.mass * keep)
    }
}
#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct Read {
    pub evasion: f64,
    pub shooting: f64,
    pub consecutive_laps: u8,
    pub exposure: f64,
    pub exposed_windows: u32,
    pub dodge_samples: u32,
    pub shot_samples: u32,
    pub last_response_ticks: Option<u64>,
    pub late_windows: u32,
}
impl Read {
    pub(super) fn weakness(self) -> f64 {
        (-self.evasion)
            .max(-self.shooting)
            .max(-self.exposure)
            .max(0.)
    }
    pub(super) fn tempo(self) -> f64 {
        // Good fire discipline still deserves respect even from an easy-to-hit seat.
        let respect = self.shooting.max(self.evasion).max(0.);
        (self.weakness() - respect * 1.15).clamp(-1., 1.)
    }
}
#[derive(Clone, Copy)]
struct Trial {
    shot: Ball,
    start: u64,
    eta: u64,
    car: Car,
    motion: Point,
    cleared: Option<u64>,
}
// A predictable turret phase is a temporary opening, not a permanent skill
// penalty. Count complete revolutions, not one independent sample per frame.
#[derive(Default)]
struct Orbit {
    sign: i32,
    anchor: Point,
    phase: u64,
    laps: u8,
}
impl Orbit {
    fn advance(&mut self, e: Car, previous: Option<Heading>) {
        let d = previous.map_or(0, |h| e.h.0.wrapping_sub(h.0) as i32);
        if d == 0 || self.sign != d.signum() || dist(self.anchor, e.pos) > 1.2 {
            *self = Self {
                sign: d.signum(),
                anchor: e.pos,
                ..Self::default()
            };
            return;
        }
        self.phase += u64::from(d.unsigned_abs());
        if self.phase >= 1u64 << 32 {
            self.laps = if dist(self.anchor, e.pos) < 0.52 {
                self.laps.saturating_add(1)
            } else {
                0
            };
            self.phase = 0;
            self.anchor = e.pos;
        }
    }
}
#[derive(Default)]
struct SeatRead {
    orbit: Orbit,
    dodge: Evidence,
    shot: Evidence,
    pending: Option<Trial>,
    opening: Option<(u64, Car)>,
    last_open: Option<u64>,
    exposure: Evidence,
    // At most one new trial every 36 ticks; a normal round lives at most 600.
    seen: [Option<u64>; 18],
    cursor: usize,
    next: u64,
    prev: Option<(Heading, Point)>,
    response: Option<u64>,
    late: u32,
}
#[derive(Default)]
pub(super) struct Book([SeatRead; 8]);

// First contact time on a straight relative-motion segment (before any bounce).
fn eta(b: Ball, c: Car, motion: Point, ticks: u64) -> Option<f64> {
    let rel = sub(b.v, motion);
    let d = s::body_hit(b.pos, mul(rel, ticks as f64), &c.tank(), SIZES.shell_radius)?;
    Some(norm(d) / norm(rel).max(1.))
}
fn uncluttered(v: &View, e: Car, b: Ball) -> bool {
    e.shield == 0
        && e.weapon != Some(Item::Laser)
        && !v.balls.iter().any(|o| {
            o.id != b.id && (o.owner != e.seat.slot || o.deadly) && dist(e.pos, o.pos) < 1.0
        })
}
fn escape_exists(v: &View, e: Car, b: Ball, ticks: u64) -> bool {
    // A missed reaction is only interesting if at least one legal control could
    // have cleared this round. Check the full prefix, including wall/body blocks.
    for turn in [Turn::None, Turn::Left, Turn::Right] {
        for drive in [Drive::Forward, Drive::Reverse, Drive::Stop] {
            let mut car = e.tank();
            (car.turn, car.drive) = (turn, drive);
            let mut pos = b.pos;
            let mut escaped = true;
            for t in 1..=ticks {
                let pose = car.proposed_pose(v.map, e.boost >= t);
                if !v
                    .cars
                    .iter()
                    .any(|o| o.seat != e.seat && s::overlap(pose, (o.pos, o.h)))
                {
                    (car.center, car.heading) = pose;
                }
                if s::body_hit(pos, b.v, &car, SIZES.shell_radius).is_some() {
                    escaped = false;
                    break;
                }
                pos = add(pos, b.v);
            }
            if escaped
                && dist(pos, car.center) > 0.34
                && s::body_hit(
                    b.pos,
                    mul(b.v, (ticks + 70) as f64),
                    &car,
                    SIZES.shell_radius,
                )
                .is_none_or(|d| s::wall(v.map, b.pos, d, SIZES.shell_radius).is_some())
            {
                return true;
            }
        }
    }
    false
}
impl SeatRead {
    // A positional error is not inferred from silence. Look for an actually
    // playable, uncontested direct shot, repeatedly left open without counter-aim
    // or meaningful displacement, *despite* a legal way off the line.
    fn observe_opening(&mut self, v: &View, e: Car, continuous: bool) {
        if !continuous {
            self.opening = None;
            self.last_open = Some(v.tick);
            return;
        }
        let m = v.me;
        let d = dist(m.pos, e.pos);
        if !(0.95..2.85).contains(&d)
            || m.weapon.is_some()
            || e.weapon.is_some()
            || m.shield > 0
            || e.shield > 0
            || v.sight.ammo == 0
            || !v.sight.enemy_ahead
            || err(e.h, sub(m.pos, e.pos)).abs() < 0.55
        {
            self.opening = None;
            return;
        }
        let b = ball::outgoing(v);
        let Some(t) = eta(b, e, Point::default(), 90) else {
            self.opening = None;
            return;
        };
        let lane = mul(b.v, t + 0.1);
        if s::wall(v.map, b.pos, lane, SIZES.shell_radius).is_some()
            || v.cars.iter().any(|o| {
                o.seat != e.seat
                    && o.seat != m.seat
                    && (s::body_hit(b.pos, lane, &o.tank(), SIZES.shell_radius).is_some()
                        || o.seat.team != e.seat.team
                            && dist(o.pos, e.pos) < 3.
                            && s::wall(v.map, o.pos, sub(e.pos, o.pos), SIZES.shell_radius)
                                .is_none())
            })
            || v.balls
                .iter()
                .any(|o| (o.owner != e.seat.slot || o.deadly) && dist(e.pos, o.pos) < 1.)
        {
            self.opening = None;
            return;
        }
        let fresh = self
            .last_open
            .is_none_or(|last| v.tick.saturating_sub(last) >= 16);
        self.last_open = Some(v.tick);
        if fresh {
            self.opening = Some((v.tick, e));
        }
        if let Some((began, initial)) = self.opening {
            if dist(e.pos, initial.pos) > 0.20 || err(initial.h, e.h.direction(CELL)).abs() > 0.42 {
                self.opening = None;
            } else if v.tick - began >= 16 {
                self.opening = None;
                // Confirm the *motion* can actually leave the gun line, not just
                // survive a bullet that hasn't reached this distant tank yet.
                if escape_exists(v, e, b, t.ceil() as u64 + 6) {
                    self.exposure.add(v.tick, -0.75);
                }
            }
        }
    }
    fn step(&mut self, v: &View, e: Car, motion: Point, continuous: bool) {
        let steady = continuous
            && self
                .prev
                .is_some_and(|(h, vel)| e.h == h && norm(sub(vel, motion)) < f64::from(PX));
        self.orbit
            .advance(e, self.prev.filter(|_| continuous).map(|p| p.0));
        self.prev = Some((e.h, motion));
        if !continuous {
            self.pending = None;
            return;
        }
        if let Some(mut p) = self.pending.take() {
            let elapsed = v.tick - p.start;
            let Some(&b) = v.balls.iter().find(|b| b.id == p.shot.id) else {
                return;
            };
            if !uncluttered(v, e, b) || b.v != p.shot.v || elapsed >= p.eta {
                return;
            }
            let changed = e.h != p.car.h
                || dist(e.pos, add(p.car.pos, mul(p.motion, elapsed as f64))) > 0.018;
            if changed && eta(b, e, motion, p.eta + 6 - elapsed).is_none() {
                let since = *p.cleared.get_or_insert(elapsed);
                if elapsed > since {
                    self.response = Some(since);
                    self.dodge
                        .add(v.tick, ((10. - since as f64) / 8.).clamp(-1., 1.));
                    return;
                }
            } else {
                p.cleared = None;
            }
            if elapsed + 3 >= p.eta {
                if !changed && eta(b, e, motion, 7).is_some() {
                    self.dodge.add(v.tick, -0.8);
                    self.late = self.late.saturating_add(1);
                }
            } else {
                self.pending = Some(p);
            }
            return;
        }
        if !steady || v.tick < self.next || !v.tick.is_multiple_of(3) {
            return;
        }
        for &b in v
            .balls
            .iter()
            .filter(|b| (b.owner != e.seat.slot || b.deadly) && b.kind.is_none())
        {
            if dist(e.pos, b.pos) > 2.8 || self.seen.contains(&Some(b.id)) {
                continue;
            }
            let Some(t) = eta(b, e, motion, 34) else {
                continue;
            };
            if t < 17. || !uncluttered(v, e, b) {
                continue;
            }
            let t = t.ceil() as u64;
            let span = t + 6;
            let line = mul(b.v, span as f64);
            let drift = mul(motion, span as f64);
            if u64::from(b.life) < span
                || s::wall(v.map, b.pos, line, SIZES.shell_radius).is_some()
                || s::wall(v.map, e.pos, drift, SIZES.half_length).is_some()
                || v.cars.iter().any(|o| {
                    o.seat != e.seat
                        && (((o.seat.slot != b.owner || b.deadly)
                            && s::body_hit(b.pos, line, &o.tank(), SIZES.shell_radius).is_some())
                            || s::body_hit(e.pos, drift, &o.tank(), SIZES.half_length).is_some())
                })
                || !escape_exists(v, e, b, span)
            {
                continue;
            }
            self.seen[self.cursor] = Some(b.id);
            self.cursor = (self.cursor + 1) % self.seen.len();
            self.next = v.tick + 36;
            self.pending = Some(Trial {
                shot: b,
                start: v.tick,
                eta: t,
                car: e,
                motion,
                cleared: None,
            });
            break;
        }
    }
}

// Grade only fresh, ordinary rounds with an unambiguous first contact in BOTH
// a brake and a short coasting model. A miss (including possible area denial)
// or a speculative later self-return is not evidence of poor decisions.
fn shot_choice(v: &View, b: Ball, shooter: Car, motion: &[Point; 8]) -> Option<f64> {
    let mut first = [None; 2];
    ball::trace(v, b, 40, |t, leg| {
        for (k, result) in first.iter_mut().enumerate().filter(|(_, r)| r.is_none()) {
            let hit = v
                .cars
                .iter()
                .filter(|c| c.seat.slot != b.owner || leg.armed)
                .filter_map(|c| {
                    let mut car = c.tank();
                    let drift = mul(motion[ix(c.seat.slot)], (k * t.min(10)) as f64);
                    if drift != Point::default()
                        && s::wall(v.map, c.pos, drift, SIZES.half_width).is_none()
                    {
                        car.center = add(car.center, drift);
                    }
                    s::body_hit(leg.p, leg.d, &car, leg.r).map(|d| (norm(d), c))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, c)) = hit {
                *result = Some((c.seat, c.shield < t as u64, t));
            }
        }
        first.iter().any(Option::is_none)
    });
    let [(a, true, ta), (b, true, tb)] = [first[0]?, first[1]?] else {
        return None;
    };
    if a != b {
        None
    } else if a.team != shooter.seat.team {
        Some(1.)
    } else if ta.max(tb) <= 14 {
        Some(-1.)
    } else {
        None
    }
}
impl Book {
    pub(super) fn observe(
        &mut self,
        v: &View,
        motion: &[Point; 8],
        last: Option<u64>,
    ) -> [Read; 8] {
        let continuous = last.is_some_and(|t| t.checked_add(1) == Some(v.tick));
        let mut reads = [Read::default(); 8];
        for &e in v.cars.iter().filter(|c| c.seat.team != v.me.seat.team) {
            let i = ix(e.seat.slot);
            let seat = &mut self.0[i];
            seat.step(v, e, motion[i], continuous);
            seat.observe_opening(v, e, continuous);
            if continuous && (seat.shot.samples == 0 || v.tick.saturating_sub(seat.shot.at) >= 40) {
                for &b in v
                    .balls
                    .iter()
                    .filter(|b| b.owner == e.seat.slot && b.kind.is_none() && b.life == s::LIFE - 1)
                {
                    if let Some(value) = shot_choice(v, b, e, motion) {
                        seat.shot.add(v.tick, value);
                        break;
                    }
                }
            }
            reads[i] = Read {
                evasion: seat.dodge.estimate(v.tick),
                shooting: seat.shot.estimate(v.tick),
                consecutive_laps: seat.orbit.laps,
                exposure: seat.exposure.estimate(v.tick),
                exposed_windows: seat.exposure.samples,
                dodge_samples: seat.dodge.samples,
                shot_samples: seat.shot.samples,
                last_response_ticks: seat.response,
                late_windows: seat.late,
            };
        }
        reads
    }
}
