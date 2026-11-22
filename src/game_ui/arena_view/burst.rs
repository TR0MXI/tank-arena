//! 每个席位只设一个固定容量的粒子发射器，按命中后经过的时间绘制。
//! 特效不依赖渲染帧间隔，跳过快照或回放跳转都不会漏播、重播爆炸。

use super::*;
use bevy::{
    ecs::system::SystemParam,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};

const LIFETIME: f32 = 0.62;

pub(in crate::game_ui) fn tail_duration(tick: u64, hits: &[Option<u64>], hz: u32) -> f32 {
    hits.iter()
        .flatten()
        .filter_map(|hit| tick.checked_sub(*hit))
        .min()
        .map_or(0., |ticks| (LIFETIME - ticks as f32 / hz as f32).max(0.))
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
struct Debris {
    #[uniform(0)]
    tint_seed: Vec4,
    #[uniform(1)]
    phase: f32,
}

impl Material2d for Debris {
    fn fragment_shader() -> ShaderRef {
        artwork::BURST.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Component)]
struct Emitter(usize);

pub(super) fn install(app: &mut App) {
    app.add_plugins(Material2dPlugin::<Debris>::default())
        .add_systems(OnEnter(Page::Live), prepare)
        .add_systems(OnEnter(Page::Replay), prepare);
}

fn prepare(
    mut c: Commands,
    page: Res<State<Page>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Debris>>,
) {
    let quad = meshes.add(Rectangle::new(1., 1.));
    for i in 0..MAX_SLOTS {
        c.spawn((
            DespawnOnExit(*page.get()),
            Emitter(i),
            Mesh2d(quad.clone()),
            MeshMaterial2d(materials.add(Debris {
                tint_seed: Vec4::ZERO,
                phase: 0.,
            })),
            Visibility::Hidden,
        ));
    }
}

type Emitters<'w, 's> = Query<
    'w,
    's,
    (
        &'static Emitter,
        &'static MeshMaterial2d<Debris>,
        &'static mut Visibility,
        &'static mut Transform,
    ),
    (Without<Actor>, Without<annotations::Marker>),
>;

#[derive(SystemParam)]
pub(super) struct Bursts<'w, 's> {
    emitters: Emitters<'w, 's>,
    materials: ResMut<'w, Assets<Debris>>,
}

impl Bursts<'_, '_> {
    pub(super) fn paint(
        &mut self,
        map: &ArenaMap,
        tanks: &[Tank],
        hits: &[Option<u64>],
        tick: u64,
        hz: u32,
        extra: f32,
    ) {
        for (emitter, material, mut vis, mut pose) in &mut self.emitters {
            let i = emitter.0;
            let age = hits[i]
                .and_then(|t| tick.checked_sub(t))
                .map(|n| n as f32 / hz as f32 + extra);
            if let Some(age) = age.filter(|&a| a < LIFETIME) {
                if *vis == Visibility::Hidden {
                    let t = &tanks[i];
                    let color = tint(index(t.seat.team)).to_linear();
                    if let Some(mut m) = self.materials.get_mut(material) {
                        m.tint_seed = Vec4::new(
                            color.red,
                            color.green,
                            color.blue,
                            (t.seat.slot.0 % 997) as f32,
                        );
                    }
                    pose.translation = xy(t.center, map).extend(4.3);
                    pose.scale = Vec3::new(1.9 * tile(map), 1.9 * tile(map), 1.);
                    *vis = Visibility::Inherited;
                }
                let phase = age / LIFETIME;
                if self
                    .materials
                    .get(material)
                    .is_some_and(|m| m.phase != phase)
                {
                    self.materials.get_mut(material).unwrap().phase = phase;
                }
            } else {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
