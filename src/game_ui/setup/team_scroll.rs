//! 滚动状态不存入阵容草稿，以免编辑卡片时面板跳动、滚动时重建面板。

use super::*;
use bevy::{input::mouse::MouseScrollPixelsPerLine, ui::ScrollPosition};

const CARD: f32 = 308.;
const GAP: f32 = 12.;
const FADE: f32 = 32.;
// 边框可以进入渐隐区，静止时首尾两个控件仍须清晰可见。
const END_INSET: f32 = 8.;

#[derive(Component)]
pub(super) struct Viewport;

#[derive(Resource, Default)]
pub(super) struct TeamScroll {
    viewport_height: f32,
    drag_origin: f32,
    dragging: bool,
    offset: f32,
    target: f32,
}

impl TeamScroll {
    fn limit(&self, teams: usize) -> f32 {
        let tiles = teams + usize::from(teams < MAX_SLOTS);
        let rows = tiles.div_ceil(2) as f32;
        (2. * END_INSET + rows * (CARD + GAP) - GAP - self.viewport_height).max(0.)
    }

    pub(super) fn reveal(&mut self, index: usize) {
        let top = (index / 2) as f32 * (CARD + GAP);
        let bottom = top + CARD + 2. * END_INSET;
        self.target = self
            .target
            .clamp((bottom - self.viewport_height).clamp(0., top), top);
    }
}

pub(super) fn grid(p: &mut ChildSpawnerCommands, cards: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node {
        width: percent(100),
        flex_grow: 1.,
        flex_basis: px(0),
        min_height: px(0),
        ..default()
    })
    .with_children(|p| {
        p.spawn((
            Viewport,
            ScrollPosition::default(),
            Node {
                width: percent(100),
                height: percent(100),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .observe(
            |mut e: On<PointerScroll>,
             ratio: Res<MouseScrollPixelsPerLine>,
             d: Res<Draft>,
             mut scroll: ResMut<TeamScroll>| {
                e.propagate(false);
                scroll.target =
                    (scroll.target - e.to_pixels(&ratio).y).clamp(0., scroll.limit(d.teams.len()));
            },
        )
        .observe(|_: On<PointerDragStart>, mut scroll: ResMut<TeamScroll>| {
            scroll.drag_origin = scroll.offset;
            scroll.dragging = false;
        })
        .observe(drag)
        .with_children(|p| {
            p.spawn(Node {
                width: percent(100),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(2, 1.),
                grid_auto_rows: vec![GridTrack::px(CARD)],
                padding: UiRect::vertical(px(END_INSET)),
                column_gap: px(GAP),
                row_gap: px(GAP),
                align_self: AlignSelf::FlexStart,
                ..default()
            })
            .with_children(cards);
        });
        for top in [true, false] {
            let tint = paper();
            let gradient = LinearGradient::new(
                if top {
                    LinearGradient::TO_BOTTOM
                } else {
                    LinearGradient::TO_TOP
                },
                vec![
                    ColorStop::new(tint, percent(0)),
                    ColorStop::new(tint.with_alpha(0.38), percent(45)),
                    ColorStop::new(tint.with_alpha(0.), percent(100)),
                ],
            );
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: px(FADE),
                    top: if top { px(0) } else { Val::Auto },
                    bottom: if top { Val::Auto } else { px(0) },
                    ..default()
                },
                BackgroundGradient(vec![gradient.into()]),
                Pickable::IGNORE,
            ));
        }
    });
}

fn drag(
    mut ev: On<PointerDrag>,
    mut scroll: ResMut<TeamScroll>,
    d: Res<Draft>,
    scale: Res<UiScale>,
    pressed: Query<(), With<bevy::ui::Pressed>>,
    parents: Query<&ChildOf>,
    mut c: Commands,
) {
    if ev.button != PointerButton::Primary || scroll.limit(d.teams.len()) == 0. {
        return;
    }

    if !scroll.dragging {
        if ev.distance.y.abs() < 6. || ev.distance.y.abs() < ev.distance.x.abs() {
            return;
        }

        scroll.dragging = true;

        // 从增减按钮上开始滑动，松手时不再算点击。
        let start = ev.original_event_target();
        for id in std::iter::once(start).chain(parents.iter_ancestors(start)) {
            if pressed.contains(id) {
                c.entity(id).remove::<bevy::ui::Pressed>();
            }
        }
    }
    ev.propagate(false);
    scroll.target =
        (scroll.drag_origin - ev.distance.y / scale.0).clamp(0., scroll.limit(d.teams.len()));
    scroll.offset = scroll.target;
}

pub(super) fn animate(
    time: Res<Time<Real>>,
    d: Res<Draft>,
    mut scroll: ResMut<TeamScroll>,
    mut view: Single<(&ComputedNode, &mut ScrollPosition), With<Viewport>>,
) {
    let (layout, position) = &mut *view;
    // 重建的面板要等布局结束才有尺寸，在此之前保留上次偏移量。
    if layout.size().y > 0. {
        scroll.viewport_height = layout.size().y * layout.inverse_scale_factor();
    }
    let max = scroll.limit(d.teams.len());
    scroll.target = scroll.target.clamp(0., max);
    let delta = scroll.target - scroll.offset;
    scroll.offset = if delta.abs() < 0.3 {
        scroll.target
    } else {
        (scroll.offset + delta * (1. - (-time.delta_secs() * 22.).exp())).clamp(0., max)
    };
    if position.y != scroll.offset {
        position.y = scroll.offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_fades_are_pinned_outside_the_scroll_content_and_pass_input_through() {
        let mut world = bevy::prelude::World::new();
        world
            .commands()
            .spawn_empty()
            .with_children(|p| grid(p, |_| {}));
        world.flush();
        let viewport_parent = world
            .query_filtered::<&ChildOf, With<Viewport>>()
            .single(&world)
            .unwrap()
            .parent();
        let mut anchors = Vec::new();
        for (node, parent, pick) in world
            .query_filtered::<(&Node, &ChildOf, &Pickable), With<BackgroundGradient>>()
            .iter(&world)
        {
            assert_eq!(parent.parent(), viewport_parent);
            assert!(!pick.is_hoverable && !pick.should_block_lower);
            assert_eq!(node.position_type, PositionType::Absolute);
            assert_eq!(node.height, px(FADE));
            anchors.push((node.top, node.bottom));
        }

        assert_eq!(anchors.len(), 2);
        assert!(anchors.contains(&(px(0), Val::Auto)) && anchors.contains(&(Val::Auto, px(0))));
    }

    #[test]
    fn new_cards_are_revealed_without_overscrolling_the_last_row() {
        for height in [480., 590., 640., 900.] {
            let mut scroll = TeamScroll {
                viewport_height: height,
                ..default()
            };
            assert_eq!(scroll.limit(1), 0.);
            for n in 2..=MAX_SLOTS {
                scroll.reveal(n - 1);
                let top = ((n - 1) / 2) as f32 * (CARD + GAP);
                assert!(
                    scroll.target <= top && scroll.target + height >= top + CARD + 2. * END_INSET
                );
                assert!(scroll.target <= scroll.limit(n));
            }

            assert_eq!(scroll.limit(MAX_SLOTS - 1), scroll.limit(MAX_SLOTS));
            assert_eq!(scroll.target, scroll.limit(MAX_SLOTS));
            scroll.reveal(0);
            assert_eq!(scroll.target, 0.);
        }
    }
}
