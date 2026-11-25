//! 渲染层只认资产路径。开发模式从 assets/ 监听文件，便携版用同一份清单内嵌。

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
};

pub(super) struct ArtworkPlugin;

macro_rules! artwork {
    ($($name:ident: $path:literal),+ $(,)?) => {
        $(
            pub(super) const $name: &str = if cfg!(feature = "dev-assets") {
                $path
            } else {
                concat!("embedded://tank_arena/", $path)
            };
        )+

        impl Plugin for ArtworkPlugin {
            fn build(&self, app: &mut App) {
                #[cfg(not(feature = "dev-assets"))]
                {
                    use bevy::asset::io::embedded::EmbeddedAssetRegistry;
                    use std::path::Path;

                    let embedded = app.world().resource::<EmbeddedAssetRegistry>();
                    $(
                        embedded.insert_asset(
                            Default::default(),
                            Path::new(concat!("tank_arena/", $path)),
                            &include_bytes!(concat!("../../../assets/", $path))[..],
                        );
                    )+
                }
                app.init_resource::<super::Art>();
            }
        }
    };
}

artwork! {
    HOME: "ui/home.png",
    REPLAYS: "ui/replays.png",
    MAPS: "ui/maps.png",
    TANK: "ui/tank.png",
    SETTINGS: "ui/settings.png",
    PLAY: "ui/play.png",
    PAUSE: "ui/pause.png",
    STEP: "ui/step.png",
    RESTART: "ui/restart.png",
    DUEL: "ui/duel.png",
    ITEM_BADGES: "ui/item-badges.png",

    FLOOR: "arena/floor.png",
    FRAME: "arena/frame.png",
    LIVERY: "arena/livery.png",

    BURST: "shaders/burst.wesl",
    BLUR: "shaders/blur.wesl",
}

pub(super) fn texture(
    server: &AssetServer,
    path: &'static str,
    sampler: ImageSampler,
) -> Handle<Image> {
    server
        .load_builder()
        .with_settings(move |s: &mut ImageLoaderSettings| {
            s.sampler = sampler.clone();
            s.asset_usage = RenderAssetUsages::RENDER_WORLD;
        })
        .load(path)
}

#[cfg(test)]
pub(super) mod tests;
