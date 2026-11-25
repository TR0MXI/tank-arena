//! Deterministic crate pool, equipment and projectile metadata shared by live play and replay.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[repr(u8)]
pub enum Item {
    Laser,
    Double,
    Shotgun,
    Missile,
    Mine,
    Gatling,
    Frag,
    Shield,
    Speed,
}
impl Item {
    pub const ALL: [Self; 9] = [
        Self::Laser,
        Self::Double,
        Self::Shotgun,
        Self::Missile,
        Self::Mine,
        Self::Gatling,
        Self::Frag,
        Self::Shield,
        Self::Speed,
    ];
    pub fn key(self) -> &'static str {
        [
            "laser",
            "doubleBarrel",
            "shotgun",
            "homingMissile",
            "mine",
            "gatling",
            "fragBomb",
            "shield",
            "speedBoost",
        ][self as usize]
    }
    pub fn symbol(self) -> &'static str {
        ["L", "II", "SG", "HM", "M", "G", "F", "SH", "+"][self as usize]
    }
    pub fn rounds(self) -> u8 {
        [1, 10, 3, 1, 3, 20, 1, 0, 0][self as usize]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(try_from = "u16")]
pub struct ItemPool(u16);
impl TryFrom<u16> for ItemPool {
    type Error = &'static str;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        let pool = Self(v);
        pool.is_valid()
            .then_some(pool)
            .ok_or("unknown item in crate pool")
    }
}
impl ItemPool {
    pub const ALL: Self = Self((1 << Item::ALL.len()) - 1);
    pub const NONE: Self = Self(0);
    pub fn contains(self, item: Item) -> bool {
        self.0 & (1 << item as u8) != 0
    }
    pub fn toggle(&mut self, item: Item) {
        self.0 ^= 1 << item as u8;
    }
    pub(crate) fn is_valid(self) -> bool {
        self.0 & !Self::ALL.0 == 0
    }
}

pub const MAX_CRATES: usize = 3;
pub const SHIELD_TICKS: u64 = 6 * HZ as u64;
pub const BOOST_TICKS: u64 = 10 * HZ as u64;
pub const BOOST_PERCENT: i32 = 30;
pub const LASER_LIFE: u16 = 48;
pub const LASER_SPEED: i32 = 3600;
pub const LASER_LOCK: u64 = 12;
pub const DOUBLE_LIFE: u16 = 360;
pub const MISSILE_LIFE: u16 = 600;
pub const SHIELD_RADIUS: i32 = 86 * PX;
pub const SHOTGUN_RELOAD: u64 = HZ as u64;
pub const PELLETS: u8 = 20;
pub const FRAGMENTS: u8 = 30;
pub const SEEK_DELAY: u16 = 2 * HZ as u16;
pub const GATLING_WARMUP: u64 = 12;
pub const GATLING_INTERVAL: u64 = 6;
pub const FUSE: u16 = 24;
pub const MINE_ARM: u16 = 30;
pub const FRAG_LIFE: u16 = 7 * HZ as u16;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub struct CrateDrop {
    pub item: Item,
    pub center: Point,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Encode, Decode)]
pub struct Equipment {
    pub queue: Vec<Item>,
    pub ammo: u8,
    pub shield_until: u64,
    pub boost_until: u64,
    pub(super) locked_until: u64,
    pub(super) ready: u64,
    pub(super) remote: Option<u64>,
    pub(super) firing: bool,
}
impl Equipment {
    pub fn weapon(&self) -> Option<Item> {
        self.queue.first().copied()
    }
    fn take(&mut self, item: Item, tick: u64) -> bool {
        match item {
            Item::Shield if self.shield_until <= tick => self.shield_until = tick + SHIELD_TICKS,
            Item::Speed if self.boost_until <= tick => self.boost_until = tick + BOOST_TICKS,
            Item::Shield | Item::Speed => return false,
            _ if self.queue.len() < 3 => {
                if self.queue.is_empty() {
                    self.ammo = item.rounds();
                }
                self.queue.push(item);
            }
            _ => return false,
        }
        true
    }
    pub(super) fn advance(&mut self) {
        self.queue.remove(0);
        self.ammo = self.weapon().map_or(0, Item::rounds);
        self.firing = false;
        self.ready = 0;
        self.remote = None;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub enum ShotKind {
    Laser,
    Double,
    Pellet,
    Missile,
    Mine,
    Gatling,
    Frag,
    Shard,
}
impl ShotKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Laser => "laser",
            Self::Double => "doubleBarrel",
            Self::Pellet => "pellet",
            Self::Missile => "homingMissile",
            Self::Mine => "mine",
            Self::Gatling => "gatling",
            Self::Frag => "fragBomb",
            Self::Shard => "shard",
        }
    }
    pub fn radius(self) -> i32 {
        (match self {
            Self::Laser => 0,
            Self::Double => 5,
            Self::Missile => 4,
            Self::Mine => 16,
            Self::Frag => 14,
            _ => 2,
        }) * PX
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Payload {
    pub kind: ShotKind,
    pub(super) age: u16,
    pub(super) fuse: Option<u16>,
    pub(super) clear_owner: bool,
}
impl Payload {
    pub fn can_hit_owner(&self) -> bool {
        self.clear_owner
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Beam {
    pub from: Point,
    pub to: Point,
    pub until: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Encode, Decode)]
pub struct Items {
    pub pool: ItemPool,
    pub crates: Vec<CrateDrop>,
    pub gear: [Equipment; MAX_SLOTS],
    pub projectiles: BTreeMap<u64, Payload>,
    pub beams: Vec<Beam>,
    rng: u64,
    next_drop: u64,
}
impl Items {
    pub fn gear(&self, id: SlotId) -> &Equipment {
        &self.gear[id.0 as usize - 1]
    }
    // SplitMix64: explicit serializable state; never consume a bot's random stream.
    pub(super) fn random(&mut self, n: u64) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        (z ^ (z >> 31)) % n
    }
    fn delay(&mut self) -> u64 {
        3 * HZ as u64 + self.random(5 * HZ as u64 + 1)
    }
}
impl World {
    pub fn items(&self) -> &Items {
        &self.items
    }
    pub fn with_items(mut self, pool: ItemPool) -> Self {
        assert_eq!(self.tick, 0, "configure crates before starting the match");
        self.items = Items::default();
        if pool != ItemPool::NONE {
            self.items.pool = pool;
            self.items.rng = self.behavior_seed.unwrap_or(0) ^ 0x435241544553;
            self.items.next_drop = self.items.delay();
        }
        self
    }
    pub(super) fn update_crates(&mut self) {
        let now = self.tick;
        self.items.beams.retain(|b| b.until > now);
        self.items.crates.retain(|c| {
            !self.tanks.iter().any(|t| {
                t.alive
                    && body(c.center, Point::default(), t, 32 * PX).is_some()
                    && self.items.gear[t.seat.slot.0 as usize - 1].take(c.item, now)
            })
        });
        if now < self.items.next_drop {
            return;
        }
        self.items.next_drop = now + self.items.delay();
        if self.items.crates.len() == MAX_CRATES {
            return;
        }
        let (w, h) = self.map.size();
        // Reservoir sample unoccupied cell centres. Prefer four tiles' clearance, with a
        // safe fallback for small maps where that distance is impossible.
        let mut choice = None;
        for clearance in [4 * CELL, CELL / 2] {
            let mut n = 0;
            for y in 0..h {
                for x in 0..w {
                    let p = Point {
                        x: x as i32 * CELL + CELL / 2,
                        y: y as i32 * CELL + CELL / 2,
                    };
                    if self.items.crates.iter().any(|c| c.center == p)
                        || self.tanks.iter().any(|t| {
                            t.alive && distance2(t.center, p) < i64::from(clearance).pow(2)
                        })
                    {
                        continue;
                    }
                    n += 1;
                    if self.items.random(n) == 0 {
                        choice = Some(p);
                    }
                }
            }
            if choice.is_some() {
                break;
            }
        }
        if let Some(center) = choice {
            let pool = self.items.pool;
            let index = self.items.random(pool.0.count_ones().into()) as usize;
            let item = Item::ALL
                .into_iter()
                .filter(|&k| pool.contains(k))
                .nth(index)
                .unwrap();
            self.items.crates.push(CrateDrop { item, center });
        }
    }
}
pub(super) fn distance2(a: Point, b: Point) -> i64 {
    i64::from(a.x - b.x).pow(2) + i64::from(a.y - b.y).pow(2)
}
