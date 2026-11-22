use super::*;
use bevy::ui::InteractionDisabled;

const DESTINATIONS: [Icon; 4] = [Icon::Replays, Icon::Maps, Icon::Tanks, Icon::Settings];
const TRAVEL: f32 = 0.22;

#[derive(Component)]
pub(super) struct Toolbar {
    phase: f32,
}

#[derive(Component)]
struct Slot(usize);

pub(super) fn install(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, sync);
}

fn spawn(mut c: Commands, art: Res<Art>) {
    c.spawn((
        Toolbar { phase: 1. },
        GlobalZIndex(5),
        Node {
            position_type: PositionType::Absolute,
            left: px(24),
            top: px(22),
            width: px(ICON_SIZE * DESTINATIONS.len() as f32),
            height: px(ICON_SIZE),
            overflow: Overflow::clip(),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(ink()),
        Pickable::IGNORE,
    ))
    .with_children(|p| {
        for (index, icon) in DESTINATIONS.into_iter().enumerate() {
            p.spawn((
                Slot(index),
                art.icon_button(icon, ICON_SIZE, BorderRadius::MAX, quiet()),
                observe(activate),
            ));
        }
    });
}

type Slots<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Slot,
        &'static mut Icon,
        &'static mut Visibility,
        Has<InteractionDisabled>,
    ),
>;

fn sync(
    mut c: Commands,
    page: Res<State<Page>>,
    panel: Res<Panel>,
    time: Res<Time<Real>>,
    bar: Single<(&mut Toolbar, &mut Node)>,
    mut slots: Slots,
) {
    let home = *page.get() == Page::Menu;
    let target = if home { 1. } else { 0. };
    let (mut bar, mut node) = bar.into_inner();
    if bar.phase == target && !page.is_changed() && !panel.is_changed() {
        return;
    }

    // 锚点、按钮实体和点击区保持不动，仅由胶囊外框裁切或显露按钮。
    bar.phase = bar.phase.move_towards(target, time.delta_secs() / TRAVEL);
    let width = ICON_SIZE * (1. + (DESTINATIONS.len() - 1) as f32 * ease(bar.phase));
    node.width = px(width);
    for (id, slot, mut shown, mut visibility, disabled) in &mut slots {
        let lead = slot.0 == 0;
        let icon = if lead && !home {
            Icon::Home
        } else {
            DESTINATIONS[slot.0]
        };
        shown.set_if_neq(icon);
        visibility.set_if_neq(if !lead && bar.phase == 0. {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });

        // 被裁切或正在退出的入口不能响应点击。
        let enabled =
            *panel == Panel::Closed && (lead || (home && width >= (slot.0 + 1) as f32 * ICON_SIZE));
        if enabled && disabled {
            c.entity(id).remove::<InteractionDisabled>();
        }
        if !enabled && !disabled {
            c.entity(id).insert(InteractionDisabled);
        }
    }
}

fn activate(
    ev: On<Activate>,
    slots: Query<&Slot, Without<InteractionDisabled>>,
    page: Res<State<Page>>,
    mut panel: ResMut<Panel>,
    mut c: Commands,
    mut next: ResMut<NextState<Page>>,
) {
    if *panel != Panel::Closed {
        return;
    }
    let Ok(slot) = slots.get(ev.entity) else {
        return;
    };

    if *page.get() == Page::Menu {
        match DESTINATIONS[slot.0] {
            Icon::Replays => next.set(Page::Library),
            Icon::Maps => next.set(Page::Maps),
            Icon::Tanks => next.set(Page::Tanks),
            Icon::Settings => *panel = Panel::Settings,
            _ => unreachable!("not a toolbar destination"),
        }
    } else if slot.0 == 0 {
        match page.get() {
            Page::Live => c.trigger(battle::RequestHome),
            _ => next.set(Page::Menu),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_reverses_without_replacing_targets_or_activating_clipped_slots() {
        let mut app = artwork::tests::headless();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(40),
        ))
        .insert_resource(State::new(Page::Menu))
        .init_resource::<NextState<Page>>()
        .init_resource::<Panel>()
        .init_resource::<locale::Preferences>();
        install(&mut app);
        app.add_systems(PostUpdate, icons::sync);
        app.update();
        let w = app.world_mut();
        let root = w
            .query_filtered::<Entity, With<Toolbar>>()
            .single(w)
            .unwrap();
        let [lead, maps, _, gear] = **w.get::<Children>(root).unwrap() else {
            panic!("expected four slots")
        };
        assert_eq!(w.query::<&Text>().iter(w).count(), 0);
        let width = |a: &App| {
            let Val::Px(w) = a.world().get::<Node>(root).unwrap().width else {
                panic!()
            };
            w
        };
        assert_eq!(width(&app), 4. * ICON_SIZE);
        icons::assert_face(app.world(), lead, Icon::Replays);
        app.world_mut().trigger(Activate { entity: maps });

        assert!(matches!(
            app.world().resource::<NextState<Page>>(),
            NextState::Pending(Page::Maps)
        ));
        app.insert_resource(State::new(Page::Maps));
        app.update();
        app.update();
        icons::assert_face(app.world(), lead, Icon::Home);
        let mid = width(&app);
        assert!((ICON_SIZE..4. * ICON_SIZE).contains(&mid));
        assert!(app.world().get::<InteractionDisabled>(maps).is_some());
        app.world_mut().trigger(Activate { entity: gear });

        assert!(matches!(app.world().resource::<Panel>(), Panel::Closed));
        app.insert_resource(State::new(Page::Menu));
        app.update();
        icons::assert_face(app.world(), lead, Icon::Replays);

        assert!(width(&app) > mid && width(&app) < 4. * ICON_SIZE);
        assert!(app.world().get::<InteractionDisabled>(gear).is_some());
        for _ in 0..6 {
            app.update();
        }

        app.world_mut().trigger(Activate { entity: gear });
        app.update();

        assert!(app.world().get::<InteractionDisabled>(lead).is_some());
        app.insert_resource(Panel::Closed)
            .insert_resource(State::new(Page::Maps));
        for _ in 0..7 {
            app.update();
        }

        assert_eq!(width(&app), ICON_SIZE);
        assert_eq!(
            app.world().get::<Visibility>(maps),
            Some(&Visibility::Hidden)
        );
        icons::assert_face(app.world(), lead, Icon::Home);
        app.world_mut().trigger(Activate { entity: lead });

        assert!(matches!(
            app.world().resource::<NextState<Page>>(),
            NextState::Pending(Page::Menu)
        ));
    }
}
