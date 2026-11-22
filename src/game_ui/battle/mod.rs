mod frame;
mod hud;
mod result;

use super::*;
pub(super) use frame::LiveFrame;
use result::Summary;
use tank_arena::sim::ClockRules;
use tank_arena::{replay::Recording, runner::LiveMatch};

#[derive(Resource, Default)]
pub(super) struct LiveSession {
    live: Option<LiveMatch>,
    record: Option<Recording>,
    stage: Option<std::sync::Arc<tempfile::TempDir>>,
    finished: Option<Summary>,
    names: Vec<String>,
    clock: ClockRules,
    pub(super) show_roster: bool,
    confirm_stop: Option<StopPrompt>,
    pub(super) tail: f32,
}

impl LiveSession {
    pub(super) fn running(&self) -> bool {
        self.live.is_some()
    }

    pub(super) fn start(
        &mut self,
        live: LiveMatch,
        names: Vec<String>,
        record: Recording,
        stage: tempfile::TempDir,
        clock: ClockRules,
    ) {
        *self = Self {
            live: Some(live),
            names,
            record: Some(record),
            stage: Some(std::sync::Arc::new(stage)),
            clock,
            ..default()
        };
    }
}

#[derive(bevy::prelude::Event)]
pub(super) struct RequestHome;

fn request_home(_: On<RequestHome>, mut g: ResMut<LiveSession>, mut next: ResMut<NextState<Page>>) {
    if g.running() {
        g.confirm_stop.get_or_insert(StopPrompt::Home);
    } else {
        next.set(Page::Menu);
    }
}

#[derive(Component)]
struct Confirm;

#[derive(Clone, Copy)]
pub(super) enum StopPrompt {
    Result,
    Home,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<LiveSession>()
        .init_resource::<LiveFrame>()
        .add_observer(request_home)
        .add_systems(OnEnter(Page::Live), (arena_view::draw, hud::spawn))
        .add_systems(
            Update,
            (
                frame::capture,
                hud::update,
                cancel_prompt,
                stop_prompt,
                result::overlay,
                result::actions,
            )
                .chain()
                .run_if(in_state(Page::Live)),
        )
        .add_systems(OnExit(Page::Live), frame::leave);
}

fn cancel_prompt(keys: Res<ButtonInput<KeyCode>>, mut g: ResMut<LiveSession>) {
    if g.confirm_stop.is_some() && keys.just_pressed(KeyCode::Escape) {
        g.confirm_stop = None;
    }
}

fn stop_prompt(mut c: Commands, g: Res<LiveSession>, open: Query<Entity, With<Confirm>>) {
    let Some(prompt) = g.confirm_stop else {
        for e in &open {
            c.entity(e).despawn();
        }
        return;
    };
    if !open.is_empty() {
        return;
    }

    let (title, hint, accept) = match prompt {
        StopPrompt::Result => ("endMatch", "stopHint", "confirmEnd"),
        StopPrompt::Home => ("returnHome", "leaveHint", "returnHome"),
    };
    c.spawn((
        DespawnOnExit(Page::Live),
        Confirm,
        menu::modal_layer(),
        BackgroundColor(Color::srgba_u8(14, 26, 26, 162)),
    ))
    .with_children(|p| {
        p.spawn(menu::modal_card(464.)).with_children(|p| {
            p.spawn(headline(&tr(title), 26.));
            p.spawn(label(tr(hint), 16., quiet()));
            p.spawn(row()).with_children(|p| {
                p.spawn((
                    primary(&tr("cancel")),
                    observe(|_: On<Activate>, mut g: ResMut<LiveSession>| g.confirm_stop = None),
                ));
                p.spawn((
                    compact(&tr(accept)),
                    observe(
                        move |_: On<Activate>,
                              g: Res<LiveSession>,
                              mut next: ResMut<NextState<Page>>| {
                            if let Some(l) = &g.live {
                                match prompt {
                                    StopPrompt::Result => l.stop(),
                                    // 退出实况时先停止比赛并等待线程结束，再释放录制器。
                                    StopPrompt::Home => next.set(Page::Menu),
                                }
                            }
                        },
                    ),
                ));
            });
        });
    });
}
