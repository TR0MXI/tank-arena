use super::*;
use tank_arena::bot::Script;

#[derive(Component)]
pub(super) struct Legend;

pub(super) fn spawn(c: &mut Commands) {
    c.spawn((DespawnOnExit(Page::Tanks), root(), BackgroundColor(paper())))
        .with_children(|p| {
            p.spawn(page_body()).with_children(|p| {
                p.spawn(page_header()).with_children(|p| {
                    p.spawn((PageIntro, headline(&tr("tankManager"), 30.)));
                });
                p.spawn(Node {
                    width: percent(100),
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|p| {
                    p.spawn(card(54.)).with_children(|p| {
                        p.spawn(spread()).with_children(|p| {
                            p.spawn(headline(&tr("pixelCanvas"), 23.));
                            p.spawn(eyebrow(&tr("frontRight")));
                        });
                        p.spawn((ToolBar, row()));
                        grid(p);
                        p.spawn((Legend, label(guides::legend(), 14., quiet())));
                        p.spawn((Swatches, column(6.)));
                    });
                    p.spawn(Node {
                        flex_grow: 1.,
                        flex_basis: px(0),
                        min_width: px(0),
                        ..column(14.)
                    })
                    .with_children(|p| {
                        p.spawn(card(100.)).with_children(|p| {
                            p.spawn(headline(&tr("draftPreview"), 20.));
                            p.spawn((
                                Node {
                                    width: percent(100),
                                    height: px(100),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border_radius: BorderRadius::all(px(8)),
                                    ..default()
                                },
                                BackgroundColor(ink()),
                            ))
                            .with_children(|p| {
                                p.spawn((
                                    DraftImage::Preview,
                                    arena_view::skin_image_node(Handle::default()),
                                    arena_view::art_box(120.),
                                ));
                            });
                            p.spawn((TeamCount, label("", 16., ink())));
                        });
                        p.spawn((Inspector, card(100.)));
                    });
                });
                p.spawn((Feedback, label("", 15., quiet())));
            });
        });
}

fn grid(p: &mut ChildSpawnerCommands) {
    let size = guides::canvas_size(tank_arena::sim::SIZES);
    p.spawn((
        Canvas,
        DraftImage::Canvas,
        arena_view::skin_image_node(Handle::default()),
        RelativeCursorPosition::default(),
        Node {
            width: px(size.x),
            height: px(size.y),
            flex_shrink: 0.,
            ..default()
        },
    ))
    .with_children(|p| {
        lines(p, size, false);
        p.spawn((
            FineLines,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| lines(p, size, true));
        guides::spawn(p);
    });
}

fn lines(p: &mut ChildSpawnerCommands, size: Vec2, fine: bool) {
    for vertical in [true, false] {
        let steps = if vertical { WIDTH } else { HEIGHT };
        // 两种模式都显示偶数网格线，高级模式额外叠加奇数网格线。
        for i in ((if fine { 1 } else { SCALE })..steps).step_by(SCALE) {
            let t = i as f32 / steps as f32;
            let (x, y, w, h) = if vertical {
                (t * size.x - 1., 0., 1., size.y)
            } else {
                (0., t * size.y - 1., size.x, 1.)
            };
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x),
                    top: px(y),
                    width: px(w),
                    height: px(h),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(150, 164, 157, 100)),
                Pickable::IGNORE,
            ));
        }
    }
}

pub(super) fn inspect(
    mut c: Commands,
    e: Res<Editor>,
    looks: Res<Looks>,
    inspector: Single<Entity, With<Inspector>>,
    eligible: Res<Eligible>,
    mut feedback: Single<&mut Text, With<Feedback>>,
    mut vehicles: arena_view::VehicleImages,
) {
    if !eligible.is_changed() && !e.is_changed() && !looks.is_changed() {
        return;
    }

    feedback.0.clone_from(&e.note);
    c.entity(*inspector).despawn_children().with_children(|p| {
        p.spawn(row()).with_children(|p| {
            for (shelf, key) in [
                (Shelf::Target, "skinTarget"),
                (Shelf::Designs, "savedDesigns"),
            ] {
                p.spawn((
                    choice(&tr(key), e.shelf == shelf),
                    observe(move |_: On<Activate>, mut e: ResMut<Editor>| e.shelf = shelf),
                ));
            }
        });
        if e.shelf == Shelf::Designs {
            pager(p, e.saved, looks.library.designs.len(), |i| {
                act(Action::Saved(i))
            });
            if let Some(s) = looks.library.designs.get(e.saved) {
                p.spawn(row()).with_children(|p| {
                    p.spawn((
                        arena_view::skin_image_node(thumbnail(s, &mut vehicles.images)),
                        arena_view::art_box(66.),
                    ));
                    p.spawn(label(
                        format!("{} {:02}", tr("designNumber"), e.saved + 1),
                        15.,
                        quiet(),
                    ));
                    p.spawn((compact(&tr("loadDesign")), act(Action::Load)));
                });
            } else {
                p.spawn(label(tr("noSavedDesigns"), 14., quiet()));
            }
            return;
        }

        pager(p, e.target, e.models.len(), |i| act(Action::Target(i)));
        if let Some(path) = e.model() {
            let script = Script::read_entry(path).ok();
            let active = looks.resolve(path, script.as_ref().map(Script::text));
            let name = script
                .as_ref()
                .map(|s| s.name().to_owned())
                .unwrap_or_else(|| tank_arena::bot::catalog::tank_name(path));
            let source = if looks.binding(path).is_some() {
                "userLook"
            } else if active.is_some() {
                "authorLook"
            } else {
                "systemLook"
            };
            let art = vehicles.model(active.as_ref());
            p.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            })
            .with_children(|p| {
                p.spawn(art.icon(Color::srgb_u8(55, 139, 151)));
                p.spawn(Node {
                    flex_grow: 1.,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..column(0.)
                })
                .with_children(|p| {
                    p.spawn((headline(&name, 17.), TextLayout::no_wrap()));
                    p.spawn(label(tr(source), 13., quiet()));
                });
            });
            p.spawn(row()).with_children(|p| {
                if eligible.0 {
                    p.spawn((compact(&tr("applyDesign")), act(Action::Apply)));
                } else {
                    p.spawn(disabled(&tr("applyDesign")));
                }
                p.spawn((compact(&tr("useTarget")), act(Action::UseTarget)));
                if looks.binding(path).is_some() {
                    p.spawn((compact(&tr("removeSkin")), act(Action::Restore)));
                }
            });
        } else {
            p.spawn(label(tr("noSkinTarget"), 14., quiet()));
        }
    });
}

pub(super) fn brushes(
    mut c: Commands,
    brush: Res<Brush>,
    eligible: Res<Eligible>,
    swatches: Single<Entity, With<Swatches>>,
) {
    if !eligible.is_changed() && !brush.is_changed() {
        return;
    }

    c.entity(*swatches).despawn_children().with_children(|p| {
        p.spawn(eyebrow(&format!(
            "{}   /   {}",
            tr("pixelBrush"),
            tr(BRUSHES[brush.0])
        )));
        p.spawn(row()).with_children(|p| {
            for (i, &v) in PALETTE.iter().enumerate() {
                p.spawn((
                    Button,
                    AccessibleLabel::new(tr(BRUSHES[i])),
                    observe(move |_: On<Activate>, mut brush: ResMut<Brush>| brush.0 = i),
                    Node {
                        width: px(32),
                        height: px(36),
                        padding: UiRect::all(px(3)),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BorderColor::all(if i == brush.0 { ink() } else { Color::NONE }),
                ))
                .with_children(|p| {
                    p.spawn((
                        Node {
                            width: percent(100),
                            height: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(Color::from(Srgba::from_u8_array(checker_pixel(v, i)))),
                        Pickable::IGNORE,
                    ))
                    .with_children(|p| {
                        if i == 0 {
                            p.spawn((label("×", 16., quiet()), Pickable::IGNORE));
                        }
                    });
                });
            }
        });
        if eligible.0 {
            p.spawn((primary(&tr("saveDesign")), act(Action::Save)));
        } else {
            p.spawn(disabled_primary(&tr("saveDesign")));
        }
    });
}

pub(super) fn tools(
    mut c: Commands,
    tools: Res<Tools>,
    bar: Single<Entity, With<ToolBar>>,
    mut legend: Single<&mut Node, With<Legend>>,
    mut overlay: Single<&mut Visibility, With<guides::Overlay>>,
    mut minor: Single<&mut Visibility, (With<FineLines>, Without<guides::Overlay>)>,
) {
    if !tools.is_changed() {
        return;
    }

    legend.display = if tools.guides {
        Display::Flex
    } else {
        Display::None
    };
    **minor = if tools.density == Density::Fine {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    **overlay = if tools.guides {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    c.entity(*bar).despawn_children().with_children(|p| {
        for (mode, key) in [
            (Density::Simple, "simpleLayout"),
            (Density::Fine, "fineLayout"),
        ] {
            p.spawn((
                choice(&tr(key), tools.density == mode),
                observe(move |_: On<Activate>, mut tools: ResMut<Tools>| tools.density = mode),
            ));
        }

        p.spawn((
            choice(&tr("showGuides"), tools.guides),
            observe(|_: On<Activate>, mut tools: ResMut<Tools>| tools.guides = !tools.guides),
        ));
    });
}
