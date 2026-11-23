mod board;
mod draft;
mod grouping;
mod hangar;
mod items;
mod profiles;
mod team_scroll;

use super::*;
use bevy::input::keyboard::KeyboardInput;
use draft::*;
use profiles::Profiles;
use std::sync::Arc;
use tank_arena::{
    bot::Script,
    replay::Recorder,
    runner::{Entrant, LiveMatch, StartError},
    sim::*,
};

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
enum Section {
    #[default]
    Rules,
    Lineup,
}

#[derive(Component)]
struct Sheet;

pub(super) fn install(app: &mut App) {
    app.init_resource::<Draft>()
        .init_resource::<board::Preview>()
        .init_resource::<Profiles>()
        .init_resource::<Section>()
        .init_resource::<team_scroll::TeamScroll>()
        .add_systems(OnEnter(Page::Setup), (field::refresh, refresh_models))
        .add_systems(
            OnExit(Page::Setup),
            |mut section: ResMut<Section>, mut d: ResMut<Draft>| {
                *section = Section::Rules;
                d.confirm_team = None;
            },
        )
        // 从赛果页选择「编辑阵容」返回时，保留当前部署。
        .add_systems(
            OnTransition {
                exited: Page::Live,
                entered: Page::Setup,
            },
            |mut section: ResMut<Section>| *section = Section::Lineup,
        )
        .add_systems(
            Update,
            (
                (field::refresh, refresh_models).run_if(on_real_timer(RESCAN)),
                back,
                typing,
                board::preview,
                profiles::refresh.run_if(resource_equals(Section::Lineup)),
                board::render,
                hangar::sync,
                hangar::update_destination,
                hangar::update_tips,
                team_scroll::animate.run_if(resource_equals(Section::Lineup)),
            )
                .chain()
                .run_if(in_state(Page::Setup)),
        );
}

fn next_step(_: On<Activate>, d: Res<Draft>, mut section: ResMut<Section>) {
    if *section == Section::Rules && d.clock().is_ok() {
        *section = Section::Lineup;
    }
}

fn back(
    keys: Res<ButtonInput<KeyCode>>,
    mut section: ResMut<Section>,
    mut d: ResMut<Draft>,
    mut page: ResMut<NextState<Page>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if d.confirm_team.is_some() {
            d.confirm_team = None;
        } else if *section == Section::Lineup {
            *section = Section::Rules;
        } else {
            page.set(Page::Menu);
        }
    }
}

fn typing(mut keys: MessageReader<KeyboardInput>, section: Res<Section>, mut d: ResMut<Draft>) {
    for e in keys.read() {
        if *section != Section::Rules || d.limit != LimitMode::Custom || !e.state.is_pressed() {
            continue;
        }

        if e.key_code == KeyCode::Backspace {
            d.seconds.pop();
        } else if let Some(s) = &e.text
            && s.bytes().all(|c| c.is_ascii_digit())
            && d.seconds.len() + s.len() <= 20
        {
            d.seconds.push_str(s);
        }
    }
}

fn edit(label: &str, f: impl Fn(&mut Draft) + Send + Sync + 'static) -> impl Bundle {
    (
        compact(label),
        observe(move |_: On<Activate>, mut draft: ResMut<Draft>| f(&mut draft)),
    )
}

pub(super) fn launch() -> impl Bundle {
    observe(start)
}

fn start(
    _: On<Activate>,
    mut d: ResMut<Draft>,
    field: Res<Field>,
    mut game: ResMut<battle::LiveSession>,
    (current, mut page): (Res<State<Page>>, ResMut<NextState<Page>>),
    (looks, prefs): (Res<tank_manager::Looks>, Res<locale::Preferences>),
    (mut images, mut art): (arena_view::VehicleImages, ResMut<arena_view::MatchArt>),
) {
    // 两次激活可能在同一帧到达；不重复创建 VM 或录制文件。
    if game.running() {
        return;
    }

    // 两次轮询之间文件可能消失，包括上一场比赛进行期间。
    let previous_count = d.seats().count();
    sync_models(&mut d);
    let Ok((clock, seats)) = d.config(&field.map) else {
        if *current.get() == Page::Live {
            page.set(Page::Setup);
        }
        return;
    };
    if previous_count != seats.len() {
        // 显示更新后的阵容，不要悄悄减员后直接开赛。
        if *current.get() == Page::Live {
            page.set(Page::Setup);
        }
        return;
    }

    let launch = || -> Result<_, String> {
        // 同一车型的所有席位共享一份开局源码快照，但不共享 VM。
        let mut snapshots = std::collections::BTreeMap::new();
        let bots: Vec<_> = seats
            .into_iter()
            .map(|seat| {
                let path = &d.tank(seat.slot).unwrap().model;
                let first = snapshots
                    .entry(path)
                    .or_insert_with(|| Entrant::from_file(seat, path));
                Entrant {
                    seat,
                    ..first.clone()
                }
            })
            .collect();
        let names = bots.iter().map(|b| b.name.clone()).collect();
        let dir = tempfile::tempdir().map_err(|_| tr("missingDir"))?;
        let mut skins: tank_arena::replay::SeatSkins = Default::default();
        for bot in &bots {
            skins[bot.seat.slot.0 as usize - 1] = looks.resolve(
                &d.tank(bot.seat.slot).unwrap().model,
                bot.script.as_ref().ok().map(Script::text),
            );
        }

        let models = images.for_roster(bots.iter().map(|b| b.seat.slot), &skins);
        let (rec, status) = Recorder::with_skins(dir.path(), skins);
        let live = LiveMatch::start_with_items(field.map.clone(), clock, bots, prefs.items, rec)
            .map_err(|e| match e {
                StartError::Configuration(e) => draft::config_error(e),
                StartError::Thread(_) => tr("workerLost"),
            })?;
        Ok((live, names, status, clock, dir, models))
    };
    match launch() {
        Ok((live, names, status, clock, dir, models)) => {
            *art = models;
            game.start(live, names, status, dir, clock);
            if *current.get() != Page::Live {
                page.set(Page::Live);
            }
        }
        Err(e) => {
            d.note = e;
            if *current.get() == Page::Live {
                page.set(Page::Setup);
            }
        }
    }
}

fn toggle(
    title: &str,
    selected: bool,
    f: impl Fn(&mut Draft) + Send + Sync + 'static,
) -> impl Bundle {
    (
        choice(title, selected),
        observe(move |_: On<Activate>, mut d: ResMut<Draft>| f(&mut d)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, keyboard::Key};

    #[test]
    fn map_step_precedes_roster_validation_and_back_preserves_both_steps() {
        let mut app = App::new();
        app.init_resource::<Section>()
            .init_resource::<Draft>()
            .init_resource::<NextState<Page>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, back);
        let next = app.world_mut().spawn(observe(next_step)).id();
        let w = app.world_mut();
        assert!(matches!(*w.resource::<Section>(), Section::Rules));
        w.resource_mut::<Draft>().limit = LimitMode::Custom;
        w.trigger(Activate { entity: next });

        assert!(matches!(*w.resource::<Section>(), Section::Rules));
        w.resource_mut::<Draft>().seconds = "90".into();
        w.trigger(Activate { entity: next });

        assert!(matches!(*w.resource::<Section>(), Section::Lineup)); // 即使阵容为空也一样。
        w.resource_mut::<Draft>().add_team();
        w.resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let w = app.world_mut();
        assert!(matches!(*w.resource::<Section>(), Section::Rules));
        assert_eq!(w.resource::<Draft>().seconds, "90");
        assert_eq!(w.resource::<Draft>().selected_team, TeamId(3));
        assert!(matches!(
            *w.resource::<NextState<Page>>(),
            NextState::Unchanged
        ));
    }

    #[test]
    fn switching_sections_keeps_the_draft_and_hidden_clock_ignores_typing() {
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .insert_resource(Section::Rules)
            .init_resource::<Draft>()
            .add_systems(Update, typing);
        app.world_mut().resource_mut::<Draft>().limit = LimitMode::Custom;
        let type_digit = |app: &mut App| {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Digit5,
                logical_key: Key::Character("5".into()),
                state: ButtonState::Pressed,
                text: Some("5".into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        };
        type_digit(&mut app);
        app.insert_resource(Section::Lineup);
        type_digit(&mut app);

        assert_eq!(app.world().resource::<Draft>().seconds, "5");
        app.insert_resource(Section::Rules);
        type_digit(&mut app);
        assert_eq!(app.world().resource::<Draft>().seconds, "55");
    }
}

#[cfg(test)]
mod lineup_tests;
