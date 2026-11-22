mod arena_view;
mod artwork;
mod battle;
mod field;
mod icons;
mod locale;
mod map_library;
mod match_text;
mod menu;
mod player;
mod setup;
mod shelves;
mod tactical_map;
mod tank_manager;
mod theme;

use bevy::{
    picking::hover::Hovered,
    prelude::*,
    time::common_conditions::on_real_timer,
    ui_widgets::{Activate, Button, observe},
};
use field::Field;
use icons::{Art, Icon};
use locale::tr;
use std::{path::PathBuf, time::Duration};
#[cfg(test)]
use tank_arena::sim::SWITCHYARD;
use tank_arena::{
    local_files::{self, Folder},
    sim::{ArenaMap, HZ},
};
use theme::*;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Page {
    #[default]
    Menu,
    Setup,
    Live,
    Library,
    Maps,
    Tanks,
    Loading,
    Replay,
}

const RESCAN: Duration = Duration::from_millis(500);
const WINDOW: (u32, u32) = (1080, 720);

pub(super) fn run() {
    if let Err(e) = local_files::prepare() {
        eprintln!("Content directories: {e}");
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Tank Arena".into(),
            resolution: WINDOW.into(),
            resizable: false,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb_u8(15, 28, 34)))
    .init_resource::<Field>()
    .add_plugins(artwork::ArtworkPlugin)
    .init_state::<Page>()
    .add_systems(Update, (hover, keys))
    .add_systems(
        PostUpdate,
        (icons::sync, page_intro).before(bevy::ui::UiSystems::Layout),
    );
    app.add_plugins((
        arena_view::install,
        locale::install,
        menu::install,
        setup::install,
        tank_manager::install,
        map_library::install,
        battle::install,
        player::install,
    ));
    app.run();
}

fn navigate(page: Page) -> impl Bundle {
    observe(move |_: On<Activate>, mut next: ResMut<NextState<Page>>| next.set(page))
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    page: Res<State<Page>>,
    mut panel: ResMut<menu::Panel>,
    mut next: ResMut<NextState<Page>>,
) {
    if *panel != menu::Panel::Closed {
        if keys.just_pressed(KeyCode::Escape) {
            panel.set_if_neq(menu::Panel::Closing);
        }
        return;
    }

    if keys.just_pressed(KeyCode::Escape)
        && !matches!(page.get(), Page::Live | Page::Replay | Page::Setup)
    {
        next.set(Page::Menu);
    } else if keys.just_pressed(KeyCode::Enter) && *page.get() == Page::Menu {
        next.set(Page::Setup);
    }
}
