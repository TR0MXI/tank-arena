use super::*;
use crate::sim::geometry::{tank_clear, tanks_overlap};

impl Tank {
    fn pose(&self) -> (Point, Heading) {
        (self.center, self.heading)
    }

    fn proposed_pose(&self, map: &ArenaMap, boost: bool) -> (Point, Heading) {
        let mult = if boost { 100 + BOOST_PERCENT } else { 100 };
        let turn_step = (u64::from(Heading::STEP) * mult as u64 / 100) as u32;
        let mut heading = match self.turn {
            Turn::Right => Heading(self.heading.0.wrapping_add(turn_step)),
            Turn::Left => Heading(self.heading.0.wrapping_sub(turn_step)),
            Turn::None => self.heading,
        };
        if heading != self.heading && !tank_clear(map, self.center, heading) {
            heading = self.heading;
        }

        let speed = match self.drive {
            Drive::Forward => FORWARD_SPEED,
            Drive::Reverse => -REVERSE_SPEED,
            Drive::Stop => return (self.center, heading),
        };
        let next = self.center.plus(heading.direction(speed * mult / 100));
        let center = if tank_clear(map, next, heading) {
            next
        } else {
            self.center
        };
        (center, heading)
    }
}

impl World {
    pub(super) fn drive(&mut self) {
        // 先生成候选姿态，再统一判阻挡；不能让名单中排在前面的坦克先占位置。
        let mut moves = [None; MAX_SLOTS];
        for (index, tank) in self.tanks.iter().enumerate().filter(|(_, tank)| tank.alive) {
            if self.items.gear(tank.seat.slot).locked_until > self.tick {
                continue;
            }
            let pose = tank.proposed_pose(
                &self.map,
                self.items.gear(tank.seat.slot).boost_until > self.tick,
            );
            if pose != tank.pose() {
                moves[index] = Some(pose);
            }
        }

        let mut blocked = [false; MAX_SLOTS];
        for (i, &pose) in moves[..self.tanks.len()].iter().enumerate() {
            let Some(next) = pose else { continue };

            let current = self.tanks[i].pose();
            for (j, other) in self.tanks.iter().enumerate() {
                if i == j || !other.alive || (j < i && moves[j].is_some()) {
                    continue;
                }

                // 两辆车不能同时占据同一位置，也不能停进对方可能被挡回的原位。
                let other_next = moves[j].unwrap_or(other.pose());
                if tanks_overlap(next, other.pose())
                    || tanks_overlap(current, other_next)
                    || (moves[j].is_some() && tanks_overlap(next, other_next))
                {
                    blocked[i] = true;
                    blocked[j] = true;
                }
            }
        }

        for (i, tank) in self.tanks.iter_mut().enumerate() {
            if !blocked[i]
                && let Some(pose) = moves[i]
            {
                (tank.center, tank.heading) = pose;
            }
        }
    }

    pub(super) fn fire(&mut self, shots: &[bool; MAX_SLOTS]) {
        for (i, &shoot) in shots.iter().enumerate().take(self.tanks.len()) {
            let t = self.tanks[i];
            if !t.alive {
                continue;
            }
            let g = &mut self.items.gear[t.seat.slot.0 as usize - 1];
            if g.weapon() == Some(Item::Laser) && g.ammo == 0 && self.tick >= g.ready {
                g.advance();
            }
            if let Some(item) = self.items.gear(t.seat.slot).weapon() {
                self.fire_item(i, shoot, item);
                continue;
            }
            if !shoot || self.shells.ammo(t.seat.slot) == 0 {
                continue;
            }

            let id = self.next_shell;
            self.next_shell += 1;
            self.events.push(Event::Fired {
                shell: id,
                owner: t.seat.slot,
            });
            let (center, mut velocity) = t.shot_path();
            if self.items.gear(t.seat.slot).boost_until > self.tick {
                velocity = t
                    .heading
                    .direction(SIZES.shell_speed * (100 + BOOST_PERCENT) / 100);
            }
            self.shells.push(Shell {
                id,
                owner: t.seat.slot,
                center,
                velocity,
                life: LIFE,
                bounces: [0; BOUNCE_LIMIT],
            });
        }
    }

    pub(super) fn fly(&mut self) {
        let mut dead = [false; MAX_SLOTS];
        let mut blasts = Vec::new();
        self.shells
            .retain_mut(&mut self.items.projectiles, |s, meta| {
                let kind = meta.as_ref().map(|p| p.kind);
                let radius = kind.map_or(SIZES.shell_radius, ShotKind::radius);
                let mut deadly = s.deadly_to_owner();
                if let Some(p) = meta {
                    p.age = p.age.saturating_add(1);
                    if kind == Some(ShotKind::Mine) {
                        if s.velocity != Point::default() {
                            if let Some(hit) = walls(&self.map, s.center, s.velocity, radius) {
                                s.center = s.center.plus(hit.t.part(s.velocity));
                                s.velocity = Point::default();
                            } else {
                                s.center = s.center.plus(s.velocity);
                                let len = libm::hypot(s.velocity.x as f64, s.velocity.y as f64);
                                let ratio =
                                    (1. - (800 * PX / (HZ * HZ) as i32) as f64 / len).max(0.);
                                s.velocity = Point {
                                    x: (s.velocity.x as f64 * ratio).round() as i32,
                                    y: (s.velocity.y as f64 * ratio).round() as i32,
                                };
                            }
                        }
                        let touching =
                            |t: &Tank| body(s.center, Point::default(), t, radius).is_some();
                        p.clear_owner |= !self
                            .tanks
                            .iter()
                            .any(|t| t.alive && t.seat.slot == s.owner && touching(t));
                        if p.age >= MINE_ARM
                            && self.tanks.iter().any(|t| {
                                t.alive && (t.seat.slot != s.owner || p.clear_owner) && touching(t)
                            })
                        {
                            p.fuse.get_or_insert(FUSE);
                        }
                    }
                    if let Some(f) = &mut p.fuse {
                        *f -= 1;
                        if *f == 0 {
                            blasts.push(*s);
                            return false;
                        }
                    }
                    if kind == Some(ShotKind::Mine) {
                        return true;
                    }
                    if kind == Some(ShotKind::Missile) && p.age >= SEEK_DELAY {
                        p.clear_owner = true;
                        if (p.age - SEEK_DELAY).is_multiple_of(6) {
                            weapons::steer(s, &self.map, &self.tanks);
                        }
                    }
                    deadly |= p.clear_owner;
                }
                let mut delta = s.velocity;
                loop {
                    let mut wall = walls(&self.map, s.center, delta, radius);
                    let skip = (!deadly && !s.deadly_to_owner()).then_some(s.owner);
                    for t in &self.tanks {
                        if t.alive
                            && Some(t.seat.slot) != skip
                            && self.items.gear[t.seat.slot.0 as usize - 1].shield_until > self.tick
                            && let Some(c) =
                                geometry::shield(s.center, delta, t.center, radius + SHIELD_RADIUS)
                            && wall.is_none_or(|w| c.t.cmp(w.t).is_lt())
                        {
                            wall = Some(c);
                        }
                    }
                    let target = first_tank(&self.tanks, s.center, delta, skip, radius);
                    let trace = |beams: &mut Vec<Beam>, to| {
                        if kind == Some(ShotKind::Laser) {
                            beams.push(Beam {
                                from: s.center,
                                to,
                                until: self.tick + 8,
                            });
                        }
                    };
                    if let Some((i, h)) = target
                        && wall.is_none_or(|w| h.t.cmp(w.t).is_lt())
                    {
                        trace(&mut self.items.beams, s.center.plus(h.t.part(delta)));
                        if kind == Some(ShotKind::Frag) {
                            s.center = s.center.plus(h.t.part(delta));
                            blasts.push(*s);
                        }
                        let victim = self.tanks[i].seat;
                        if self.items.gear[victim.slot.0 as usize - 1].shield_until <= self.tick {
                            dead[i] = true;
                            let owner = self.tanks[self.roster_index[s.owner.0 as usize - 1]].seat;
                            self.events.push(hit(s.id, owner, victim));
                        }
                        return false;
                    }
                    let Some(w) = wall else {
                        let end = s.center.plus(delta);
                        trace(&mut self.items.beams, end);
                        s.center = end;
                        break;
                    };
                    let used = w.t.part(delta);
                    let end = s.center.plus(used);
                    trace(&mut self.items.beams, end);
                    s.center = end;
                    if kind == Some(ShotKind::Shard) {
                        return false;
                    }
                    delta.x -= used.x;
                    delta.y -= used.y;
                    delta = w.reflect(delta);
                    s.velocity = w.reflect(s.velocity);
                    self.events.push(Event::Bounce {
                        shell: s.id,
                        at: s.center,
                    });
                    if kind == Some(ShotKind::Pellet) {
                        s.life = s.life.min(42);
                    }
                    if s.record_bounce() {
                        if kind == Some(ShotKind::Frag) {
                            blasts.push(*s);
                        }
                        return false;
                    }
                    if delta == Point::default() {
                        break;
                    }
                }
                s.life -= 1;
                if s.life == 0 && kind == Some(ShotKind::Frag) {
                    blasts.push(*s);
                }
                s.life > 0
            });
        for s in blasts {
            let g = &mut self.items.gear[s.owner.0 as usize - 1];
            if g.remote == Some(s.id) {
                g.advance();
            }
            self.explode(s);
        }

        // 等本步所有炮弹结算完再淘汰坦克，保留同一步内的相互命中。
        for (i, t) in self.tanks.iter_mut().enumerate() {
            if dead[i] {
                self.hit_ticks[i] = Some(self.tick + 1);
                t.eliminate();
            }
        }
    }
}

fn hit(shell: u64, owner: Seat, victim: Seat) -> Event {
    let kind = if owner.slot == victim.slot {
        HitKind::SelfHit
    } else if owner.team == victim.team {
        HitKind::Friendly
    } else {
        HitKind::Enemy
    };
    Event::Hit {
        shell,
        owner: owner.slot,
        target: victim.slot,
        kind,
    }
}
