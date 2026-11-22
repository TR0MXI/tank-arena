mod motion;

use super::*;
use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    image::ImageSampler,
    render::render_resource::{AsBindGroup, TextureFormat},
    shader::ShaderRef,
};

const SHADOW_ALPHA: f32 = 0.22;

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
pub(in crate::game_ui) enum Panel {
    #[default]
    Closed,
    Settings,
    Closing,
}

#[derive(Component)]
struct Overlay;

type BackdropRoots = Or<(With<Home>, With<toolbar::Toolbar>)>;

pub(super) fn install(app: &mut App) {
    motion::install(app);
    app.add_plugins(UiMaterialPlugin::<FrostedMenu>::default())
        .init_resource::<Panel>()
        .add_systems(
            Update,
            sync.after(keys)
                .run_if(in_state(Page::Menu))
                .run_if(resource_changed::<Panel>),
        )
        .add_systems(
            Update,
            locale::settings_contents
                .after(sync)
                .run_if(in_state(Page::Menu)),
        )
        .add_systems(
            OnExit(Page::Menu),
            |mut c: Commands,
             bar: Single<Entity, With<toolbar::Toolbar>>,
             mut panel: ResMut<Panel>| {
                c.entity(*bar).remove::<UiTargetCamera>();
                *panel = Panel::Closed;
            },
        );
}

fn sync(
    mut c: Commands,
    panel: Res<Panel>,
    backdrop_roots: Query<Entity, BackdropRoots>,
    window: Single<&Window>,
    old: Query<Entity, With<Overlay>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FrostedMenu>>,
) {
    if *panel == Panel::Closing {
        // 退出面板完全消失前，继续拦截指针输入。
        c.spawn((Overlay, DespawnOnExit(Page::Menu), GlobalZIndex(11), root()));
        return;
    }

    for e in &old {
        c.entity(e).despawn();
    }

    if *panel == Panel::Closed {
        for root in &backdrop_roots {
            c.entity(root).remove::<UiTargetCamera>();
        }
        return;
    }

    let layer = c
        .spawn((Overlay, DespawnOnExit(Page::Menu), modal_layer()))
        .id();

    // 背景点击区和卡片互为兄弟节点，卡片内的空白也会拦截点选，
    // 卡片控件的点击不会冒泡到背景。
    let outside = |_: On<PointerClick>, mut panel: ResMut<Panel>| {
        panel.set_if_neq(Panel::Closing);
    };
    let (camera, backdrop) = frost(
        &mut c,
        &window,
        &mut images,
        &mut materials,
        (Overlay, DespawnOnExit(Page::Menu)),
    );
    for root in &backdrop_roots {
        c.entity(root).insert(UiTargetCamera(camera));
    }

    c.entity(layer).with_children(|p| {
        p.spawn(backdrop).observe(outside);
        p.spawn((locale::SettingsCard, modal_card(420.)));
    });
}

pub(in crate::game_ui) fn modal_layer() -> impl Bundle {
    (
        GlobalZIndex(10),
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..root()
        },
    )
}

pub(in crate::game_ui) fn modal_card(width: f32) -> impl Bundle {
    (
        motion::Reveal::default(),
        Node {
            width: px(width),
            max_width: percent(92),
            padding: UiRect::all(px(28)),
            border_radius: BorderRadius::all(px(22)),
            ..column(14.)
        },
        BackgroundColor(paper()),
        BoxShadow::new(
            Color::srgba(0.04, 0.10, 0.09, SHADOW_ALPHA),
            px(0),
            px(10),
            px(0),
            px(30),
        ),
    )
}

/// 同时匹配物理像素尺寸和屏幕缩放系数，
/// 才能让图片固有尺寸、字体度量和场景投影与未遮挡的窗口一致。
pub(in crate::game_ui) fn frost(
    c: &mut Commands,
    window: &Window,
    images: &mut Assets<Image>,
    materials: &mut Assets<FrostedMenu>,
    scope: impl Bundle,
) -> (Entity, impl Bundle) {
    let mut image = Image::new_target_texture(
        window.physical_width().max(1),
        window.physical_height().max(1),
        TextureFormat::Bgra8UnormSrgb,
        None,
    );
    image.sampler = ImageSampler::linear();
    let texture = images.add(image);
    let camera = c
        .spawn((
            Camera2d,
            arena_view::projection(),
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: texture.clone(),
                scale_factor: window.scale_factor(),
            }),
            scope,
        ))
        .id();
    (
        camera,
        (
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            MaterialNode(materials.add(FrostedMenu {
                texture,
                reveal: 0.,
            })),
        ),
    )
}

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub(in crate::game_ui) struct FrostedMenu {
    #[texture(0)]
    #[sampler(1)]
    texture: Handle<Image>,
    #[uniform(2)]
    reveal: f32,
}

impl UiMaterial for FrostedMenu {
    fn fragment_shader() -> ShaderRef {
        artwork::BLUR.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keeps_home_and_only_esc_dismisses_via_keyboard() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::animation::AnimationPlugin,
        ))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(16),
        ))
        .insert_resource(State::new(Page::Menu))
        .init_resource::<NextState<Page>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<FrostedMenu>>()
        .insert_resource(Panel::Settings)
        .add_systems(
            Update,
            (keys, sync.run_if(resource_changed::<Panel>)).chain(),
        );
        motion::install(&mut app);
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(2.));
        window.resolution.set(1080., 720.);
        app.world_mut().spawn(window);
        let home = app.world_mut().spawn((Home, root())).id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let card = app
            .world_mut()
            .query_filtered::<Entity, With<locale::SettingsCard>>()
            .single(app.world())
            .unwrap();
        let blank = app.world_mut().spawn((Node::default(), ChildOf(card))).id();
        let camera = app.world().get::<UiTargetCamera>(home).unwrap().entity();
        let RenderTarget::Image(target) = app.world().get::<RenderTarget>(camera).unwrap() else {
            panic!("missing blur texture")
        };
        assert_eq!(target.scale_factor, 2.);
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&target.handle)
                .unwrap()
                .size(),
            UVec2::new(2160, 1440)
        );
        assert!(matches!(
            app.world().resource::<NextState<Page>>(),
            NextState::Unchanged
        ));
        assert_eq!(
            app.world_mut()
                .query::<&locale::SettingsCard>()
                .iter(app.world())
                .count(),
            1
        );

        for _ in 0..6 {
            app.update();
        }

        // 只改变透明度；关闭按钮的位置和大小保持不变。
        let world = app.world();
        assert_eq!(world.get::<UiTransform>(card).unwrap().scale, Vec2::ONE);
        let opacity = world.get::<BackgroundColor>(card).unwrap().0.alpha();
        assert!(opacity > 0. && opacity < 1.);
        assert_eq!(
            app.world().get::<BackgroundColor>(blank).unwrap().0.alpha(),
            0.
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();

        assert!(matches!(app.world().resource::<Panel>(), Panel::Closing));
        assert!(app.world().get::<UiTargetCamera>(home).is_some());

        // 等反向动画结束后，再移除退出的卡片和输入屏障。
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        for _ in 0..10 {
            app.update();
        }

        assert!(matches!(app.world().resource::<Panel>(), Panel::Closed));
        assert!(app.world().get::<Home>(home).is_some());
        assert!(app.world().get::<UiTargetCamera>(home).is_none());
        assert!(app.world().get_entity(camera).is_err());
        assert_eq!(
            app.world_mut()
                .query::<&Overlay>()
                .iter(app.world())
                .count(),
            0
        );
        assert!(matches!(
            app.world().resource::<NextState<Page>>(),
            NextState::Unchanged
        ));
    }
}
