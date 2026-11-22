use super::*;
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use tank_arena::sim::{CELL, Heading, Point, SIZES, WALL_HALF, World as MatchWorld};

pub(super) fn replay(world: &MatchWorld) -> Image {
    let (w, h) = world.map().size();
    image(
        world.map(),
        [520, 216],
        (496.0 / w as f32).min(196.0 / h as f32),
        [32, 52, 53],
        world
            .tanks()
            .iter()
            .filter(|t| t.alive)
            .map(|t| (t.center, t.heading, arena_view::team_rgb(t.seat.team))),
        world.shells().iter().map(|s| s.center),
    )
}

pub(super) fn thumbnail(map: &ArenaMap) -> Image {
    let (w, h) = map.size();
    image(
        map,
        [520, 216],
        (520.0 / w as f32).min(216.0 / h as f32),
        [230, 235, 224],
        map.spawns()
            .iter()
            .map(|s| (s.center, s.heading, [133, 155, 129])),
        [],
    )
}

/// 使用地图的实际边界，不在部署纹理中预留黑边。
pub(super) fn deployment(
    map: &ArenaMap,
    tanks: impl IntoIterator<Item = (Point, Heading, [u8; 3])>,
) -> Image {
    let (w, h) = map.size();
    let cell = (720 / w.max(h)).min(48);
    image(map, [w * cell, h * cell], cell as f32, [0; 3], tanks, [])
}

fn image(
    map: &ArenaMap,
    [width, height]: [usize; 2],
    cell: f32,
    gutter: [u8; 3],
    tanks: impl IntoIterator<Item = (Point, Heading, [u8; 3])>,
    shells: impl IntoIterator<Item = Point>,
) -> Image {
    let (w, h) = map.size();
    let border = 2. * WALL_HALF as f32 / CELL as f32;
    let cell = cell
        .min(width as f32 / (w as f32 + border))
        .min(height as f32 / (h as f32 + border));
    let scale = cell / CELL as f32;
    let origin =
        (Vec2::new(width as f32, height as f32) - Vec2::new(w as f32, h as f32) * cell) * 0.5;
    let xy = |p: Point| origin + Vec2::new(p.x as f32, p.y as f32) * scale;
    let mut data = vec![0u8; width * height * 4];
    for (i, p) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let u = (Vec2::new((i % width) as f32, (i / width) as f32) - origin) / cell;
        let col = if u.x < 0. || u.y < 0. || u.x >= w as f32 || u.y >= h as f32 {
            gutter
        } else if (u.x as i32 + u.y as i32) % 2 == 0 {
            [211, 219, 200]
        } else {
            [218, 225, 207]
        };
        p.copy_from_slice(&[col[0], col[1], col[2], 255]);
    }

    let mut rect = |a: Vec2, b: Vec2, col: [u8; 3]| {
        for y in (a.y.floor() as isize).clamp(0, height as isize)
            ..(b.y.ceil() as isize).clamp(0, height as isize)
        {
            for x in (a.x.floor() as isize).clamp(0, width as isize)
                ..(b.x.ceil() as isize).clamp(0, width as isize)
            {
                let i = (y as usize * width + x as usize) * 4;
                data[i..i + 3].copy_from_slice(&col);
            }
        }
    };
    for wall in map.wall_rectangles() {
        rect(xy(wall.min), xy(wall.max), [74, 93, 90]);
    }

    for (center, heading, color) in tanks {
        let p = xy(center);
        let dir = Vec2::from_angle(heading.radians() as f32);
        let r = SIZES.half_width as f32 * scale;
        let tip = (SIZES.turret_offset + SIZES.turret_half_length) as f32 * scale;
        for y in -(r * 2.).ceil() as i32..=(r * 2.).ceil() as i32 {
            for x in -(r * 2.).ceil() as i32..=(r * 2.).ceil() as i32 {
                let v = Vec2::new(x as f32, y as f32);
                let along = v.dot(dir);
                let across = v.perp_dot(dir).abs();
                let c = if along >= (SIZES.turret_offset - SIZES.turret_half_length) as f32 * scale
                    && along <= tip
                    && across <= SIZES.turret_half_width as f32 * scale
                {
                    [33, 47, 46]
                } else if along.abs() <= SIZES.half_length as f32 * scale && across < r {
                    if across > r * 0.70 {
                        [53, 64, 61]
                    } else {
                        color
                    }
                } else {
                    continue;
                };
                let a = p.floor() + v;
                rect(a, a + Vec2::ONE, c);
            }
        }
    }

    for shell in shells {
        let p = xy(shell);
        rect(
            p - Vec2::splat(SIZES.shell_radius as f32 * scale),
            p + Vec2::splat(SIZES.shell_radius as f32 * scale),
            [201, 117, 39],
        );
    }

    Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
