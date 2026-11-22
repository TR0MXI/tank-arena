mod overlay;
mod toolbar;

use super::*;
pub(super) use overlay::{FrostedMenu, Panel, frost, modal_card, modal_layer};

#[derive(Component)]
struct Home;

pub(super) fn install(app: &mut App) {
    app.add_systems(OnEnter(Page::Menu), |mut c: Commands| {
        c.spawn((
            Home,
            DespawnOnExit(Page::Menu),
            root(),
            BackgroundColor(paper()),
        ));
    })
    .add_systems(Update, refresh.run_if(in_state(Page::Menu)));
    toolbar::install(app);
    overlay::install(app);
}

fn refresh(
    mut c: Commands,
    home: Single<(Entity, Ref<Home>)>,
    prefs: Res<locale::Preferences>,
    art: Res<Art>,
) {
    if !home.1.is_added() && !prefs.is_changed() {
        return;
    }

    c.entity(home.0).despawn_children().with_children(|p| {
        p.spawn(Node {
            flex_grow: 1.,
            padding: UiRect::horizontal(px(44)),
            column_gap: px(44),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Node {
                    width: percent(55),
                    min_width: px(0),
                    flex_shrink: 0.,
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(px(18)),
                    ..column(0.)
                },
                BackgroundColor(ink()),
            ))
            .with_children(|p| {
                p.spawn((
                    PageIntro,
                    ImageNode::new(art.duel.clone()),
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        min_height: px(0),
                        aspect_ratio: Some(640. / 422.),
                        border_radius: BorderRadius::all(px(18)),
                        ..default()
                    },
                ));
            });
            p.spawn(Node {
                flex_grow: 1.,
                flex_basis: px(0),
                min_width: px(0),
                ..column(16.)
            })
            .with_children(|p| {
                p.spawn((PageIntro, headline("Tank Arena", 54.)));
                p.spawn((primary(&tr("start")), navigate(Page::Setup)));
            });
        });
    });
}
