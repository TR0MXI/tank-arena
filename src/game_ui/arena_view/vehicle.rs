use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use tank_arena::appearance::{HEIGHT, PixelSkin, WIDTH, rgba};

#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::game_ui) struct VehicleImages<'w> {
    palette: Res<'w, Palette>,
    pub(in crate::game_ui) images: ResMut<'w, Assets<Image>>,
}

impl VehicleImages<'_> {
    pub(in crate::game_ui) fn model(&mut self, skin: Option<&PixelSkin>) -> VehicleArt {
        skin.map_or_else(
            || self.palette.vehicle.clone(),
            |s| VehicleArt::pixel(s, &mut self.images),
        )
    }

    pub(in crate::game_ui) fn for_roster(
        &mut self,
        slots: impl Iterator<Item = tank_arena::sim::SlotId>,
        skins: &tank_arena::replay::SeatSkins,
    ) -> MatchArt {
        // 同局同款涂装只上传一次；缓存随本局名单释放，不跨局累计。
        let mut models = Vec::<(&PixelSkin, VehicleArt)>::new();
        MatchArt(
            slots
                .map(|slot| {
                    let Some(skin) = skins[slot.0 as usize - 1].as_ref() else {
                        return self.palette.vehicle.clone();
                    };
                    if let Some((_, art)) = models.iter().find(|(other, _)| *other == skin) {
                        return art.clone();
                    }

                    let art = self.model(Some(skin));
                    models.push((skin, art.clone()));
                    art
                })
                .collect(),
        )
    }
}

/// 赛场精灵和车库辅助线共用以车身中心为原点的画框。
pub(in crate::game_ui) fn art_extent(d: Dimensions) -> Vec2 {
    Vec2::new(d.half_length as f32 * 3., d.half_width as f32 * 2.4)
}

/// 将像素纹理拉伸到与赛场精灵相同的车身画框。
pub(in crate::game_ui) fn skin_image_node(image: Handle<Image>) -> ImageNode {
    ImageNode::new(image).with_mode(bevy::ui::widget::NodeImageMode::Stretch)
}

pub(in crate::game_ui) fn art_box(width: f32) -> Node {
    let extent = art_extent(SIZES);
    Node {
        width: px(width),
        height: px(width * extent.y / extent.x),
        ..default()
    }
}

pub(in crate::game_ui) fn pixel_image(w: usize, h: usize, data: Vec<u8>) -> Image {
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

#[derive(Component)]
struct Layer(usize, bool); // 名单中的位置、是否为队色图层

#[derive(Resource, Default)]
pub(in crate::game_ui) struct MatchArt(pub Vec<VehicleArt>);

#[derive(Clone)]
pub(in crate::game_ui) struct VehicleArt {
    frame: Handle<Image>,
    livery: Handle<Image>,
}

impl VehicleArt {
    fn pixel(skin: &PixelSkin, images: &mut Assets<Image>) -> Self {
        let mut layer = |team| {
            images.add(pixel_image(
                WIDTH,
                HEIGHT,
                skin.cells()
                    .iter()
                    .flat_map(|&c| {
                        if (c == b'T') == team {
                            rgba(c, [255; 3])
                        } else {
                            [0; 4]
                        }
                    })
                    .collect(),
            ))
        };
        Self {
            frame: layer(false),
            livery: layer(true),
        }
    }

    pub(super) fn load(server: &AssetServer) -> Self {
        let load = |path| artwork::texture(server, path, ImageSampler::linear());
        Self {
            frame: load(artwork::FRAME),
            livery: load(artwork::LIVERY),
        }
    }

    pub(in crate::game_ui) fn icon(&self, color: Color) -> impl Bundle {
        let layer = || Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        };
        (
            art_box(54.),
            Pickable::IGNORE,
            children![
                (
                    layer(),
                    skin_image_node(self.frame.clone()),
                    Pickable::IGNORE
                ),
                (
                    layer(),
                    ImageNode {
                        color,
                        ..skin_image_node(self.livery.clone())
                    },
                    Pickable::IGNORE
                ),
            ],
        )
    }

    pub(super) fn spawn(
        &self,
        p: &mut ChildSpawnerCommands,
        i: usize,
        tr: Transform,
        vis: Visibility,
        dimensions: Dimensions,
    ) {
        // 画框为 120 × 72 逻辑像素，和碰撞体一样以车身中心为原点。
        let size = art_extent(dimensions);
        let sprite = |image, color| Sprite {
            image,
            custom_size: Some(size),
            color,
            ..default()
        };
        p.spawn((
            Actor::Tank(i),
            Layer(i, true),
            tr,
            vis,
            sprite(self.livery.clone(), tint(i % 2)),
            children![(
                Layer(i, false),
                sprite(self.frame.clone(), Color::WHITE),
                Transform::from_xyz(0., 0., -0.05)
            )],
        ));
    }
}

// 再战可能不会重新进入实况页面，此时也要刷新两个图层。
pub(super) fn install(app: &mut App) {
    fn release(mut art: ResMut<MatchArt>) {
        art.0.clear();
    }
    app.init_resource::<MatchArt>()
        .add_systems(OnExit(Page::Live), release)
        .add_systems(OnExit(Page::Replay), release)
        .add_systems(
            PostUpdate,
            (|art: Res<MatchArt>,
              palette: Res<Palette>,
              mut sprites: Query<(&Layer, &mut Sprite)>| {
                for (layer, mut sprite) in &mut sprites {
                    let model = art.0.get(layer.0).unwrap_or(&palette.vehicle);
                    sprite.image = if layer.1 {
                        model.livery.clone()
                    } else {
                        model.frame.clone()
                    };
                }
            })
            .run_if(resource_changed::<MatchArt>),
        );
}
