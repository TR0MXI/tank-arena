mod checkpoint;
mod items;
mod physics;
mod weapons;
pub use items::*;
pub use weapons::laser_guide;
mod shells;

use super::{
    geometry::{body, walls},
    *,
};
use bitcode::{Decode, Encode};
pub(crate) use checkpoint::Checkpoint;
use serde::{Deserialize, Serialize};
use shells::Shells;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Tank {
    pub seat: Seat,
    pub center: Point,
    pub heading: Heading,
    pub alive: bool,
    pub drive: Drive,
    pub turn: Turn,
    pub fire_down: bool,
}

impl Tank {
    /// 更新按键并报告扳机是否刚被按下。空仓扣扳机也会锁存，不会在补弹时自动开火。
    fn apply_control(&mut self, update: ControlUpdate) -> bool {
        self.drive = update.drive.unwrap_or(self.drive);
        self.turn = update.turn.unwrap_or(self.turn);

        let was_down = self.fire_down;
        self.fire_down = update.fire.unwrap_or(was_down);
        self.fire_down && !was_down
    }

    fn eliminate(&mut self) {
        self.alive = false;
        self.drive = Drive::Stop;
        self.turn = Turn::None;
        self.fire_down = false;
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Shell {
    pub id: u64,
    pub owner: SlotId,
    pub center: Point,
    pub velocity: Point,
    pub life: u16,
    /// 记录最近几次撞墙时炮弹的剩余寿命；零表示该项未使用。
    pub bounces: [u16; BOUNCE_LIMIT],
}

impl Shell {
    pub fn deadly_to_owner(&self) -> bool {
        self.bounces[0] != 0
    }

    /// 使用模拟时间而非现实时间，暂停回放不会改变判定结果。
    fn record_bounce(&mut self) -> bool {
        self.bounces.rotate_right(1);
        self.bounces[0] = self.life;
        let oldest = self.bounces[BOUNCE_LIMIT - 1];
        oldest != 0 && u32::from(oldest - self.life) * 1000 <= BOUNCE_WINDOW_MS * HZ
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub enum HitKind {
    Enemy,
    Friendly,
    SelfHit,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub enum Event {
    Fired {
        shell: u64,
        owner: SlotId,
    },
    Bounce {
        shell: u64,
        at: Point,
    },
    Hit {
        shell: u64,
        owner: SlotId,
        target: SlotId,
        kind: HitKind,
    },
    Forfeit(SlotId),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub enum Outcome {
    Winner(TeamId),
    MutualElimination,
    Timeout,
    AllForfeited,
    Stopped,
    WorkerLost,
}

// 离线试验可以复制局面：只读地图共享，控制状态和炮弹独立。
#[derive(Clone, Debug)]
pub struct World {
    // 席位脚本和道具分别从开局种子派生随机流。
    behavior_seed: Option<u64>,
    cue: Option<Event>,
    // 显示用的命中时刻按名单顺序索引，不计入裁判状态或其哈希。
    hit_ticks: [Option<u64>; MAX_SLOTS],
    map: Arc<ArenaMap>,
    clock: ClockRules,
    tick: u64,
    tanks: Vec<Tank>,
    shells: Shells,
    items: Items,
    // 席号是稳定的；空席用越界下标表示，不能把席号直接当作紧凑名单的下标。
    roster_index: [usize; MAX_SLOTS],
    events: Vec<Event>,
    next_shell: u64,
    outcome: Option<Outcome>,
}

impl World {
    pub fn new(
        map: Arc<ArenaMap>,
        seats: Vec<Seat>,
        clock: ClockRules,
    ) -> Result<Self, ConfigError> {
        let roster = Roster::new(seats, &map)?;
        let tanks = roster
            .seats()
            .iter()
            .zip(map.spawns())
            .map(|(&seat, s)| Tank {
                seat,
                center: s.center,
                heading: s.heading,
                alive: true,
                drive: Drive::Stop,
                turn: Turn::None,
                fire_down: false,
            })
            .collect();
        let mut roster_index = [usize::MAX; MAX_SLOTS];
        for (i, seat) in roster.seats().iter().enumerate() {
            roster_index[seat.slot.0 as usize - 1] = i;
        }

        Ok(Self {
            behavior_seed: None,
            roster_index,
            cue: None,
            hit_ticks: [None; MAX_SLOTS],
            map,
            clock,
            tick: 0,
            tanks,
            shells: Shells::from_rounds(
                Vec::with_capacity(MAX_SLOTS * usize::from(AMMO)),
                &Items::default(),
            ),
            items: Items::default(),
            events: Vec::with_capacity(128),
            next_shell: 0,
            outcome: None,
        })
    }

    /// None 表示无随机种子的确定性基线；Some(0) 也是有效的随机流。
    pub fn with_behavior_seed(mut self, seed: Option<u64>) -> Self {
        self.behavior_seed = seed;
        if self.tick == 0 && self.items.pool != ItemPool::NONE {
            let pool = self.items.pool;
            return self.with_items(pool);
        }
        self
    }

    pub fn behavior_seed(&self) -> Option<u64> {
        self.behavior_seed
    }

    /// 每席得到能由 Luau 数值类型精确表示的 32 位值，而不是地图种子或宿主时钟。
    pub fn pilot_seed(&self, slot: SlotId) -> Option<u32> {
        self.behavior_seed.map(|seed| {
            let bytes = blake3::hash(&seed.to_le_bytes());
            let base = u32::from_le_bytes(bytes.as_bytes()[..4].try_into().unwrap());
            base.wrapping_add(slot.0.wrapping_mul(0x9e37_79b9))
        })
    }

    pub fn map(&self) -> &ArenaMap {
        &self.map
    }

    pub fn clock(&self) -> ClockRules {
        self.clock
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn tanks(&self) -> &[Tank] {
        &self.tanks
    }

    /// 按稳定席位编号查询；阵容有空位时，编号并不等于名单下标加一。
    pub fn tank(&self, slot: SlotId) -> Option<&Tank> {
        let index = self.roster_index.get(slot.0.checked_sub(1)? as usize)?;
        self.tanks.get(*index)
    }

    pub fn shells(&self) -> &[Shell] {
        self.shells.as_slice()
    }

    pub fn cue(&self) -> Option<Event> {
        self.cue
    }

    pub fn hit_ticks(&self) -> &[Option<u64>; MAX_SLOTS] {
        &self.hit_ticks
    }

    pub(crate) fn restore_cues(&mut self, c: Option<Event>, hits: [Option<u64>; MAX_SLOTS]) {
        self.cue = c;
        self.hit_ticks = hits;
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn stop(&mut self) {
        self.invalidate(Outcome::Stopped);
    }

    pub(crate) fn invalidate(&mut self, outcome: Outcome) {
        self.outcome.get_or_insert(outcome);
    }

    pub fn remaining_ammo(&self, slot: SlotId) -> Option<u8> {
        self.tank(slot).map(|_| self.available_ammo(slot))
    }

    fn available_ammo(&self, slot: SlotId) -> u8 {
        let g = self.items.gear(slot);
        if g.weapon().is_some() {
            g.ammo.max(u8::from(g.remote.is_some()))
        } else {
            self.shells.ammo(slot)
        }
    }

    /// 只计算指定席位的视线。定位坦克和读取弹槽均不需要遍历列表。
    pub fn sight(&self, slot: SlotId) -> Option<Sight> {
        self.tank(slot).map(|tank| self.sight_for(tank))
    }

    /// 按名单顺序惰性计算各席的观察，包含已阵亡的坦克。
    pub fn sights(&self) -> impl ExactSizeIterator<Item = Sight> + '_ {
        self.tanks.iter().map(|tank| self.sight_for(tank))
    }

    fn sight_for(&self, tank: &Tank) -> Sight {
        let ammo = self.available_ammo(tank.seat.slot);
        Sight {
            drive: tank.drive,
            turn: tank.turn,
            fire_down: tank.fire_down,
            enemy_ahead: tank.alive && self.enemy_ahead(tank),
            can_fire: tank.alive
                && ammo > 0
                && (self.items.gear(tank.seat.slot).weapon() == Some(Item::Gatling)
                    || (!tank.fire_down && self.items.gear(tank.seat.slot).ready <= self.tick)),
            ammo,
        }
    }

    fn enemy_ahead(&self, tank: &Tank) -> bool {
        let (muzzle, velocity) = tank.shot_path();
        let delta = Point {
            x: velocity.x * i32::from(LIFE),
            y: velocity.y * i32::from(LIFE),
        };
        let Some((index, hit)) = first_tank(
            &self.tanks,
            muzzle,
            delta,
            Some(tank.seat.slot),
            SIZES.shell_radius,
        ) else {
            return false;
        };

        // 只看直射路径上的第一个目标；与墙同时接触时，墙挡住炮弹。
        self.tanks[index].seat.team != tank.seat.team
            && walls(&self.map, muzzle, delta, SIZES.shell_radius)
                .is_none_or(|wall| hit.t.cmp(wall.t).is_lt())
    }

    /// 同一边界先批量判弃权，再处理控制。缺席或省略字段不改变按键；仅扳机按下边沿请求开炮。
    pub fn step(&mut self, inputs: &[(SlotId, ControlUpdate)], forfeits: &[SlotId]) {
        if self.outcome.is_some() {
            return;
        }

        self.events.clear();
        self.forfeit(forfeits);
        if self.outcome.is_some() {
            return;
        }

        let mut shots = [false; MAX_SLOTS];
        for (i, t) in self.tanks.iter_mut().enumerate() {
            if t.alive
                && let Some((_, update)) = inputs.iter().find(|(id, _)| *id == t.seat.slot)
            {
                shots[i] = t.apply_control(*update);
            }
        }

        // 先按当前炮口位置采样开火，再推进车身和炮弹，顺序与原版控制器一致。
        if self.items.pool != ItemPool::NONE {
            self.update_crates();
        }
        self.fire(&shots);
        self.drive();
        self.fly();
        self.judge(Outcome::MutualElimination);

        if let Some(e) = self.events.last() {
            self.cue = Some(*e);
        }
        self.tick += 1;
        if self.clock.ticks() == Some(self.tick) && self.outcome.is_none() {
            self.outcome = Some(Outcome::Timeout);
        }
    }

    pub fn forfeit(&mut self, ids: &[SlotId]) {
        if ids.is_empty() || self.outcome.is_some() {
            return;
        }

        for t in &mut self.tanks {
            if t.alive && ids.contains(&t.seat.slot) {
                t.eliminate();
                let event = Event::Forfeit(t.seat.slot);
                self.events.push(event);
                self.cue = Some(event);
            }
        }

        self.judge(Outcome::AllForfeited);
    }

    fn judge(&mut self, empty: Outcome) {
        let mut teams = self.tanks.iter().filter(|t| t.alive).map(|t| t.seat.team);
        match teams.next() {
            None => self.outcome = Some(empty),
            Some(team) if teams.all(|v| v == team) => self.outcome = Some(Outcome::Winner(team)),
            _ => {}
        }
    }
}

fn first_tank(
    tanks: &[Tank],
    p: Point,
    d: Point,
    skip: Option<SlotId>,
    radius: i32,
) -> Option<(usize, geometry::Contact)> {
    tanks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.alive && Some(t.seat.slot) != skip)
        .filter_map(|(i, t)| body(p, d, t, radius).map(|h| (i, h)))
        .min_by(|(_, a), (_, b)| a.t.cmp(b.t))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sight {
    pub drive: Drive,
    pub turn: Turn,
    pub fire_down: bool,
    pub enemy_ahead: bool,
    pub can_fire: bool,
    pub ammo: u8,
}

#[cfg(test)]
mod scenarios;

#[cfg(test)]
mod item_tests;
