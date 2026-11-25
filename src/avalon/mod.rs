//! Avalon: an observation-only native pilot. No VM, referee handle or RNG state in decide().
use crate::sim::{self as s, CELL, PX, SIZES, Tank};
pub use crate::sim::{
    ArenaMap, ControlUpdate, CrateDrop, Drive, Heading, Item, Point, Seat, ShotKind, Sight, SlotId,
    Turn,
};
use serde::Serialize;
mod ball;
mod fire;
mod lanes;
mod opponents;
pub use opponents::Read as OpponentRead;
mod planner;
mod route;
mod tactics;
#[derive(Clone, Copy, Debug)]
pub struct Car {
    pub seat: Seat,
    pub pos: Point,
    pub h: Heading,
    pub weapon: Option<Item>,
    pub shield: u64,
    pub boost: u64,
}
impl Car {
    pub fn observe(t: &Tank, items: &s::Items, tick: u64) -> Self {
        let gear = items.gear(t.seat.slot);
        Self {
            seat: t.seat,
            pos: t.center,
            h: t.heading,
            weapon: gear.weapon(),
            shield: gear.shield_until.saturating_sub(tick),
            boost: gear.boost_until.saturating_sub(tick),
        }
    }

    fn tank(self) -> Tank {
        Tank {
            seat: self.seat,
            center: self.pos,
            heading: self.h,
            alive: true,
            drive: Drive::Stop,
            turn: Turn::None,
            fire_down: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub id: u64,
    pub owner: SlotId,
    pub pos: Point,
    pub v: Point,
    pub life: u16,
    pub kind: Option<ShotKind>,
    pub deadly: bool,
}
impl Ball {
    pub fn observe(shell: &s::Shell, items: &s::Items) -> Self {
        let payload = items.projectiles.get(&shell.id);
        Self {
            id: shell.id,
            owner: shell.owner,
            pos: shell.center,
            v: shell.velocity,
            life: shell.life,
            kind: payload.map(|p| p.kind),
            deadly: shell.deadly_to_owner() || payload.is_some_and(|p| p.can_hit_owner()),
        }
    }
}

pub struct View<'a> {
    pub map: &'a ArenaMap,
    pub tick: u64,
    pub me: Car,
    pub sight: Sight,
    /// Number of weapons in this seat's FIFO, including the active one.
    pub weapon_slots_used: usize,
    pub cars: &'a [Car],
    pub balls: &'a [Ball],
    pub crates: &'a [CrateDrop],
}
impl View<'_> {
    /// Only ordinary rounds from this seat can return one of its normal ammo slots.
    fn next_refill(&self) -> u16 {
        self.balls
            .iter()
            .filter(|p| p.owner == self.me.seat.slot && p.kind.is_none())
            .map(|p| p.life)
            .min()
            .unwrap_or(s::LIFE)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Thought {
    pub target: u32,
    pub task: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supply_target: Option<Point>,
    pub opponent_read: OpponentRead,
    pub posture: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relief_instead_of: Option<SlotId>,
    pub waypoint: Point,
    pub depth: usize,
    pub nodes: u32,
    pub hazard: Option<u64>,
    pub hit_shell: Option<u64>,
    pub first_hit: Option<usize>,
    pub ballistic_ticks: usize,
    pub fire_reason: &'static str,
}
#[derive(Default)]
pub struct Avalon {
    opponents: opponents::Book,
    previous: [Option<Point>; 8],
    velocity: [Point; 8],
    last_tick: Option<u64>,
    last_shot: Option<u64>,
    motion_history: planner::MotionHistory,
    terrain: tactics::Tactics,
    supply: Option<CrateDrop>,
    anchor: Option<(Point, u64)>,
    lock_until: u64,
    detonating: Option<u64>,
}
fn add(a: Point, b: Point) -> Point {
    Point {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}
fn sub(a: Point, b: Point) -> Point {
    Point {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}
fn mul(a: Point, f: f64) -> Point {
    Point {
        x: (a.x as f64 * f).round() as i32,
        y: (a.y as f64 * f).round() as i32,
    }
}
fn norm(a: Point) -> f64 {
    (a.x as f64).hypot(a.y as f64)
}
fn dist(a: Point, b: Point) -> f64 {
    norm(sub(a, b)) / CELL as f64
}
fn err(h: Heading, d: Point) -> f64 {
    let a = (d.y as f64).atan2(d.x as f64) - h.radians();
    a.sin().atan2(a.cos())
}
fn ix(s: SlotId) -> usize {
    s.0 as usize - 1
}
impl Avalon {
    pub fn decide(&mut self, v: &View) -> (ControlUpdate, Thought) {
        self.decide_with_waypoint(v, None)
    }

    // The offline duel can replace only the navigation goal in a matched-state
    // experiment. Fire control, the evasive search and its history are unchanged.
    pub(crate) fn decide_with_waypoint(
        &mut self,
        v: &View,
        waypoint: Option<(Point, &'static str)>,
    ) -> (ControlUpdate, Thought) {
        let mut log = Thought::default();
        let dt = v
            .tick
            .saturating_sub(self.last_tick.unwrap_or(v.tick))
            .max(1) as f64;
        for c in v.cars {
            let i = ix(c.seat.slot);
            if let Some(p) = self.previous[i] {
                self.velocity[i] = mul(sub(c.pos, p), 1. / dt);
            }
            self.previous[i] = Some(c.pos);
        }
        self.motion_history.observe(v, self.last_tick);
        let reads = self.opponents.observe(v, &self.velocity, self.last_tick);
        self.last_tick = Some(v.tick);
        let order = fire::trigger(v, &self.velocity, self.last_shot, &mut log.ballistic_ticks);
        let anchor = self.anchor.get_or_insert((v.me.pos, v.tick));
        if dist(anchor.0, v.me.pos) > 0.35 || order.press || dt > 12. {
            *anchor = (v.me.pos, v.tick);
        }
        let stalled = v.tick.saturating_sub(anchor.1) >= 100
            && v.me.shield == 0
            && (v.sight.ammo >= 2 || v.me.weapon.is_some())
            && v.me.weapon != Some(Item::Mine);
        let mut goal = route::mission(v, &self.velocity, &reads, self.supply, order.press, stalled);
        self.supply = goal.supply;
        let original = goal;
        if let Some((point, task)) = waypoint {
            goal.point = point;
            goal.combat = false;
            goal.task = task;
        } else {
            self.terrain.navigate(v, &mut goal, order.press);
        }
        log.task = goal.task;
        log.waypoint = goal.point;
        let mut fire = order.press;
        let reason = order.reason;
        let remote = v
            .balls
            .iter()
            .find(|b| b.owner == v.me.seat.slot && b.kind == Some(ShotKind::Frag))
            .map(|b| b.id);
        if remote != self.detonating {
            self.detonating = None;
        }
        if remote.is_some() && self.detonating == remote {
            fire = false;
            log.fire_reason = "fuse running";
        } else {
            log.fire_reason = reason;
        }
        let (mut act, mut tail) = planner::choose(
            v,
            &goal,
            &self.velocity,
            &self.motion_history,
            self.lock_until.saturating_sub(v.tick) as usize,
            fire,
            &mut log,
        );
        if waypoint.is_none()
            && let Some(dest) = self.terrain.started(v.tick)
        {
            let mut hold = Thought::default();
            let (direct, direct_tail) = planner::choose(
                v,
                &original,
                &self.velocity,
                &self.motion_history,
                self.lock_until.saturating_sub(v.tick) as usize,
                fire,
                &mut hold,
            );
            log.nodes += hold.nodes;
            if !tactics::improves(
                v,
                dest,
                (act, &tail, log.first_hit),
                (direct, &direct_tail, hold.first_hit),
            ) {
                self.terrain.abort(v.tick);
                act = direct;
                tail = direct_tail;
                goal = original;
                log.waypoint = goal.point;
                log.task = goal.task;
                log.first_hit = hold.first_hit;
                log.hazard = hold.hazard;
                log.hit_shell = hold.hit_shell;
                log.depth = hold.depth;
            }
        }
        // A collectible is optional. If this detour is about to get hit,
        // compare with the same fire decision and the ordinary navigation goal.
        if goal.supply.is_some() && waypoint.is_none() && log.first_hit.is_some_and(|t| t <= 45) {
            let without = View { crates: &[], ..*v };
            let normal = route::mission(&without, &self.velocity, &reads, None, order.press, false);
            let mut check = Thought::default();
            let (ka, seq) = planner::choose(
                v,
                &normal,
                &self.velocity,
                &self.motion_history,
                self.lock_until.saturating_sub(v.tick) as usize,
                fire,
                &mut check,
            );
            log.nodes += check.nodes;
            if check.first_hit.is_none() {
                self.supply = None;
                goal = normal;
                act = ka;
                tail = seq;
                log.task = goal.task;
                log.waypoint = goal.point;
                log.first_hit = check.first_hit;
                log.hazard = check.hazard;
                log.hit_shell = check.hit_shell;
                log.depth = check.depth;
            }
        }
        log.target = goal.enemy.seat.slot.0;
        log.opponent_read = reads[ix(goal.enemy.seat.slot)];
        log.posture = if !goal.combat || goal.tempo == 0. {
            "standard"
        } else if goal.tempo > 0. {
            "press"
        } else {
            "patient"
        };
        log.relief_instead_of = goal.relief_from;
        log.supply_target = goal.supply.map(|b| b.center);
        // The new projectile is hypothetical. Do not create a forced return if withholding
        // it leaves a viable route. A real projectile can never be removed by this test.
        let unused_screen = order
            .screen
            .is_some_and(|s| log.first_hit.is_some() || !s.used_by(v, act, &tail));
        // A laser roots the tank before the actual shot has flown. Test the
        // unlocked alternative if a real round would reach us during that lock.
        let unsafe_lock = v.me.weapon == Some(Item::Laser)
            && log.first_hit.is_some_and(|t| t <= s::LASER_LOCK as usize);
        if fire && (unused_screen || unsafe_lock || log.hit_shell == Some(u64::MAX)) {
            let mut alt = Thought::default();
            let (other, other_tail) = planner::choose(
                v,
                &goal,
                &self.velocity,
                &self.motion_history,
                self.lock_until.saturating_sub(v.tick) as usize,
                false,
                &mut alt,
            );
            log.nodes += alt.nodes;
            if unused_screen || alt.first_hit.is_none() {
                fire = false;
                act = other;
                tail = other_tail;
                log.hazard = alt.hazard;
                log.hit_shell = alt.hit_shell;
                log.first_hit = alt.first_hit;
                log.depth = alt.depth;
                log.fire_reason = if unused_screen {
                    "no safe reposition"
                } else if unsafe_lock {
                    "evade before laser lock"
                } else {
                    "veto forced self-return"
                };
            }
        }
        self.motion_history.commit(act, tail);
        if fire && v.me.weapon == Some(Item::Laser) && v.sight.can_fire {
            self.lock_until = v.tick + s::LASER_LOCK;
        }
        if fire && reason == "remote fuse" {
            self.detonating = remote;
        }
        if fire && !v.sight.fire_down {
            self.last_shot = Some(v.tick);
        }
        (
            ControlUpdate {
                drive: Some(act.0),
                turn: Some(act.1),
                fire: Some(fire),
            },
            log,
        )
    }
}

/// Match and duel recordings identify the policy by its compiled-in source snapshot.
pub fn fingerprint() -> blake3::Hash {
    static DIGEST: std::sync::OnceLock<blake3::Hash> = std::sync::OnceLock::new();
    *DIGEST.get_or_init(|| {
        let mut h = blake3::Hasher::new();
        for source in [
            include_bytes!("mod.rs").as_slice(),
            include_bytes!("route.rs"),
            include_bytes!("ball.rs"),
            include_bytes!("fire.rs"),
            include_bytes!("planner.rs"),
            include_bytes!("lanes.rs"),
            include_bytes!("tactics.rs"),
            include_bytes!("opponents.rs"),
        ] {
            h.update(source);
        }
        h.finalize()
    })
}

#[cfg(test)]
mod tests;
