//! 参数取自 Tank Trouble Online RELEASE-2026-11-19-01，长度使用逻辑像素而非屏幕像素。
//! 定点比例可精确表示各轴向速度。

/// 输入、裁判和回放使用同一固定步频。
pub const HZ: u32 = 60;
pub const PX: i32 = 1200;
pub const CELL: i32 = 200 * PX;
pub const WALL_HALF: i32 = 8 * PX;
pub const FORWARD_SPEED: i32 = 319 * PX / HZ as i32;
pub const REVERSE_SPEED: i32 = 256 * PX / HZ as i32;
pub const TURN_RAD_S: f64 = 5.;
pub const AMMO: u8 = 5;
pub const LIFE: u16 = 10 * HZ as u16;
pub const BOUNCE_WINDOW_MS: u32 = 35;
pub const BOUNCE_LIMIT: usize = 5;

#[derive(Clone, Copy)]
pub struct Dimensions {
    pub half_width: i32,
    pub half_length: i32,
    pub turret_half_width: i32,
    pub turret_half_length: i32,
    pub turret_offset: i32,
    pub shell_radius: i32,
    pub muzzle: i32,
    pub shell_speed: i32,
}

pub const SIZES: Dimensions = Dimensions {
    half_width: 30 * PX,
    half_length: 40 * PX,
    turret_half_width: 7 * PX,
    turret_half_length: 14 * PX,
    turret_offset: 40 * PX,
    shell_radius: 5 * PX,
    muzzle: 50 * PX,
    shell_speed: 360 * PX / HZ as i32,
};

// 长、宽分量之和是旋转后任一轴上投影的上界，车身和前伸炮塔都必须包含在内。
pub const TANK_REACH: i32 = {
    let hull = SIZES.half_length + SIZES.half_width;
    let turret = SIZES.turret_offset + SIZES.turret_half_length + SIZES.turret_half_width;
    if hull > turret { hull } else { turret }
};
