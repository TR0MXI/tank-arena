use super::*;
use bevy::{
    image::{CompressedImageFormats, ImageLoader},
    shader::ShaderLoader,
};
use std::time::{Duration, Instant};

fn app_with_assets(plugin: AssetPlugin) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, plugin))
        .init_asset::<Image>()
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE))
        .init_asset::<Shader>()
        .init_asset_loader::<ShaderLoader>()
        .add_plugins(ArtworkPlugin);
    app
}

pub(in crate::game_ui) fn headless() -> App {
    app_with_assets(AssetPlugin {
        watch_for_changes_override: Some(false),
        ..default()
    })
}

fn wait_for(app: &mut App, ready: impl Fn(&World) -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        app.update();
        if ready(app.world()) {
            return;
        }
        assert!(Instant::now() < end, "asset was not loaded or refreshed");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn artwork_catalog_loads_images_and_both_material_shaders() {
    #[cfg(not(feature = "dev-assets"))]
    let mut app = {
        let empty = tempfile::tempdir().unwrap();
        app_with_assets(AssetPlugin {
            file_path: empty.path().to_str().unwrap().into(),
            ..default()
        })
    };
    #[cfg(feature = "dev-assets")]
    let mut app = headless();
    let server = app.world().resource::<AssetServer>();
    let images = [
        HOME, REPLAYS, MAPS, TANK, SETTINGS, PLAY, PAUSE, STEP, RESTART, DUEL, FLOOR, FRAME, LIVERY,
    ]
    .map(|path| server.load::<Image>(path));
    let shaders = [BLUR, BURST].map(|path| server.load::<Shader>(path));
    wait_for(&mut app, |w| {
        images
            .iter()
            .all(|h| w.resource::<Assets<Image>>().contains(h))
            && shaders
                .iter()
                .all(|h| w.resource::<Assets<Shader>>().contains(h))
    });
}

#[cfg(feature = "dev-assets")]
#[test]
fn watcher_updates_an_existing_icon_and_shader_handle() {
    use std::{fs, path::Path};

    const MARKER: &str = "\n// 文件监听测试\n";

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let root = tempfile::tempdir().unwrap();
    for folder in ["ui", "shaders"] {
        fs::create_dir(root.path().join(folder)).unwrap();
        for item in fs::read_dir(source.join(folder)).unwrap() {
            let file = item.unwrap();
            fs::copy(file.path(), root.path().join(folder).join(file.file_name())).unwrap();
        }
    }

    let mut app = app_with_assets(AssetPlugin {
        // macOS 通过实际路径 /private/var 报告文件变动，而不是通过符号链接 /var。
        file_path: root.path().canonicalize().unwrap().to_str().unwrap().into(),
        ..default()
    });
    let server = app.world().resource::<AssetServer>();
    let icon = server.load::<Image>(HOME);
    let blur = server.load::<Shader>(BLUR);
    wait_for(&mut app, |w| {
        w.resource::<Assets<Image>>().contains(&icon)
            && w.resource::<Assets<Shader>>().contains(&blur)
    });

    let original = app
        .world()
        .resource::<Assets<Image>>()
        .get(&icon)
        .unwrap()
        .size();

    fs::copy(source.join(DUEL), root.path().join(HOME)).unwrap();
    let shader_path = root.path().join(BLUR);
    let mut code = fs::read_to_string(&shader_path).unwrap();
    code.push_str(MARKER);
    fs::write(shader_path, code).unwrap();

    wait_for(&mut app, |w| {
        w.resource::<Assets<Image>>()
            .get(&icon)
            .is_some_and(|image| image.size() != original)
            && w.resource::<Assets<Shader>>()
                .get(&blur)
                .is_some_and(|shader| shader.source.as_str().contains(MARKER))
    });
}
