use super::*;
use bevy::{
    animation::{AnimatedBy, AnimationTargetId, animated_field},
    app::AnimationSystems,
    ui::UiSystems,
};

const OPEN: f32 = 0.16;
const CLOSE: f32 = 0.10;

#[derive(Component, Default, Reflect, Clone)]
pub(super) struct Reveal {
    amount: f32,
}

#[derive(Resource)]
struct Clip {
    graph: Handle<AnimationGraph>,
    node: AnimationNodeIndex,
    target: AnimationTargetId,
}

impl FromWorld for Clip {
    fn from_world(world: &mut World) -> Self {
        let target = AnimationTargetId::from_name(&Name::new("panel-reveal"));
        let mut clip = AnimationClip::default();
        clip.add_curve_to_target(
            target,
            AnimatableCurve::new(
                animated_field!(Reveal::amount),
                AnimatableKeyframeCurve::new([
                    (0., 0.),
                    (0.035, 0.46),
                    (0.08, 0.84),
                    (0.12, 0.975),
                    (OPEN, 1.),
                ])
                .unwrap(),
            ),
        );
        let clip = world.resource_mut::<Assets<AnimationClip>>().add(clip);
        let (graph, node) = AnimationGraph::from_clip(clip);
        Self {
            graph: world.resource_mut::<Assets<AnimationGraph>>().add(graph),
            node,
            target,
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Clip>()
        .add_systems(
            Update,
            (
                bind.after(sync),
                reverse_or_finish.after(bind).run_if(in_state(Page::Menu)),
            ),
        )
        .add_systems(
            PostUpdate,
            apply.after(AnimationSystems).before(UiSystems::Layout),
        );
}

fn bind(mut c: Commands, clip: Res<Clip>, added: Query<Entity, Added<Reveal>>) {
    for e in &added {
        let mut player = AnimationPlayer::default();
        player.start(clip.node);
        c.entity(e).insert((
            player,
            AnimationGraphHandle(clip.graph.clone()),
            clip.target,
            AnimatedBy(e),
        ));
    }
}

fn reverse_or_finish(
    mut panel: ResMut<Panel>,
    clip: Res<Clip>,
    mut cards: Query<&mut AnimationPlayer, With<Reveal>>,
) {
    if *panel != Panel::Closing {
        return;
    }
    let Ok(mut player) = cards.single_mut() else {
        *panel = Panel::Closed;
        return;
    };
    let Some(active) = player.animation(clip.node) else {
        *panel = Panel::Closed;
        return;
    };

    if active.speed() >= 0. {
        // 从当前可见姿态反向播放，包括面板展开到一半就被关闭的情况。
        let pos = active.seek_time().clamp(0., OPEN);
        player
            .start(clip.node)
            .set_seek_time(pos)
            .set_speed(-OPEN / CLOSE);
    } else if active.is_finished() {
        *panel = Panel::Closed;
    }
}

type Faces<'w, 's> =
    Query<'w, 's, (Entity, &'static Reveal, Option<&'static mut BoxShadow>), Changed<Reveal>>;
type PaintedFaces = Or<(With<Reveal>, With<Skin>)>;

fn apply(
    mut cards: Faces,
    children: Query<&Children>,
    // 布局节点和文字节点默认也带有透明的 BackgroundColor。
    mut fills: Query<&mut BackgroundColor, PaintedFaces>,
    mut labels: Query<&mut TextColor>,
    frost: Query<&MaterialNode<FrostedMenu>>,
    mut materials: ResMut<Assets<FrostedMenu>>,
) {
    for (card, reveal, shadow) in &mut cards {
        let p = reveal.amount.clamp(0., 1.);
        for e in std::iter::once(card).chain(children.iter_descendants(card)) {
            if let Ok(mut fill) = fills.get_mut(e) {
                fill.0.set_alpha(p);
            }
            if let Ok(mut text) = labels.get_mut(e) {
                text.0.set_alpha(p);
            }
        }

        if let Some(mut shadow) = shadow {
            for layer in &mut shadow.0 {
                layer.color.set_alpha(super::SHADOW_ALPHA * p);
            }
        }
        if let Ok(handle) = frost.single()
            && materials.get(&handle.0).is_some_and(|m| m.reveal != p)
        {
            materials.get_mut(&handle.0).unwrap().reveal = p;
        }
    }
}
