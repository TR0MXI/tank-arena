//! 在编辑器坐标系中绘制裁判碰撞体的投影；辅助线不会写入涂装。

use super::*;
use tank_arena::sim::{Dimensions, PX, SIZES};

pub(super) const CANVAS_W: f32 = 480.;

pub(super) fn canvas_size(d: Dimensions) -> Vec2 {
    let extent = arena_view::art_extent(d);
    Vec2::new(CANVAS_W, CANVAS_W * extent.y / extent.x)
}

struct Guides {
    hull: Rect,
    barrel: Rect,
    spawn: Vec2,
    shell_radius: f32,
}

impl Guides {
    fn project(d: Dimensions) -> Self {
        let center = canvas_size(d) * 0.5;
        let scale = CANVAS_W / arena_view::art_extent(d).x;
        let fixture = |offset, half_length, half_width| {
            Rect::from_center_half_size(
                center + Vec2::X * (offset as f32 * scale),
                Vec2::new(half_length as f32, half_width as f32) * scale,
            )
        };
        Self {
            hull: fixture(0, d.half_length, d.half_width),
            barrel: fixture(d.turret_offset, d.turret_half_length, d.turret_half_width),
            spawn: center + Vec2::X * (d.muzzle as f32 * scale),
            shell_radius: d.shell_radius as f32 * scale,
        }
    }
}

#[derive(Component)]
pub(super) struct Overlay;

fn mark(p: &mut ChildSpawnerCommands, rect: Rect, fill: Color, border: f32, edge: Color) {
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(rect.min.x),
            top: px(rect.min.y),
            width: px(rect.width()),
            height: px(rect.height()),
            border: UiRect::all(px(border)),
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(edge),
        Pickable::IGNORE,
    ));
}

pub(super) fn spawn(p: &mut ChildSpawnerCommands) {
    let g = Guides::project(SIZES);
    p.spawn((
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..default()
        },
        ZIndex(2),
        Pickable::IGNORE,
    ))
    .with_children(|p| {
        let blue = Color::srgb_u8(47, 137, 221);
        let orange = Color::srgb_u8(255, 150, 45);
        for r in [g.hull, g.barrel] {
            mark(p, r, Color::NONE, 2., blue);
        }

        let line = |p: &mut ChildSpawnerCommands, center: Vec2, size: Vec2| {
            mark(
                p,
                Rect::from_center_size(center, size),
                orange,
                0.,
                Color::NONE,
            );
        };

        // 竖向标尺标注炮弹生成点处的弹径，不是更宽的炮口直径。
        line(p, g.spawn, Vec2::new(2., 2. * g.shell_radius));
        for dy in [-g.shell_radius, g.shell_radius] {
            line(p, g.spawn + Vec2::Y * dy, Vec2::new(10., 2.));
        }

        let end = CANVAS_W - 4.;
        line(
            p,
            Vec2::new((g.spawn.x + end) * 0.5, g.spawn.y),
            Vec2::new(end - g.spawn.x, 3.),
        );
        for i in 1..=5 {
            for sign in [-1., 1.] {
                line(
                    p,
                    Vec2::new(end - i as f32, g.spawn.y + sign * i as f32),
                    Vec2::splat(3.),
                );
            }
        }
    });
}

pub(super) fn legend() -> String {
    rust_i18n::t!(
        "guideLegend",
        barrel = 2 * SIZES.turret_half_width / PX,
        shell = 2 * SIZES.shell_radius / PX
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_projects_the_same_centered_frame_as_the_world_sprite() {
        let g = Guides::project(SIZES);
        assert_eq!(canvas_size(SIZES), Vec2::new(480., 288.));
        assert_eq!(g.hull, Rect::new(80., 24., 400., 264.));
        assert_eq!(g.barrel, Rect::new(344., 116., 456., 172.));
        assert_eq!(g.spawn, Vec2::new(440., 144.));
        assert_eq!(g.shell_radius, 20.);
    }
}
