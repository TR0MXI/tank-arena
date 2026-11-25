use super::*;
use std::collections::VecDeque;

const fn speed(px_per_second: i32) -> i32 {
    px_per_second * PX / HZ as i32
}
impl World {
    pub(super) fn fire_item(&mut self, index: usize, edge: bool, item: Item) {
        let t = self.tanks[index];
        let slot = t.seat.slot;
        let i = slot.0 as usize - 1;
        let g = &mut self.items.gear[i];
        if item == Item::Gatling {
            if !t.fire_down {
                g.firing = false;
                return;
            }
            if !g.firing {
                g.firing = true;
                g.ready = self.tick + GATLING_WARMUP;
                return;
            }
        } else if !edge {
            return;
        }
        if self.tick < g.ready || (g.ammo == 0 && g.remote.is_none()) {
            return;
        }
        if let Some(id) = g.remote {
            if let Some(p) = self.items.projectiles.get_mut(&id) {
                p.fuse.get_or_insert(FUSE);
            }
            return;
        }
        let ammo = g.ammo;
        g.ammo -= 1;
        let (kind, offset, life, velocity, count, spread) = match item {
            Item::Laser => {
                g.locked_until = self.tick + LASER_LOCK;
                g.ready = self.tick + u64::from(LASER_LIFE);
                (ShotKind::Laser, 50, LASER_LIFE, LASER_SPEED, 1, 0.)
            }
            Item::Double => (ShotKind::Double, 45, DOUBLE_LIFE, 360, 1, 0.),
            Item::Shotgun => {
                g.ready = self.tick + SHOTGUN_RELOAD;
                (ShotKind::Pellet, 49, 120, 600, PELLETS, 0.3)
            }
            Item::Missile => (ShotKind::Missile, 50, MISSILE_LIFE, 360, 1, 0.),
            Item::Mine => (ShotKind::Mine, -29, u16::MAX, -250, 1, 0.),
            Item::Gatling => {
                g.ready = self.tick + GATLING_INTERVAL;
                (ShotKind::Gatling, 60, 120, 500, 1, 0.06)
            }
            Item::Frag => (ShotKind::Frag, 45, FRAG_LIFE, 360, 1, 0.),
            Item::Shield | Item::Speed => unreachable!(),
        };
        let boosted = g.boost_until > self.tick;
        let mut center = t.center.plus(t.heading.direction(offset * PX));
        if item == Item::Double {
            center = center.plus(Heading(t.heading.0.wrapping_add(1 << 30)).direction(
                if ammo.is_multiple_of(2) {
                    -9 * PX
                } else {
                    9 * PX
                },
            ));
        }
        // Never drop a stationary trap on the far side of a thin wall.
        if item == Item::Mine {
            // Bound the number of unattended traps during unlimited matches.
            let mut planted = self.shells.as_slice().iter().filter(|s| {
                s.owner == slot
                    && self
                        .items
                        .projectiles
                        .get(&s.id)
                        .is_some_and(|p| p.kind == ShotKind::Mine && p.fuse.is_none())
            });
            if let Some(old) = planted.next()
                && planted.count() >= 2
            {
                self.items.projectiles.get_mut(&old.id).unwrap().fuse = Some(FUSE);
            }
            let offset = t.heading.direction(offset * PX);
            if let Some(w) = walls(&self.map, t.center, offset, kind.radius()) {
                center = t.center.plus(w.t.part(offset));
            }
        }
        let first = self.next_shell;
        for _ in 0..count {
            let (angle, v) = if spread != 0. {
                let r = self.items.random(65536) as f64 / 65535. - 0.5;
                let da = (r * spread * 4294967296. / std::f64::consts::TAU) as i64;
                (
                    Heading(t.heading.0.wrapping_add(da as u32)),
                    velocity + self.items.random(101) as i32,
                )
            } else {
                (t.heading, velocity)
            };
            let mut v = speed(v);
            if item == Item::Double && boosted {
                v = v * (100 + BOOST_PERCENT) / 100;
            }
            self.special(slot, kind, center, angle.direction(v), life);
        }
        self.events.push(Event::Fired {
            shell: first,
            owner: slot,
        });
        let g = &mut self.items.gear[i];
        if item == Item::Frag {
            g.remote = Some(first);
        } else if g.ammo == 0 && item != Item::Laser {
            g.advance();
        }
    }
    fn special(
        &mut self,
        owner: SlotId,
        kind: ShotKind,
        center: Point,
        velocity: Point,
        life: u16,
    ) {
        let id = self.next_shell;
        self.next_shell += 1;
        self.items.projectiles.insert(
            id,
            Payload {
                kind,
                age: 0,
                fuse: None,
                clear_owner: kind == ShotKind::Shard,
            },
        );
        self.shells.push_special(Shell {
            id,
            owner,
            center,
            velocity,
            life,
            bounces: [0; BOUNCE_LIMIT],
        });
    }
    pub(super) fn explode(&mut self, s: Shell) {
        for _ in 0..FRAGMENTS {
            let angle = Heading(self.items.random(1 << 32) as u32);
            let v = speed(500 + self.items.random(201) as i32);
            self.special(s.owner, ShotKind::Shard, s.center, angle.direction(v), 120);
        }
    }
}

// Follow the nearest tank by maze distance, not through a wall separating two corridors.
// Ties favour another tank over the launcher. Steering is bounded so missiles can be dodged.
pub(super) fn steer(s: &mut Shell, map: &ArenaMap, tanks: &[Tank]) {
    let (w, h) = map.size();
    let cell = |p: Point| {
        let x = (p.x / CELL).clamp(0, w as i32 - 1) as usize;
        let y = (p.y / CELL).clamp(0, h as i32 - 1) as usize;
        y * w + x
    };
    let start = cell(s.center);
    let mut path = vec![(u16::MAX, start); w * h];
    path[start] = (0, start);
    let mut q = VecDeque::from([start]);
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if map.passage((x, y), (nx, ny)) {
                let j = ny * w + nx;
                if path[j].0 == u16::MAX {
                    path[j] = (path[i].0 + 1, if i == start { j } else { path[i].1 });
                    q.push_back(j);
                }
            }
        }
    }
    let Some(t) = tanks.iter().filter(|t| t.alive).min_by_key(|t| {
        (
            path[cell(t.center)].0,
            t.seat.slot == s.owner,
            distance2(s.center, t.center),
        )
    }) else {
        return;
    };
    let (d, next) = path[cell(t.center)];
    let p = if d <= 1 {
        t.center
    } else {
        Point {
            x: (next % w) as i32 * CELL + CELL / 2,
            y: (next / w) as i32 * CELL + CELL / 2,
        }
    };
    let target = libm::atan2((p.y - s.center.y) as f64, (p.x - s.center.x) as f64);
    let a = libm::atan2(s.velocity.y as f64, s.velocity.x as f64);
    let diff = (target - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    let (sin, cos) = libm::sincos(a + diff.clamp(-0.5, 0.5));
    s.velocity = Point {
        x: (cos * speed(360) as f64).round() as i32,
        y: (sin * speed(360) as f64).round() as i32,
    };
}

/// Segments for the laser guide; use the same wall sweep/reflection as the projectile.
pub fn laser_guide(map: &ArenaMap, t: &Tank, tanks: &[Tank]) -> Vec<(Point, Point)> {
    let mut p = t.shot_path().0;
    let mut d = t.heading.direction(6 * CELL);
    let mut segments = Vec::new();
    for i in 0..16 {
        let w = walls(map, p, d, 0);
        if let Some((_, h)) = first_tank(tanks, p, d, (i == 0).then_some(t.seat.slot), 0)
            && w.is_none_or(|w| h.t.cmp(w.t).is_lt())
        {
            segments.push((p, p.plus(h.t.part(d))));
            break;
        }
        let Some(w) = w else {
            segments.push((p, p.plus(d)));
            break;
        };
        let used = w.t.part(d);
        let end = p.plus(used);
        segments.push((p, end));
        p = end;
        d = w.reflect(Point {
            x: d.x - used.x,
            y: d.y - used.y,
        });
        if d == Point::default() {
            break;
        }
    }
    segments
}
