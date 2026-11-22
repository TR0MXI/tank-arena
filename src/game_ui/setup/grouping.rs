use super::*;

pub(super) fn teams(p: &mut ChildSpawnerCommands, d: &Draft, profiles: &Profiles) {
    p.spawn(spread()).with_children(|p| {
        p.spawn(headline(&tr("groups"), 20.));
        p.spawn(label(
            rust_i18n::t!("deployedCount", n = d.seats().count(), max = MAX_SLOTS),
            14.,
            quiet(),
        ));
    });
    team_scroll::grid(p, |p| {
        for &team in &d.teams {
            team_card(p, d, team, profiles);
        }

        if d.teams.len() < MAX_SLOTS {
            p.spawn((
                Button,
                AccessibleLabel::new(tr("addTeam")),
                Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(207, 217, 202)),
                tile_surface(card_bg()),
                observe(
                    |_: On<Activate>,
                     mut d: ResMut<Draft>,
                     mut scroll: ResMut<team_scroll::TeamScroll>| {
                        d.add_team();
                        scroll.reveal(d.teams.len() - 1);
                    },
                ),
                children![stroke(24., 3.), stroke(3., 24.)],
            ));
        }
    });
}

fn stroke(width: f32, height: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: px(width),
            height: px(height),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(quiet()),
        Pickable::IGNORE,
    )
}

fn team_card(p: &mut ChildSpawnerCommands, d: &Draft, team: TeamId, profiles: &Profiles) {
    let [r, g, b] = arena_view::team_rgb(team);
    let color = Color::srgb_u8(r, g, b);
    let selected = team == d.selected_team;
    let title = rust_i18n::t!("teamLabel", n = team.0);
    let base = if selected {
        Color::srgb_u8(240, 246, 228)
    } else {
        card_bg()
    };
    p.spawn((
        Button,
        AccessibleLabel::new(title.to_string()),
        Node {
            min_width: px(0),
            padding: UiRect::axes(px(10), px(8)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(10)),
            ..column(5.)
        },
        BorderColor::all(if selected {
            ink()
        } else {
            Color::srgb_u8(213, 221, 209)
        }),
        tile_surface(base),
        observe(move |_: On<Activate>, mut d: ResMut<Draft>| d.select_team(team)),
        hangar::drop_into(team),
    ))
    .with_children(|p| {
        p.spawn((
            Node {
                height: px(32),
                ..spread()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((row(), Pickable::IGNORE)).with_children(|p| {
                p.spawn((
                    Node {
                        width: px(10),
                        height: px(10),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(color),
                    Pickable::IGNORE,
                ));
                p.spawn((headline(&title, 18.), Pickable::IGNORE));
            });
            let accessible =
                || AccessibleLabel::new(rust_i18n::t!("removeTeam", n = team.0).into_owned());
            if d.confirm_team == Some(team) {
                p.spawn(row()).with_children(|p| {
                    p.spawn(edit(&tr("cancelDelete"), |d| d.confirm_team = None));
                    p.spawn((
                        edit(&tr("confirmDelete"), move |d| d.confirm_remove_team(team)),
                        accessible(),
                    ));
                });
            } else {
                let enabled = d.teams.len() > 1;
                let mut remove = p.spawn((
                    Button,
                    accessible(),
                    Node {
                        width: px(32),
                        height: px(32),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    surface(Color::NONE, Color::srgb_u8(249, 221, 213)),
                    children![(
                        label(
                            "×",
                            21.,
                            if enabled {
                                quiet()
                            } else {
                                Color::srgb_u8(170, 182, 166)
                            }
                        ),
                        Pickable::IGNORE
                    )],
                ));
                if enabled {
                    remove.observe(move |_: On<Activate>, mut d: ResMut<Draft>| {
                        d.request_remove_team(team);
                    });
                } else {
                    remove.insert(bevy::ui::InteractionDisabled);
                }
            }
        });
        let members: Vec<_> = d.slots().filter(|(_, t)| t.team == team).collect();
        if members.is_empty() {
            p.spawn((
                Node {
                    width: percent(100),
                    flex_grow: 1.,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                children![(label(tr("emptyTeam"), 15., quiet()), Pickable::IGNORE)],
            ));
        } else {
            p.spawn((
                Node {
                    width: percent(100),
                    ..column(2.)
                },
                Pickable::IGNORE,
            ))
            .with_children(|p| {
                // 默认给按钮较大的点击区，仅在同队达到七八辆时为布局让出空间。
                let height = match members.len() {
                    8 => 24.,
                    7 => 27.,
                    _ => 32.,
                };
                for (slot, t) in members {
                    member(p, slot, &profiles.name(&t.model), height);
                }
            });
        }
    });
}

fn member(p: &mut ChildSpawnerCommands, slot: SlotId, name: &str, height: f32) {
    p.spawn((
        Node {
            width: percent(100),
            height: px(height),
            padding: UiRect::horizontal(px(5)),
            column_gap: px(7),
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(Color::WHITE),
    ))
    .with_children(|p| {
        p.spawn((
            label(format!("#{}", slot.0), 13., quiet()),
            Pickable::IGNORE,
        ));
        p.spawn(clipped_name(name));
        p.spawn((
            Button,
            AccessibleLabel::new(format!("{} #{} {name}", tr("remove"), slot.0)),
            Node {
                width: px(32),
                height: px(height),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            surface(paper(), Color::srgb_u8(249, 221, 213)),
            observe(move |_: On<Activate>, mut d: ResMut<Draft>| d.remove(slot)),
            children![(
                label("×", 18., Color::srgb_u8(130, 48, 40)),
                Pickable::IGNORE
            )],
        ));
    });
}

pub(super) fn clipped_name(name: &str) -> impl Bundle {
    (
        Node {
            min_width: px(0),
            flex_grow: 1.,
            flex_basis: px(0),
            overflow: Overflow::clip(),
            ..default()
        },
        Pickable::IGNORE,
        children![(
            label(name, 15., ink()),
            TextLayout::no_wrap(),
            Pickable::IGNORE
        )],
    )
}

pub(super) fn tile_surface(base: Color) -> impl Bundle {
    surface(base, base.mix(&lime(), 0.38))
}
