//! 宿主维护一份只读观察表，脚本拿到的容器和行引用在多次决策之间保持有效。

use super::BotConfig;
use crate::sim::{
    AMMO, CELL, Drive, FORWARD_SPEED, HZ, Heading, Items, LIFE, MAX_CRATES, MAX_SLOTS,
    REVERSE_SPEED, SIZES, Shell, Sight, SlotId, Tank, Turn, WALL_HALF,
};
use mlua::{Lua, Result, Table};

/// 策略坐标以 1/1024 格为单位；速度表示每个物理步的位移。
fn units(n: i32) -> f64 {
    f64::from(n) * 1024.0 / f64::from(CELL)
}

/// 所有行及容器都由宿主持有；清掉尾部索引但不丢掉行的分配。
struct Rows {
    lua: Lua,
    fields: usize,
    table: Table,
    pool: Vec<Table>,
    len: usize,
}

impl Rows {
    fn new(lua: &Lua, cap: usize, fields: usize) -> Result<Self> {
        let pool = (0..cap)
            .map(|_| lua.create_table_with_capacity(0, fields))
            .collect::<Result<_>>()?;
        let table = lua.create_table_with_capacity(cap, 0)?;
        table.set_readonly(true);

        Ok(Self {
            lua: lua.clone(),
            fields,
            table,
            pool,
            len: 0,
        })
    }

    fn update<T>(&mut self, items: &[T], write: impl Fn(&Table, &T) -> Result<()>) -> Result<()> {
        while self.pool.len() < items.len() {
            self.pool
                .push(self.lua.create_table_with_capacity(0, self.fields)?);
        }
        if items.len() != self.len {
            self.table.set_readonly(false);
            for i in self.len..items.len() {
                self.table.raw_set(i + 1, &self.pool[i])?;
            }

            for i in items.len()..self.len {
                self.table.raw_set(i + 1, mlua::Nil)?;
            }

            self.table.set_readonly(true);
            self.len = items.len();
        }

        for (row, item) in self.pool.iter().zip(items) {
            row.set_readonly(false);
            write(row, item)?;
            row.set_readonly(true);
        }

        Ok(())
    }
}

pub(super) struct Observation {
    pub(super) slot: SlotId,
    table: Table,
    tanks: Rows,
    shells: Rows,
    crates: Rows,
}

impl Observation {
    pub(super) fn new(lua: &Lua, config: &BotConfig<'_>) -> Result<Self> {
        let table = lua.create_table_with_capacity(0, 32)?;
        let tanks = Rows::new(lua, MAX_SLOTS, 6)?;
        let shells = Rows::new(lua, MAX_SLOTS * usize::from(AMMO), 8)?;

        let crates = Rows::new(lua, MAX_CRATES, 3)?;
        table.raw_set("crates", &crates.table)?;
        // 子表只绑定一次；后续只更新行内容和可见长度。
        table.raw_set("tanks", &tanks.table)?;
        table.raw_set("shells", &shells.table)?;
        Self::configure(&table, config)?;
        Ok(Self {
            slot: config.seat.slot,
            table,
            tanks,
            shells,
            crates,
        })
    }

    fn configure(table: &Table, config: &BotConfig<'_>) -> Result<()> {
        let BotConfig {
            map,
            clock,
            seat,
            behavior_seed,
        } = config;
        table.raw_set("behavior_seed", *behavior_seed)?;
        for (key, value) in [
            ("shell_speed", SIZES.shell_speed),
            ("hit_radius", SIZES.half_width + SIZES.shell_radius),
            ("forward_speed", FORWARD_SPEED),
            ("reverse_speed", REVERSE_SPEED),
            ("wall_half", WALL_HALF),
            ("hull_half_width", SIZES.half_width),
            ("hull_half_length", SIZES.half_length),
            ("shell_radius", SIZES.shell_radius),
            ("muzzle", SIZES.muzzle),
            ("turret_half_width", SIZES.turret_half_width),
            ("turret_half_length", SIZES.turret_half_length),
            ("turret_offset", SIZES.turret_offset),
            ("cell", CELL),
        ] {
            table.raw_set(key, units(value))?;
        }

        table.raw_set("turn_step", Heading(Heading::STEP).bearing_units())?;
        table.raw_set("ammo_limit", AMMO)?;
        table.raw_set("shell_life", LIFE)?;
        table.raw_set("maze", map.source())?;
        table.raw_set("tick_hz", HZ)?;
        table.raw_set("time_limit_secs", clock.seconds())?;
        table.raw_set("slot_id", seat.slot.0)?;
        table.raw_set("team_id", seat.team.0)
    }

    pub(super) fn update(
        &mut self,
        tick: u64,
        sight: Sight,
        tanks: &[Tank],
        shells: &[Shell],
        items: &Items,
    ) -> Result<&Table> {
        let table = &self.table;
        table.set_readonly(false);
        self.tanks.update(tanks, |row, tank| {
            if tank.seat.slot == self.slot {
                table.raw_set("me", row)?;
            }

            row.raw_set("slot_id", tank.seat.slot.0)?;
            row.raw_set("team_id", tank.seat.team.0)?;
            row.raw_set("alive", tank.alive)?;
            row.raw_set("x", units(tank.center.x))?;
            row.raw_set("y", units(tank.center.y))?;
            let gear = items.gear(tank.seat.slot);
            row.raw_set("weapon", gear.weapon().map(|k| k.key()))?;
            row.raw_set("shield_ticks", gear.shield_until.saturating_sub(tick))?;
            row.raw_set("speed_ticks", gear.boost_until.saturating_sub(tick))?;
            row.raw_set("heading", tank.heading.bearing_units())
        })?;

        self.crates.update(&items.crates, |row, c| {
            row.raw_set("kind", c.item.key())?;
            row.raw_set("x", units(c.center.x))?;
            row.raw_set("y", units(c.center.y))
        })?;
        self.shells.update(shells, |row, shell| {
            row.raw_set("id", shell.id)?;
            row.raw_set("owner", shell.owner.0)?;
            row.raw_set("x", units(shell.center.x))?;
            row.raw_set("y", units(shell.center.y))?;
            row.raw_set("vx", units(shell.velocity.x))?;
            row.raw_set("vy", units(shell.velocity.y))?;
            let p = items.projectiles.get(&shell.id);
            row.raw_set("kind", p.map_or("bullet", |p| p.kind.key()))?;
            row.raw_set(
                "deadly_to_owner",
                shell.deadly_to_owner() || p.is_some_and(|p| p.can_hit_owner()),
            )?;
            row.raw_set("life", shell.life)
        })?;

        table.raw_set("weapon_slots_used", items.gear(self.slot).queue.len())?;
        table.raw_set("ammo", sight.ammo)?;
        table.raw_set("tick", tick)?;
        table.raw_set("enemy_ahead", sight.enemy_ahead)?;
        table.raw_set("can_fire", sight.can_fire)?;
        table.raw_set("fire_down", sight.fire_down)?;
        table.raw_set(
            "drive",
            match sight.drive {
                Drive::Stop => "stop",
                Drive::Forward => "forward",
                Drive::Reverse => "reverse",
            },
        )?;
        table.raw_set(
            "turn",
            match sight.turn {
                Turn::None => "none",
                Turn::Left => "left",
                Turn::Right => "right",
            },
        )?;
        table.set_readonly(true);
        Ok(table)
    }
}
