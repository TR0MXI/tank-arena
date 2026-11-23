use super::*;

#[derive(Resource, Default)]
pub(super) struct Preview {
    image: Handle<Image>,
    map: Option<Arc<ArenaMap>>,
    teams: Vec<TeamId>,
}

pub(super) fn preview(
    section: Res<Section>,
    d: Res<Draft>,
    field: Res<Field>,
    mut images: ResMut<Assets<Image>>,
    mut preview: ResMut<Preview>,
) {
    if *section != Section::Rules
        || (!section.is_changed() && !d.is_changed() && !field.is_changed())
    {
        return;
    }

    let teams: Vec<_> = d.seats().map(|s| s.team).collect();
    if preview
        .map
        .as_ref()
        .is_none_or(|m| !Arc::ptr_eq(m, &field.map))
        || preview.teams != teams
    {
        let markers = field.map.spawns().iter().enumerate().map(|(i, s)| {
            (
                s.center,
                s.heading,
                teams
                    .get(i)
                    .copied()
                    .map_or([159, 172, 155], arena_view::team_rgb),
            )
        });
        preview.image = images.add(tactical_map::deployment(&field.map, markers));
        preview.teams = teams;
        preview.map = Some(field.map.clone());
    }
}

/// 阵容页的分区实体贯穿该页面；修改队伍不销毁车型卡片或指针目标。
#[derive(Component)]
pub(super) struct LineupAreas {
    header: Entity,
    launchable: bool,
    teams: Entity,
    notes: Entity,
}

pub(super) fn render(
    mut c: Commands,
    section: Res<Section>,
    d: Res<Draft>,
    (field, preview): (Res<Field>, Res<Preview>),
    prefs: Res<locale::Preferences>,
    profiles: Res<Profiles>,
    mut old: Query<(Entity, Option<&mut LineupAreas>), With<Sheet>>,
) {
    if !section.is_changed()
        && !d.is_changed()
        && !field.is_changed()
        && !profiles.is_changed()
        && !prefs.is_changed()
        && !old.is_empty()
    {
        return;
    }

    let lineup = *section == Section::Lineup;
    let valid = if lineup {
        d.config(&field.map).map(|_| ())
    } else {
        d.clock().map(|_| ())
    };
    let notice_box = || Node {
        display: if valid.is_err() || field.scan_error.is_some() || !d.note.is_empty() {
            Display::Flex
        } else {
            Display::None
        },
        ..column(4.)
    };
    if lineup && let Ok((_, Some(mut areas))) = old.single_mut() {
        // 时间和地图只在规则页改；阵容编辑通常只影响是否允许开局。
        if areas.launchable != valid.is_ok() || field.is_changed() || prefs.is_changed() {
            areas.launchable = valid.is_ok();
            c.entity(areas.header)
                .despawn_children()
                .with_children(|p| lineup_header(p, &d, &field, valid.is_ok()));
        }
        if d.is_changed() || profiles.is_changed() || prefs.is_changed() {
            c.entity(areas.teams)
                .despawn_children()
                .with_children(|p| grouping::teams(p, &d, &profiles));
        }
        c.entity(areas.notes)
            .insert(notice_box())
            .despawn_children()
            .with_children(|p| notes(p, &valid, &field, &d));
        return;
    }

    for (e, _) in &old {
        c.entity(e).despawn();
    }

    let mut root = c.spawn((
        Sheet,
        DespawnOnExit(Page::Setup),
        root(),
        BackgroundColor(paper()),
    ));
    let mut areas = None;
    let mut body = page_body();
    if lineup {
        body.height = percent(100);
        body.padding.bottom = px(10);
    }
    root.with_children(|p| {
        p.spawn(body).with_children(|p| {
            let header = p
                .spawn(page_header())
                .with_children(|p| {
                    if lineup {
                        lineup_header(p, &d, &field, valid.is_ok());
                    } else {
                        p.spawn((PageIntro, headline(&tr("mapAndClock"), 30.)));
                        if valid.is_ok() {
                            p.spawn((primary(&tr("nextStep")), observe(next_step)));
                        } else {
                            p.spawn(disabled_primary(&tr("nextStep")));
                        }
                    }
                })
                .id();
            let mut teams = None;
            p.spawn(Node {
                width: percent(100),
                column_gap: px(16),
                flex_grow: if lineup { 1. } else { 0. },
                min_height: px(0),
                align_items: AlignItems::FlexStart,
                ..default()
            })
            .with_children(|p| {
                if lineup {
                    teams = Some(
                        p.spawn(Node {
                            width: percent(73),
                            min_height: px(0),
                            align_self: AlignSelf::Stretch,
                            ..column(6.)
                        })
                        .with_children(|p| grouping::teams(p, &d, &profiles))
                        .id(),
                    );
                    p.spawn(side_column())
                        .with_child((card(100.), hangar::Rack));
                } else {
                    map_card(p, &field, &preview.image);
                    p.spawn(side_column()).with_children(|p| {
                        p.spawn(card(100.)).with_children(|p| clock_controls(p, &d));
                        p.spawn(card(100.))
                            .with_children(|p| generator(p, &field, &d));
                    });
                }
            });
            if !lineup {
                items::manager(p, &d, prefs.items);
            }
            let notices = p
                .spawn(notice_box())
                .with_children(|p| notes(p, &valid, &field, &d))
                .id();
            areas = teams.map(|teams| LineupAreas {
                header,
                launchable: valid.is_ok(),
                teams,
                notes: notices,
            });
        });
    });
    if let Some(areas) = areas {
        root.insert(areas);
    }
}

fn notes(p: &mut ChildSpawnerCommands, valid: &Result<(), String>, field: &Field, d: &Draft) {
    if let Err(note) = valid {
        p.spawn(label(note, 14., quiet()));
    }
    if let Some(note) = &field.scan_error {
        p.spawn(label(note, 14., quiet()));
    }
    if !d.note.is_empty() {
        p.spawn(label(&d.note, 14., quiet()));
    }
}

fn lineup_header(p: &mut ChildSpawnerCommands, d: &Draft, field: &Field, valid: bool) {
    p.spawn((PageIntro, headline(&tr("lineup"), 30.)));
    p.spawn(row()).with_children(|p| {
        let time = match d.limit {
            LimitMode::Default => tr("minute"),
            LimitMode::Endless => tr("unlimited"),
            LimitMode::Custom => format!("{} s", d.seconds),
        };
        p.spawn((
            Node {
                max_width: px(275),
                overflow: Overflow::clip(),
                ..default()
            },
            children![(
                label(format!("{}  ·  {time}", field.name()), 15., quiet()),
                TextLayout::no_wrap()
            )],
        ));
        p.spawn((
            compact(&tr("previousStep")),
            observe(
                |_: On<Activate>, mut section: ResMut<Section>, mut d: ResMut<Draft>| {
                    *section = Section::Rules;
                    d.confirm_team = None;
                },
            ),
        ));
        if valid {
            p.spawn((primary(&tr("launch")), launch()));
        } else {
            p.spawn(disabled_primary(&tr("launch")));
        }
    });
}

fn side_column() -> Node {
    Node {
        flex_grow: 1.,
        flex_basis: px(0),
        min_width: px(0),
        ..column(16.)
    }
}

fn map_card(p: &mut ChildSpawnerCommands, field: &Field, image: &Handle<Image>) {
    p.spawn(card(59.)).with_children(|p| {
        p.spawn(spread()).with_children(|p| {
            p.spawn((
                Node {
                    flex_grow: 1.,
                    flex_basis: px(0),
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                children![(headline(&field.name(), 20.), TextLayout::no_wrap())],
            ));
            if field.multiple() {
                p.spawn(row()).with_children(|p| {
                    for (delta, glyph, key) in [(-1, "‹", "prevMap"), (1, "›", "nextMap")] {
                        p.spawn((
                            compact(glyph),
                            AccessibleLabel::new(tr(key)),
                            observe(move |_: On<Activate>, mut f: ResMut<Field>| f.cycle(delta)),
                        ));
                    }
                });
            }
        });
        p.spawn((
            Node {
                width: percent(100),
                height: px(320),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ink()),
        ))
        .with_children(|p| {
            let (w, h) = field.map.size();
            let scale = (540. / w as f32).min(320. / h as f32);
            p.spawn((
                ImageNode::new(image.clone()),
                Node {
                    width: px(w as f32 * scale),
                    height: px(h as f32 * scale),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
    });
}

fn generator(p: &mut ChildSpawnerCommands, field: &Field, d: &Draft) {
    p.spawn(headline(&tr("mapGenerator"), 20.));
    p.spawn(row()).with_children(|p| {
        for axis in 0..2 {
            if axis == 1 {
                p.spawn(label("×", 15., quiet()));
            }
            for delta in [-1isize, 1] {
                if delta == 1 {
                    p.spawn(label(d.dimensions[axis].to_string(), 15., ink()));
                }
                let target = d.dimensions[axis].saturating_add_signed(delta);
                let glyph = if delta < 0 { "−" } else { "+" };
                if ArenaMap::GENERATION_AXIS.contains(&target) {
                    let key = match (axis, delta) {
                        (0, -1) => "narrower",
                        (0, _) => "wider",
                        (_, -1) => "shorter",
                        _ => "taller",
                    };
                    p.spawn((
                        edit(glyph, move |d| d.dimensions[axis] = target),
                        AccessibleLabel::new(tr(key)),
                    ));
                } else {
                    p.spawn(disabled(glyph));
                }
            }
        }
    });
    p.spawn(row()).with_children(|p| {
        p.spawn((
            compact(&tr("randomMap")),
            observe(
                |_: On<Activate>, mut f: ResMut<Field>, mut d: ResMut<Draft>| {
                    d.note = f
                        .generate(d.dimensions)
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                },
            ),
        ));
        if field.can_save() {
            p.spawn((
                compact(&tr("saveMap")),
                observe(
                    |_: On<Activate>, mut f: ResMut<Field>, mut d: ResMut<Draft>| {
                        d.note = Folder::Maps
                            .path()
                            .and_then(|dir| f.save_to(&dir))
                            .err()
                            .map(|e| format!("{}: {e}", tr("mapSaveFailed")))
                            .unwrap_or_default();
                    },
                ),
            ));
        }
    });
}

fn clock_controls(p: &mut ChildSpawnerCommands, d: &Draft) {
    p.spawn(headline(&tr("limit"), 20.));
    p.spawn(row()).with_children(|p| {
        p.spawn(toggle(&tr("minute"), d.limit == LimitMode::Default, |d| {
            d.limit = LimitMode::Default;
        }));
        p.spawn(toggle(
            &tr("unlimited"),
            d.limit == LimitMode::Endless,
            |d| d.limit = LimitMode::Endless,
        ));
        p.spawn(toggle(&tr("custom"), d.limit == LimitMode::Custom, |d| {
            d.limit = LimitMode::Custom;
        }));
    });
    if d.limit == LimitMode::Custom {
        p.spawn((
            Node {
                padding: UiRect::axes(px(12), px(8)),
                min_width: px(125),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(paper()),
            children![label(format!("{}▏ s", d.seconds), 16., ink())],
        ));
    }
}
