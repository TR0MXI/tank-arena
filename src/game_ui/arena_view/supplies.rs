//! Pooled badge sprites for crates and equipment; one overlay for shots and effects. Pellet bursts
//! can exceed the original forty normal-shell sprites without creating per-pellet entities.
use super::*;
use bevy::{
    asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, mesh::PrimitiveTopology,
};
use tank_arena::sim::{Item, MAX_CRATES, ShotKind, laser_guide};
#[derive(Component)]
pub(super) struct Overlay;
#[derive(Component)]
pub(super) struct Badge(usize);
#[derive(Default)]
struct Drawing {
    pos: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
}
impl Drawing {
    fn quad(&mut self, pts: [Vec2; 4], col: Color) {
        let c = col.to_linear().to_f32_array();
        for i in [0, 1, 2, 0, 2, 3] {
            self.pos.push(pts[i].extend(0.).to_array());
            self.col.push(c);
        }
    }
    fn box_at(&mut self, p: Vec2, r: f32, c: Color) {
        self.quad(
            [
                p + Vec2::new(-r, -r),
                p + Vec2::new(r, -r),
                p + Vec2::new(r, r),
                p + Vec2::new(-r, r),
            ],
            c,
        );
    }
    fn line(&mut self, a: Vec2, b: Vec2, width: f32, c: Color) {
        let v = (b - a).normalize_or_zero().perp() * width * 0.5;
        self.quad([a - v, b - v, b + v, a + v], c);
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
    }
}
pub(super) fn spawn(
    p: &mut ChildSpawnerCommands,
    mesh: &mut Assets<Mesh>,
    palette: &Palette,
    badges: &Handle<Image>,
) {
    p.spawn((
        Overlay,
        NoFrustumCulling,
        Mesh2d(mesh.add(Drawing::default().mesh())),
        MeshMaterial2d(palette.white.clone()),
        Transform::from_xyz(0., 0., 4.1),
        Visibility::Hidden,
    ));
    for i in 0..MAX_CRATES + MAX_SLOTS {
        p.spawn((
            Badge(i),
            Sprite::from_image(badges.clone()),
            Visibility::Hidden,
        ));
    }
}
fn color(item: Item) -> Color {
    let (r, g, b) = match item {
        Item::Laser => (179, 52, 77),
        Item::Double => (56, 96, 153),
        Item::Shotgun => (149, 86, 47),
        Item::Missile => (115, 65, 153),
        Item::Mine => (132, 89, 32),
        Item::Gatling => (125, 67, 93),
        Item::Frag => (192, 74, 34),
        Item::Shield => (33, 128, 164),
        Item::Speed => (39, 131, 76),
    };
    Color::srgb_u8(r, g, b)
}
#[allow(clippy::type_complexity)]
pub(super) fn paint(
    source: source::Source,
    mut last: Local<Option<u64>>,
    overlay: Query<(&Mesh2d, &mut Visibility), With<Overlay>>,
    mut badges: Query<(&Badge, &mut Sprite, &mut Transform, &mut Visibility), Without<Overlay>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(s) = source.scene() else {
        return;
    };
    if !s.reset && *last == Some(s.tick) {
        return;
    }
    *last = Some(s.tick);
    let Ok((handle, mut vis)) = overlay.single_inner() else {
        return;
    };
    let mut draw = Drawing::default();
    let size = tile(s.map);
    let pos = |p| xy(p, s.map);
    let mut placements: [Option<(Vec2, Item, f32)>; MAX_CRATES + MAX_SLOTS] = Default::default();
    for (i, c) in s.items.crates.iter().enumerate() {
        placements[i] = Some((pos(c.center), c.item, (size * 0.36).clamp(14., 34.)));
    }
    for (i, t) in s.tanks.iter().enumerate().filter(|(_, t)| t.alive) {
        let p = pos(t.center);
        let g = s.items.gear(t.seat.slot);
        if g.shield_until > s.tick {
            let radius = size * 0.43;
            for j in 0..32 {
                let at = |n| p + Vec2::from_angle(n as f32 * std::f32::consts::TAU / 32.) * radius;
                draw.line(at(j), at(j + 1), 1.8, Color::srgb_u8(82, 219, 245));
            }
        }
        if let Some(k) = g.weapon() {
            let side = (size * 0.23).clamp(12., 21.);
            let b = p + Vec2::Y * (size * 0.27 + side * 0.5);
            placements[MAX_CRATES + i] = Some((b, k, side));
            if k == Item::Laser && g.ammo > 0 {
                for (a, b) in laser_guide(s.map, t, s.tanks) {
                    draw.line(pos(a), pos(b), 0.8, Color::srgba(1., 0.3, 0.4, 0.5));
                }
            }
        }
        if g.boost_until > s.tick {
            let b = p - Vec2::from_angle(-t.heading.radians() as f32) * size * 0.24;
            let f = Vec2::from_angle(-t.heading.radians() as f32) * size * 0.09;
            draw.line(b - f + f.perp(), b, 2., Color::srgb_u8(95, 235, 137));
            draw.line(b - f - f.perp(), b, 2., Color::srgb_u8(95, 235, 137));
        }
    }
    for b in &s.items.beams {
        draw.line(pos(b.from), pos(b.to), 2.8, Color::srgb_u8(255, 92, 105));
    }
    for (i, b) in s.shells.iter().enumerate() {
        let p = pos(b.center);
        if let Some(m) = s.items.projectiles.get(&b.id) {
            let k = match m.kind {
                ShotKind::Laser => Item::Laser,
                ShotKind::Double => Item::Double,
                ShotKind::Pellet => Item::Shotgun,
                ShotKind::Missile => Item::Missile,
                ShotKind::Mine => Item::Mine,
                ShotKind::Gatling => Item::Gatling,
                ShotKind::Shard | ShotKind::Frag => Item::Frag,
            };
            let r = (m.kind.radius() as f32 / CELL as f32 * size).max(1.8);
            draw.box_at(p, r + 0.7, ink());
            draw.box_at(p, r, color(k));
        } else if i >= MAX_SLOTS * usize::from(AMMO) {
            let col = s
                .tanks
                .iter()
                .find(|t| t.seat.slot == b.owner)
                .map_or_else(ink, |t| tint(index(t.seat.team)));
            draw.box_at(p, size * 0.025, col);
        }
    }
    if draw.pos.is_empty() {
        vis.set_if_neq(Visibility::Hidden);
    } else {
        vis.set_if_neq(Visibility::Inherited);
        if let Some(mut m) = meshes.get_mut(&handle.0) {
            *m = draw.mesh();
        }
    }
    for (b, mut sprite, mut tr, mut vis) in &mut badges {
        if let Some((p, item, side)) = placements[b.0] {
            let rect = Some(icons::item_rect(item));
            let extent = Some(Vec2::splat(side));
            if sprite.rect != rect || sprite.custom_size != extent {
                sprite.rect = rect;
                sprite.custom_size = extent;
            }
            tr.translation = p.extend(4.25);
            vis.set_if_neq(Visibility::Inherited);
        } else {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}
