use super::*;
use bevy::{
    input_focus::{
        FocusCause, InputFocus, InputFocusVisible,
        tab_navigation::{NavAction, TabIndex},
    },
    ui::{Checkable, Checked},
    ui_widgets::{MenuAction, MenuButton, MenuEvent, MenuFocusState, MenuItem, MenuPopup},
};

#[derive(Component, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(in crate::game_ui) enum Rate {
    Quarter,
    Half,
    #[default]
    Normal,
    Double,
    Quad,
}

impl Rate {
    const ALL: [Self; 5] = [
        Self::Quarter,
        Self::Half,
        Self::Normal,
        Self::Double,
        Self::Quad,
    ];

    pub(super) fn multiplier(self) -> f64 {
        match self {
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::Normal => 1.,
            Self::Double => 2.,
            Self::Quad => 4.,
        }
    }

    fn caption(self) -> String {
        format!("{}×", self.multiplier())
    }

    fn name(self) -> AccessibleLabel {
        AccessibleLabel::new(format!("{}: {}", tr("speed"), self.caption()))
    }
}

const WIDTH: f32 = 82.;
const ROW: f32 = 36.;

#[derive(Component)]
struct Reveal(f32);

#[derive(Component)]
struct Caption;

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, (update, style).run_if(in_state(Page::Replay)));
}

pub(super) fn picker(p: &mut ChildSpawnerCommands, current: Rate) {
    p.spawn(Node {
        width: px(WIDTH),
        height: px(DOCK_HEIGHT),
        ..default()
    })
    .observe(menu_event)
    .with_children(|p| {
        p.spawn((
            MenuButton,
            TabIndex(0),
            current.name(),
            surface(Color::NONE, dock_highlight()),
            Node {
                width: percent(100),
                height: percent(100),
                column_gap: px(8),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![
                (
                    Caption,
                    label(current.caption(), 15., Color::WHITE),
                    Pickable::IGNORE
                ),
                (
                    Node {
                        width: px(7),
                        height: px(7),
                        border: UiRect {
                            top: px(1.5),
                            left: px(1.5),
                            ..default()
                        },
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    UiTransform::from_rotation(Rot2::degrees(45.)),
                    Pickable::IGNORE
                )
            ],
        ));
        p.spawn((
            MenuPopup::default(),
            Reveal(0.),
            Visibility::Hidden,
            GlobalZIndex(2),
            BackgroundColor(dock_tint()),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(DOCK_HEIGHT + 6.),
                left: px(0),
                width: percent(100),
                height: px(0),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(px(10)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
        ))
        .with_children(|p| {
            for rate in Rate::ALL {
                let selected = rate == current;
                let fg = if selected { lime() } else { Color::WHITE };
                let mut item = p.spawn((
                    rate,
                    MenuItem,
                    TabIndex(0),
                    Checkable,
                    AccessibleLabel::new(rate.caption()),
                    surface(Color::NONE, dock_highlight()),
                    Node {
                        height: px(ROW),
                        flex_shrink: 0.,
                        border: UiRect::all(px(1)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    children![(label(rate.caption(), 15., fg), Pickable::IGNORE)],
                ));
                if selected {
                    item.insert(Checked);
                }
                item.observe(select);
            }
        });
    });
}

fn menu_event(
    ev: On<MenuEvent>,
    mut state: Single<&mut MenuFocusState, With<Reveal>>,
    button: Single<Entity, With<MenuButton>>,
    mut focus: ResMut<InputFocus>,
) {
    match ev.action {
        MenuAction::Open(direction) => **state = MenuFocusState::Opening(direction),
        MenuAction::Toggle if **state == MenuFocusState::Closed => {
            **state = MenuFocusState::Opening(NavAction::First);
        }
        MenuAction::Toggle | MenuAction::CloseAll => **state = MenuFocusState::Closed,
        MenuAction::FocusRoot => focus.set(*button, FocusCause::Navigated),
    }
}

fn select(
    ev: On<Activate>,
    state: Single<&MenuFocusState, With<Reveal>>,
    rates: Query<&Rate>,
    mut g: ResMut<ReplaySession>,
    mut caption: Single<&mut Text, With<Caption>>,
    button: Single<Entity, With<MenuButton>>,
    mut c: Commands,
) {
    if **state != MenuFocusState::Closed
        && let Ok(&rate) = rates.get(ev.entity)
    {
        g.speed = rate;
        caption.0 = rate.caption();
        c.entity(*button).insert(rate.name());
    }
}

fn update(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Page>>,
    popup: Single<(&mut Reveal, &mut Visibility, &mut Node, &mut MenuFocusState)>,
) {
    let (mut reveal, mut visibility, mut node, mut state) = popup.into_inner();
    if keys.just_pressed(KeyCode::Escape) {
        // 本帧内原生菜单可能已先处理 Esc 并关闭焦点。
        if *state == MenuFocusState::Closed && reveal.0 == 0. {
            next.set(Page::Menu);
        } else {
            *state = MenuFocusState::Closed;
        }
    }
    let open = *state != MenuFocusState::Closed;
    let target = if open { 1. } else { 0. };
    if reveal.0 == target {
        return;
    }

    reveal.0 = reveal
        .0
        .move_towards(target, time.delta_secs() / if open { 0.16 } else { 0.12 });
    node.height = px(ROW * Rate::ALL.len() as f32 * ease(reveal.0));
    visibility.set_if_neq(if reveal.0 == 0. {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
}

fn style(
    g: Res<ReplaySession>,
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    mut items: Query<(Entity, &Rate, &Children, Has<Checked>, &mut BorderColor)>,
    mut text: Query<&mut TextColor>,
    mut c: Commands,
) {
    for (id, &rate, children, checked, mut border) in &mut items {
        let selected = rate == g.speed;
        if selected != checked {
            if selected {
                c.entity(id).insert(Checked);
            } else {
                c.entity(id).remove::<Checked>();
            }
            if let Ok(mut fg) = text.get_mut(children[0]) {
                fg.0 = if selected { lime() } else { Color::WHITE };
            }
        }
        border.set_if_neq(BorderColor::all(if visible.0 && focus.get() == Some(id) {
            Color::WHITE
        } else {
            Color::NONE
        }));
    }
}
