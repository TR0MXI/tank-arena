use super::*;
use crate::sim::geometry::{tank_clear, tanks_overlap};

impl Tank {
    fn pose(&self) -> (Point, Heading) {
        (self.center, self.heading)
    }

    fn proposed_pose(&self, map: &ArenaMap) -> (Point, Heading) {
        let mut heading = match self.turn {
            Turn::Right => Heading(self.heading.0.wrapping_add(Heading::STEP)),
            Turn::Left => Heading(self.heading.0.wrapping_sub(Heading::STEP)),
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
        let next = self.center.plus(heading.direction(speed));
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
            let pose = tank.proposed_pose(&self.map);
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
            if !shoot || !t.alive || self.shells.ammo(t.seat.slot) == 0 {
                continue;
            }

            let id = self.next_shell;
            self.next_shell += 1;
            self.events.push(Event::Fired {
                shell: id,
                owner: t.seat.slot,
            });
            let (center, velocity) = t.shot_path();
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
        self.shells.retain_mut(|s| {
            let mut delta = s.velocity;
            loop {
                let wall = walls(&self.map, s.center, delta, SIZES.shell_radius);
                let target = first_tank(
                    &self.tanks,
                    s.center,
                    delta,
                    (!s.deadly_to_owner()).then_some(s.owner),
                );
                if let Some((i, h)) = target
                    && wall.is_none_or(|w| h.t.cmp(w.t).is_lt())
                {
                    dead[i] = true;
                    let victim = self.tanks[i].seat;
                    let owner = self.tanks[self.roster_index[s.owner.0 as usize - 1]].seat;
                    self.events.push(hit(s.id, owner, victim));
                    return false;
                }

                let Some(w) = wall else {
                    s.center = s.center.plus(delta);
                    break;
                };

                let used = w.t.part(delta);
                s.center = s.center.plus(used);
                delta.x -= used.x;
                delta.y -= used.y;
                delta = w.reflect(delta);
                s.velocity = w.reflect(s.velocity);
                self.events.push(Event::Bounce {
                    shell: s.id,
                    at: s.center,
                });
                if s.record_bounce() {
                    return false;
                }
                if delta == Point::default() {
                    break;
                }
            }

            s.life -= 1;
            s.life > 0
        });

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
