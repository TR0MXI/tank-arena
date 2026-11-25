mod annotations;
mod burst;
mod floor;
mod source;
mod supplies;
mod vehicle;
mod walls;

use super::*;
use bevy::camera::ScalingMode;
pub(super) use burst::tail_duration;
use tank_arena::sim::{
    AMMO, ArenaMap, CELL, Dimensions, MAX_SLOTS, Point, SIZES, Shell, Tank, TeamId, WALL_HALF,
};
pub(super) use vehicle::{
    MatchArt, VehicleImages, art_box, art_extent, pixel_image, skin_image_node,
};

#[derive(Component)]
pub(super) enum Actor {
    Tank(usize),
    Shell(usize),
}

#[derive(Resource)]
pub(super) struct Palette {
    dot: Handle<Mesh>,
    ring: Handle<Mesh>,
    white: Handle<ColorMaterial>,
    amber: Handle<ColorMaterial>,
    pub(super) vehicle: vehicle::VehicleArt,
    floor: Handle<ColorMaterial>,
    wall_face: Handle<ColorMaterial>,
    wall_edge: Handle<ColorMaterial>,
    steel: Handle<ColorMaterial>,
    colors: [Handle<ColorMaterial>; COLORS.len()],
}

const COLORS: [[u8; 3]; 8] = [
    [216, 83, 50],
    [35, 124, 166],
    [123, 83, 167],
    [35, 139, 104],
    [196, 147, 48],
    [180, 78, 135],
    [72, 145, 150],
    [80, 120, 57],
];

fn index(t: TeamId) -> usize {
    t.0.wrapping_sub(1) as usize % COLORS.len()
}

pub(super) fn team_rgb(t: TeamId) -> [u8; 3] {
    COLORS[index(t)]
}

fn tint(i: usize) -> Color {
    let [r, g, b] = COLORS[i];
    Color::srgb_u8(r, g, b)
}

pub(super) fn install(app: &mut App) {
    burst::install(app);
    vehicle::install(app);
    app.add_systems(Startup, camera).add_systems(
        PostUpdate,
        (paint, supplies::paint)
            .before(bevy::transform::TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
}

pub(super) fn camera(
    mut c: Commands,
    mut mesh: ResMut<Assets<Mesh>>,
    mut mat: ResMut<Assets<ColorMaterial>>,
    server: Res<AssetServer>,
) {
    c.insert_resource(Palette {
        dot: mesh.add(Circle::new(1.)),
        ring: mesh.add(Annulus::new(5.5, 8.)),
        white: mat.add(Color::WHITE),
        amber: mat.add(Color::srgb_u8(214, 139, 35)),
        vehicle: vehicle::VehicleArt::load(&server),
        floor: floor::material(&server, &mut mat),
        wall_face: mat.add(Color::srgb_u8(78, 96, 94)),
        wall_edge: mat.add(Color::srgb_u8(44, 62, 64)),
        steel: mat.add(ink()),
        colors: std::array::from_fn(|i| mat.add(tint(i))),
    });
    c.spawn((Camera2d, IsDefaultUiCamera, projection()));
}

pub(super) fn projection() -> Projection {
    Projection::from(OrthographicProjection {
        scaling_mode: ScalingMode::Fixed {
            width: WINDOW.0 as f32,
            height: WINDOW.1 as f32,
        },
        ..OrthographicProjection::default_2d()
    })
}

fn tile(m: &ArenaMap) -> f32 {
    let (w, h) = m.size();
    {
        let border = 2. * WALL_HALF as f32 / CELL as f32;
        (WINDOW.0 as f32 / (w as f32 + border)).min(WINDOW.1 as f32 / (h as f32 + border))
    }
}

fn xy(p: Point, m: &ArenaMap) -> Vec2 {
    let (w, h) = m.size();
    Vec2::new(
        p.x as f32 / CELL as f32 - w as f32 / 2.,
        h as f32 / 2. - p.y as f32 / CELL as f32,
    ) * tile(m)
}

pub(super) fn draw(
    mut c: Commands,
    state: Res<State<Page>>,
    field: Res<Field>,
    replay: Option<Res<player::ReplaySession>>,
    palette: Res<Palette>,
    (art, ui_art): (Res<MatchArt>, Res<Art>),
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let page = *state.get();
    let map = if page == Page::Replay {
        replay.as_ref().unwrap().player.world().map()
    } else {
        field.map.as_ref()
    };
    let size = tile(map);
    c.spawn((
        DespawnOnExit(page),
        // 等比投影；默认地图铺满窗口，其他长宽比的地图居中而不拉伸。
        Transform::default(),
        Visibility::default(),
    ))
    .with_children(|p| {
        floor::spawn(p, map, &mut meshes, &palette);
        walls::spawn(p, map, &mut meshes, &palette);
        annotations::spawn(p, &palette);
        supplies::spawn(p, &mut meshes, &palette, &ui_art.item_badges);
        for i in 0..MAX_SLOTS {
            let tr = Transform::from_scale(Vec3::new(size / CELL as f32, size / CELL as f32, 1.));
            art.0
                .get(i)
                .unwrap_or(&palette.vehicle)
                .spawn(p, i, tr, Visibility::Hidden, SIZES);
        }

        for i in 0..MAX_SLOTS * usize::from(AMMO) {
            p.spawn((
                Actor::Shell(i),
                children![(
                    Mesh2d(palette.dot.clone()),
                    MeshMaterial2d(palette.steel.clone()),
                    Transform::from_xyz(0., 0., -0.02).with_scale(Vec3::new(
                        1. / 0.82,
                        1. / 0.82,
                        1.
                    ))
                )],
                Mesh2d(palette.dot.clone()),
                MeshMaterial2d(palette.colors[0].clone()),
                Visibility::Hidden,
            ));
        }
    });
}

type Actors<'w, 's> = Query<
    'w,
    's,
    (
        &'static Actor,
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static mut Sprite>,
        Option<&'static mut MeshMaterial2d<ColorMaterial>>,
    ),
>;

fn paint(
    source: source::Source,
    mut last: Local<Option<(u64, bool, f32)>>,
    palette: Res<Palette>,
    mut actors: Actors,
    mut markers: annotations::Markers,
    mut bursts: burst::Bursts,
) {
    let Some(source::Scene {
        map,
        tanks,
        shells,
        items,
        cue,
        tick,
        hits,
        hz,
        extra,
        roster,
        reset,
    }) = source.scene()
    else {
        return;
    };

    let moved = reset || last.is_none_or(|(old, r, _)| old != tick || r != roster);
    if !moved && last.is_some_and(|(_, _, t)| t == extra) {
        return;
    }

    *last = Some((tick, roster, extra));
    bursts.paint(map, tanks, hits, tick, hz, extra);
    if !moved {
        return;
    }

    annotations::paint(map, tanks, shells, cue, roster, &mut markers);
    for (a, mut tr, mut vis, sprite, mat) in &mut actors {
        match a {
            Actor::Tank(i) => {
                let Some(t) = tanks.get(*i).filter(|_| hits[*i].is_none()) else {
                    vis.set_if_neq(Visibility::Hidden);
                    continue;
                };

                vis.set_if_neq(Visibility::Inherited);
                tr.translation = xy(t.center, map).extend(2.);
                tr.rotation = Quat::from_rotation_z(-t.heading.radians() as f32);
                if let Some(mut s) = sprite {
                    s.color = if t.alive {
                        tint(index(t.seat.team))
                    } else {
                        Color::srgb_u8(131, 136, 125)
                    };
                }
            }
            Actor::Shell(i) => {
                let Some(s) = shells
                    .get(*i)
                    .filter(|s| !items.projectiles.contains_key(&s.id))
                else {
                    vis.set_if_neq(Visibility::Hidden);
                    continue;
                };

                vis.set_if_neq(Visibility::Inherited);
                tr.translation = xy(s.center, map).extend(4.);
                let r = tile(map) * SIZES.shell_radius as f32 / CELL as f32;
                tr.scale = Vec3::new(r * 0.82, r * 0.82, 1.);
                if let Some(mut material) = mat
                    && let Some(owner) = tanks.iter().find(|t| t.seat.slot == s.owner)
                {
                    material.set_if_neq(MeshMaterial2d(
                        palette.colors[index(owner.seat.team)].clone(),
                    ));
                }
            }
        }
    }
}
