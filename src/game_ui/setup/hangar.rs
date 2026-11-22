use super::*;
use tank_arena::bot::Script;

#[derive(Component)]
struct ModelCard(PathBuf);

#[derive(Component)]
pub(super) struct Rack;

#[derive(Component)]
pub(super) struct Destination;

#[derive(Default)]
pub(super) struct CatalogView {
    root: Option<Entity>,
    page: usize,
    pages: usize,
    full: bool,
    versions: Vec<(PathBuf, Option<tank_arena::local_files::Entry>)>,
}

/// 仅重建发生变化的页，不因普通阵容编辑丢失卡片的指针目标。
pub(super) fn sync(
    mut c: Commands,
    rack: Single<Entity, With<Rack>>,
    d: Res<Draft>,
    (looks, prefs): (Res<tank_manager::Looks>, Res<locale::Preferences>),
    mut images: arena_view::VehicleImages,
    mut view: Local<CatalogView>,
    profiles: Res<Profiles>,
) {
    let visible = d.visible_models();
    let full = d.seats().count() >= MAX_SLOTS;
    let pages = d.models.len().div_ceil(MODELS_PER_PAGE);
    let reset = view.root != Some(*rack)
        || view.page != d.model_page
        || view.pages != pages
        || view.full != full
        || looks.is_changed()
        || prefs.is_changed();
    if !reset && !d.is_changed() && !profiles.is_changed() {
        return;
    }

    let versions = visible
        .iter()
        .map(|p| (p.clone(), profiles.version(p).cloned()))
        .collect::<Vec<_>>();
    if !reset && versions == view.versions {
        return;
    }

    view.root = Some(*rack);
    view.page = d.model_page;
    view.pages = pages;
    view.full = full;
    view.versions = versions;
    c.entity(*rack)
        .despawn_children()
        .with_children(|p| render(p, &d, &profiles, &looks, &mut images));
}

fn destination(d: &Draft) -> String {
    if d.seats().count() >= MAX_SLOTS {
        tr("lineupFull")
    } else {
        rust_i18n::t!("addToTeam", n = d.selected_team.0).into_owned()
    }
}

pub(super) fn update_destination(d: Res<Draft>, mut target: Single<&mut Text, With<Destination>>) {
    if d.is_changed() {
        target.set_if_neq(Text::new(destination(&d)));
    }
}

fn render(
    p: &mut ChildSpawnerCommands,
    d: &Draft,
    profiles: &Profiles,
    looks: &tank_manager::Looks,
    images: &mut arena_view::VehicleImages,
) {
    let full = d.seats().count() >= MAX_SLOTS;
    let pages = d.models.len().div_ceil(MODELS_PER_PAGE);
    p.spawn(headline(&tr("tankModels"), 20.));
    if d.models.is_empty() {
        p.spawn(label(tr("emptyModels"), 15., quiet()));
        return;
    }

    p.spawn((Destination, eyebrow(&destination(d))));
    p.spawn(Node {
        width: percent(100),
        ..column(6.)
    })
    .with_children(|p| {
        for path in d.visible_models() {
            let script = profiles.script(path);
            let name = profiles.name(path);
            let mut card = p.spawn(Node {
                width: percent(100),
                height: px(52),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::horizontal(px(10)),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            });
            if full {
                card.insert((
                    BackgroundColor(paper()),
                    Hovered::default(),
                    Pickable::default(),
                ));
            } else {
                card.insert((
                    ModelCard(path.clone()),
                    Button,
                    AccessibleLabel::new(name.to_string()),
                    GlobalZIndex(0),
                    Pickable::default(),
                    grouping::tile_surface(Color::srgb_u8(226, 234, 220)),
                    observe(
                        |e: On<Activate>, cards: Query<&ModelCard>, mut d: ResMut<Draft>| {
                            if let Ok(model) = cards.get(e.entity) {
                                let team = d.selected_team;
                                d.deploy(&model.0, team);
                            }
                        },
                    ),
                    observe(drag),
                    observe(drag_end),
                    observe(cancel),
                ));
            }
            card.with_children(|p| {
                p.spawn(
                    images
                        .model(looks.resolve(path, script.map(Script::text)).as_ref())
                        .icon(quiet()),
                );
                p.spawn(grouping::clipped_name(&name));
                if let Some(script) = script {
                    tip(p, script);
                }
            });
        }
    });
    pager(p, d.model_page, pages, |page| {
        observe(move |_: On<Activate>, mut d: ResMut<Draft>| d.model_page = page)
    });
}

const TIP_DELAY: Duration = Duration::from_millis(650);

#[derive(Component, Default)]
pub(super) struct CardTip {
    since: Option<Duration>,
}

fn tip(p: &mut ChildSpawnerCommands, script: &Script) {
    if script.description().is_none() && script.author().is_none() {
        return;
    }

    // 候选列位于窗口最右侧，资料卡向左展开，不接收鼠标事件。
    p.spawn((
        CardTip::default(),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            right: percent(100),
            top: px(0),
            width: px(260),
            padding: UiRect::all(px(14)),
            border_radius: BorderRadius::all(px(9)),
            ..column(5.)
        },
        BackgroundColor(ink()),
        GlobalZIndex(30),
        Pickable::IGNORE,
    ))
    .with_children(|p| {
        if let Some(description) = script.description() {
            p.spawn((label(description, 15., Color::WHITE), Pickable::IGNORE));
        }
        if let Some(author) = script.author() {
            p.spawn((
                label(rust_i18n::t!("botAuthor", author = author), 14., lime()),
                Pickable::IGNORE,
            ));
        }
    });
}

pub(super) fn update_tips(
    time: Res<Time<Real>>,
    cards: Query<(&Hovered, &Pickable, Has<bevy::ui::Pressed>)>,
    mut tips: Query<(&ChildOf, &mut CardTip, &mut Node)>,
) {
    let now = time.elapsed();
    for (parent, mut tip, mut node) in &mut tips {
        let hovered = cards
            .get(parent.parent())
            .is_ok_and(|(h, p, pressed)| h.get() && p.is_hoverable && !pressed);
        let display = if hovered {
            let since = *tip.since.get_or_insert(now);
            if now.saturating_sub(since) >= TIP_DELAY {
                Display::Flex
            } else {
                Display::None
            }
        } else {
            tip.since = None;
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

pub(super) fn drop_into(team: TeamId) -> impl Bundle {
    observe(
        move |ev: On<PointerDragDrop>,
              cards: Query<(&ModelCard, &Pickable)>,
              mut d: ResMut<Draft>| {
            if ev.button == PointerButton::Primary
                && let Ok((model, picking)) = cards.get(ev.dropped)
                && !picking.is_hoverable
            {
                d.deploy(&model.0, team);
            }
        },
    )
}

// 拖动时只移动画面；改动草稿会重建指针命中区域。
type DragCards<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut UiTransform,
        &'static mut GlobalZIndex,
        &'static mut Pickable,
    ),
    With<ModelCard>,
>;

// 触控板轻点时，按下和松开的位置可能相差几个逻辑像素。
const DRAG_SLOP: f32 = 6.;

fn drag(e: On<PointerDrag>, mut cards: DragCards, mut commands: Commands) {
    if e.button == PointerButton::Primary
        && let Ok((mut pose, mut z, mut pick)) = cards.get_mut(e.entity)
    {
        if pick.is_hoverable {
            if e.distance.length_squared() < DRAG_SLOP * DRAG_SLOP {
                return;
            }

            // 拖回起点后松手也不能触发模型按钮。
            commands.entity(e.entity).remove::<bevy::ui::Pressed>();
            z.0 = 20;
            *pick = Pickable::IGNORE;
        }
        pose.translation = Val2::px(e.distance.x, e.distance.y);
    }
}

fn reset(id: Entity, cards: &mut DragCards) {
    if let Ok((mut pose, mut z, mut pick)) = cards.get_mut(id) {
        pose.translation = Val2::ZERO;
        z.0 = 0;
        *pick = Pickable::default();
    }
}

fn drag_end(e: On<PointerDragEnd>, mut cards: DragCards) {
    if e.button == PointerButton::Primary {
        reset(e.entity, &mut cards);
    }
}

fn cancel(e: On<PointerCancel>, mut cards: DragCards) {
    reset(e.entity, &mut cards);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            events::Pointer,
            pointer::{Location, PointerId},
        },
    };

    #[test]
    fn tooltips_require_continuous_hover_and_never_intercept_picking() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .add_systems(Update, update_tips);
        let script = Script::from_source("model", "-- @description Hello\n-- @author Ada").unwrap();
        let card = app
            .world_mut()
            .commands()
            .spawn((Hovered(true), Pickable::default()))
            .with_children(|p| tip(p, &script))
            .id();
        app.world_mut().flush();
        let panel = app.world().get::<Children>(card).unwrap()[0];
        let display = |app: &App| app.world().get::<Node>(panel).unwrap().display;
        let advance = |app: &mut App, dt| {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(dt);
            app.update();
        };
        app.update();
        advance(&mut app, TIP_DELAY - Duration::from_millis(1));

        assert_eq!(display(&app), Display::None);
        advance(&mut app, Duration::from_millis(1));

        assert_eq!(display(&app), Display::Flex);
        assert!(!app.world().get::<Pickable>(panel).unwrap().is_hoverable);
        app.world_mut().entity_mut(card).insert(Hovered(false));
        app.update();

        assert_eq!(display(&app), Display::None);
        app.world_mut().entity_mut(card).insert(Hovered(true));
        advance(&mut app, TIP_DELAY);

        assert_eq!(display(&app), Display::None); // 新的一次悬停重新计时。
        app.world_mut().entity_mut(card).insert(Pickable::IGNORE);
        advance(&mut app, TIP_DELAY);
        assert_eq!(display(&app), Display::None);
    }

    fn pointer() -> Pointer {
        Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1080,
                    height: 720,
                },
                position: Vec2::ZERO,
            },
        )
    }

    #[test]
    fn only_a_real_primary_drop_on_the_team_or_its_children_deploys_a_member() {
        let mut w = bevy::prelude::World::new();
        // 注册 Bevy 遍历指针目标时使用的可选窗口组件。
        w.register_component::<Window>();
        let mut draft = Draft::default();
        let model = PathBuf::from("example.luau");
        draft.models.push(model.clone());
        w.insert_resource(draft);
        let card = w.spawn((ModelCard(model), Pickable::default())).id();
        let team = w.spawn(drop_into(TeamId(2))).id();
        let row = w.spawn(ChildOf(team)).id();
        w.flush();
        let drop = |button| PointerDragDrop {
            pointer: pointer(),
            entity: row,
            button,
            dropped: card,
            hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0., None, None),
        };
        w.trigger(drop(PointerButton::Primary)); // 尚未越过拖动阈值。
        w.entity_mut(card).insert(Pickable::IGNORE);
        w.trigger(drop(PointerButton::Secondary));

        assert_eq!(w.resource::<Draft>().seats().count(), 0);
        w.trigger(drop(PointerButton::Primary));
        let d = w.resource::<Draft>();
        assert_eq!(d.seats().count(), 1);
        assert_eq!(d.tank(SlotId(1)).unwrap().team, TeamId(2));
        assert_eq!(d.selected_team, TeamId(2));
    }

    #[test]
    fn small_movements_keep_the_tank_clickable_until_a_real_drag() {
        let mut w = bevy::prelude::World::new();
        let id = w
            .spawn((
                ModelCard("example.luau".into()),
                bevy::ui::Pressed,
                UiTransform::default(),
                GlobalZIndex(0),
                Pickable::default(),
            ))
            .observe(drag)
            .observe(drag_end)
            .id();
        let ptr = pointer();
        let moved = |distance: f32| PointerDrag {
            entity: id,
            pointer: ptr.clone(),
            button: PointerButton::Primary,
            distance: Vec2::X * distance,
            delta: Vec2::X,
        };
        w.trigger(moved(3.));

        assert!(w.get::<Pickable>(id).unwrap().is_hoverable);
        assert_eq!(w.get::<UiTransform>(id).unwrap().translation, Val2::ZERO);
        assert!(w.get::<bevy::ui::Pressed>(id).is_some());
        w.trigger(moved(12.));
        w.flush();

        assert!(w.get::<bevy::ui::Pressed>(id).is_none());
        w.trigger(moved(2.)); // 一旦被识别为拖动，即使划回起点也不再算点击。

        assert!(!w.get::<Pickable>(id).unwrap().is_hoverable);
        w.trigger(PointerDragEnd {
            entity: id,
            pointer: ptr,
            button: PointerButton::Primary,
            distance: Vec2::X * 2.,
        });

        assert!(w.get::<Pickable>(id).unwrap().is_hoverable);
        assert_eq!(w.get::<UiTransform>(id).unwrap().translation, Val2::ZERO);
    }
}
