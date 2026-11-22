use super::*;

fn replay_app() -> App {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec");
    let mut app = artwork::tests::headless();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(40),
    ))
    .init_resource::<locale::Preferences>()
    .insert_resource(ReplaySession::new(Playback::open(&source).unwrap()))
    .add_systems(Startup, controls)
    .add_systems(Update, transport_face)
    .add_systems(PostUpdate, icons::sync);
    app
}

#[test]
fn one_transport_button_keeps_its_target_and_announces_the_next_action() {
    let mut app = replay_app();
    app.update();
    let w = app.world_mut();
    let button = w
        .query_filtered::<Entity, With<PlayButton>>()
        .single(w)
        .unwrap();
    let face = |app: &App, icon| icons::assert_face(app.world(), button, icon);
    assert_eq!(
        app.world().get::<Node>(button).unwrap().width,
        px(DOCK_HEIGHT)
    );
    face(&app, Icon::Pause);
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    face(&app, Icon::Play);
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    face(&app, Icon::Pause);
    {
        let mut g = app.world_mut().resource_mut::<ReplaySession>();
        let p = &mut g.player;
        p.seek(p.length()).unwrap();
        g.tail = end_tail(p);
        g.playing = false;
        g.carry = 0.5;
    }
    app.update();
    face(&app, Icon::Restart);
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    face(&app, Icon::Pause);
    let g = app.world().resource::<ReplaySession>();
    assert_eq!(g.player.world().tick(), 0);
    assert_eq!((g.tail, g.carry), (0., 0.));
}

#[test]
fn transport_buttons_share_one_translucent_backplate() {
    use bevy::ui_widgets::{MenuButton, MenuPopup};

    let mut app = replay_app();
    app.update();
    let w = app.world_mut();
    let play = w
        .query_filtered::<Entity, With<PlayButton>>()
        .single(w)
        .unwrap();
    let dock = w.get::<ChildOf>(play).unwrap().parent();
    let node = w.get::<Node>(dock).unwrap();
    assert_eq!(node.border_radius, BorderRadius::MAX);
    assert_eq!(node.padding.left, px(0));
    assert_eq!(node.height, w.get::<Node>(play).unwrap().height);
    assert_eq!(node.overflow, Overflow::visible());
    let tint = w.get::<BackgroundColor>(dock).unwrap().0;
    assert!(tint.alpha() > 0. && tint.alpha() < 1.);
    for bg in w
        .query_filtered::<&BackgroundColor, Or<(With<Icon>, With<MenuButton>)>>()
        .iter(w)
    {
        assert_eq!(bg.0.alpha(), 0.);
    }

    assert_eq!(
        w.query_filtered::<&BackgroundColor, With<MenuPopup>>()
            .single(w)
            .unwrap()
            .0,
        tint
    );
}

#[test]
fn choosing_speed_keeps_time_and_single_step_independent_of_the_picker() {
    use bevy::{
        input_focus::{InputFocus, InputFocusVisible},
        ui::Checked,
        ui_widgets::{MenuAction, MenuButton, MenuEvent, MenuPopup},
    };

    let mut app = replay_app();
    app.init_resource::<InputFocus>()
        .init_resource::<InputFocusVisible>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<NextState<Page>>()
        .insert_resource(State::new(Page::Replay))
        .add_systems(Update, play.before(transport_face));
    speed::install(&mut app);
    app.world_mut().resource_mut::<ReplaySession>().playing = false;
    app.update();
    let w = app.world_mut();
    let button = w
        .query_filtered::<Entity, With<MenuButton>>()
        .single(w)
        .unwrap();
    let popup = w
        .query_filtered::<Entity, With<MenuPopup>>()
        .single(w)
        .unwrap();
    let readout = w
        .query_filtered::<Entity, With<Readout>>()
        .single(w)
        .unwrap();
    let step = w
        .query::<(Entity, &Icon)>()
        .iter(w)
        .find(|(_, icon)| **icon == Icon::Step)
        .unwrap()
        .0;
    icons::assert_face(w, step, Icon::Step);
    let caption = w.get::<Children>(button).unwrap()[0];
    assert_eq!(w.get::<Text>(caption).unwrap().0, "1×");
    assert!(!w.get::<Text>(readout).unwrap().0.contains('×'));
    assert_eq!(w.get::<Visibility>(popup), Some(&Visibility::Hidden));
    let options = w.get::<Children>(popup).unwrap().to_vec();
    assert_eq!(
        options
            .iter()
            .map(|id| w.get::<Rate>(*id).unwrap().multiplier())
            .collect::<Vec<_>>(),
        [0.25, 0.5, 1., 2., 4.]
    );

    // 在正在退出的选项上松开指针，不应改变播放速度。
    w.trigger(Activate { entity: options[4] });

    assert_eq!(w.resource::<ReplaySession>().speed, Rate::Normal);
    w.trigger(MenuEvent {
        source: button,
        action: MenuAction::Toggle,
    });
    app.update();
    let w = app.world_mut();
    let Val::Px(height) = w.get::<Node>(popup).unwrap().height else {
        panic!()
    };
    assert!(height > 0. && height < 180.);
    w.trigger(Activate { entity: options[4] });
    w.trigger(MenuEvent {
        source: options[4],
        action: MenuAction::CloseAll,
    });
    app.update();
    let w = app.world_mut();
    assert_eq!(w.get::<Text>(caption).unwrap().0, "4×");
    assert!(w.get::<Checked>(options[4]).is_some() && w.get::<Checked>(options[2]).is_none());
    assert_eq!(w.resource::<ReplaySession>().player.world().tick(), 0);
    w.trigger(Activate { entity: step });
    app.update();
    let g = app.world().resource::<ReplaySession>();
    assert_eq!(g.player.world().tick(), 1);
    assert!(!g.playing);
    let hz = g.player.header.tick_hz;
    app.world_mut().resource_mut::<ReplaySession>().playing = true;
    app.update();

    assert_eq!(
        app.world()
            .resource::<ReplaySession>()
            .player
            .world()
            .tick(),
        1 + (0.04 * f64::from(hz) * 4.) as u64
    );
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Hidden)
    );
}

#[test]
fn paused_arena_leaves_actor_poses_untouched_and_releases_match_assets_on_exit() {
    use bevy::{
        input_focus::{InputFocus, InputFocusVisible},
        state::app::StatesPlugin,
    };

    let mut app = artwork::tests::headless();
    app.add_plugins(StatesPlugin)
        .init_state::<Page>()
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .init_resource::<locale::Preferences>()
        .init_resource::<Field>()
        .init_resource::<battle::LiveSession>()
        .init_resource::<battle::LiveFrame>()
        .init_resource::<InputFocus>()
        .init_resource::<InputFocusVisible>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_plugins((arena_view::install, install));
    app.update(); // 先从主菜单启动，照常准备渲染资源。
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec");
    let mut session = ReplaySession::new(Playback::open(&path).unwrap());
    session.playing = false;
    app.insert_resource(session);
    app.world_mut()
        .resource_mut::<NextState<Page>>()
        .set(Page::Replay);
    app.update();
    let w = app.world_mut();
    let actor = w
        .query::<(Entity, &arena_view::Actor, &Visibility)>()
        .iter(w)
        .find(|(_, actor, vis)| {
            matches!(actor, arena_view::Actor::Tank(0)) && **vis != Visibility::Hidden
        })
        .unwrap()
        .0;
    let fill = w.query_filtered::<Entity, With<Fill>>().single(w).unwrap();
    let changed = |world: &World| {
        (
            world
                .entity(actor)
                .get_ref::<Transform>()
                .unwrap()
                .last_changed(),
            world.entity(fill).get_ref::<Node>().unwrap().last_changed(),
        )
    };
    let before = changed(w);
    for _ in 0..3 {
        app.update();
    }

    assert_eq!(before, changed(app.world()));
    app.world_mut()
        .resource_mut::<ReplaySession>()
        .player
        .advance()
        .unwrap();
    app.update();
    let after = changed(app.world());
    assert_ne!(before.0, after.0);
    assert_ne!(before.1, after.1);
    assert!(!app.world().resource::<arena_view::MatchArt>().0.is_empty());
    app.world_mut()
        .resource_mut::<NextState<Page>>()
        .set(Page::Menu);
    app.update();

    assert!(app.world().get_entity(actor).is_err());
    assert!(!app.world().contains_resource::<ReplaySession>());
    assert!(app.world().resource::<arena_view::MatchArt>().0.is_empty());
}
