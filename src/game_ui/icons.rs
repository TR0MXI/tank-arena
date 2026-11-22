//! 每个操作用一个不透明图标；动画交给按钮和工具栏，不改变图标本身。

use super::*;
use bevy::image::ImageSampler;
use std::collections::HashMap;

#[derive(Component, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Icon {
    Home,
    Replays,
    Maps,
    Tanks,
    Settings,
    Play,
    Pause,
    Restart,
    Step,
}

impl Icon {
    fn key(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Replays => "library",
            Self::Maps => "mapLibrary",
            Self::Tanks => "tankManager",
            Self::Settings => "settings",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Step => "step",
            Self::Restart => "replayAgain",
        }
    }

    pub(super) fn label(self) -> AccessibleLabel {
        AccessibleLabel::new(tr(self.key()))
    }
}

#[derive(Resource)]
pub(super) struct Art {
    pub(super) duel: Handle<Image>,
    icons: HashMap<Icon, Handle<Image>>,
}

impl FromWorld for Art {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>();
        let load = |path| artwork::texture(server, path, ImageSampler::linear());
        use Icon::*;

        let files = [
            (Home, artwork::HOME),
            (Replays, artwork::REPLAYS),
            (Maps, artwork::MAPS),
            (Tanks, artwork::TANK),
            (Settings, artwork::SETTINGS),
            (Play, artwork::PLAY),
            (Pause, artwork::PAUSE),
            (Step, artwork::STEP),
            (Restart, artwork::RESTART),
        ];
        Self {
            duel: load(artwork::DUEL),
            icons: files
                .into_iter()
                .map(|(icon, path)| (icon, load(path)))
                .collect(),
        }
    }
}

impl Art {
    pub(super) fn icon_button(
        &self,
        icon: Icon,
        size: f32,
        corners: BorderRadius,
        highlight: Color,
    ) -> impl Bundle {
        let image = ImageNode::new(self.icons[&icon].clone());
        (
            Button,
            icon,
            icon.label(),
            surface(Color::NONE, highlight),
            Node {
                width: px(size),
                height: px(size),
                flex_shrink: 0.,
                border_radius: corners,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![(
                image,
                Node {
                    width: px(22),
                    height: px(22),
                    ..default()
                },
                Pickable::IGNORE,
            )],
        )
    }
}

pub(super) fn sync(
    art: Res<Art>,
    prefs: Res<locale::Preferences>,
    mut commands: Commands,
    buttons: Query<(Entity, Ref<Icon>, &Children)>,
    mut images: Query<&mut ImageNode>,
) {
    for (button, icon, children) in &buttons {
        if icon.is_changed()
            && let [glyph] = **children
            && let Ok(mut image) = images.get_mut(glyph)
        {
            image.image = art.icons[&*icon].clone();
        }
        if icon.is_changed() || prefs.is_changed() {
            commands.entity(button).insert(icon.label());
        }
    }
}

#[cfg(test)]
pub(super) fn assert_face(world: &World, button: Entity, icon: Icon) {
    assert_eq!(world.get::<Icon>(button), Some(&icon));
    assert_eq!(
        world.get::<AccessibleLabel>(button).unwrap().0,
        icon.label().0
    );
    let [child] = **world.get::<Children>(button).unwrap() else {
        panic!("an icon button must have exactly one glyph");
    };

    let image = world.get::<ImageNode>(child).unwrap();
    assert_eq!(image.image, world.resource::<Art>().icons[&icon]);
    assert_eq!(image.color.alpha(), 1.);
    assert!(!world.get::<Pickable>(child).unwrap().should_block_lower);
}
