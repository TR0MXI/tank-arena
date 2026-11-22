use super::*;
use crate::game_ui::match_text::{clock, event};
use tank_arena::replay::RecordStatus;

#[derive(Component)]
pub(super) struct Score;

#[derive(Component)]
pub(super) struct Members;

#[derive(Component)]
pub(super) struct Recap;

pub(super) fn spawn(mut c: Commands) {
    c.spawn((
        DespawnOnExit(Page::Live),
        beside_menu(),
        BackgroundColor(hud_bg()),
        children![
            label(format!("●  {}", tr("live")), 16., lime()),
            (Score, label("", 18., Color::WHITE)),
            (
                dark(&tr("teams")),
                observe(
                    |_: On<Activate>, mut g: ResMut<LiveSession>| g.show_roster = !g.show_roster
                )
            )
        ],
    ));
    c.spawn((
        DespawnOnExit(Page::Live),
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            top: px(12),
            ..default()
        },
        children![(
            dark(&tr("endMatch")),
            observe(
                |_: On<Activate>, mut g: ResMut<LiveSession>| g.confirm_stop =
                    Some(StopPrompt::Result)
            )
        )],
    ));
    c.spawn((
        DespawnOnExit(Page::Live),
        bottom_dock(),
        Visibility::Hidden,
        children![(Recap, label("", 15., Color::WHITE))],
    ));
    c.spawn((
        DespawnOnExit(Page::Live),
        Members,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(78),
            padding: UiRect::all(px(13)),
            border_radius: BorderRadius::all(px(9)),
            ..default()
        },
        BackgroundColor(hud_bg()),
        Visibility::Hidden,
        children![label("", 16., Color::WHITE)],
    ));
}

#[derive(PartialEq, Eq)]
struct ScoreKey {
    entity: Entity,
    second: u64,
    limit: Option<u64>,
    alive: [Option<(tank_arena::sim::TeamId, bool)>; tank_arena::sim::MAX_SLOTS],
}

#[derive(Default)]
pub(super) struct HudCache {
    score: Option<ScoreKey>,
    cue: Option<(Entity, Option<tank_arena::sim::Event>, bool)>,
    roster_open: bool,
}

type HudReadouts<'w, 's> = (
    Single<'w, 's, Entity, With<Score>>,
    Single<'w, 's, (Entity, &'static ChildOf), With<Recap>>,
);

pub(super) fn update(
    g: Res<LiveSession>,
    frame: Res<LiveFrame>,
    mut texts: Query<&mut Text>,
    mut cache: Local<HudCache>,
    readouts: HudReadouts,
    members: Single<(&Children, Entity), With<Members>>,
    mut visibility: Query<&mut Visibility>,
) {
    if g.finished.is_some() && !frame.is_changed() {
        return; // 赛果停留期间保留最后一次状态栏读数，不持续重建文本。
    }
    let Some(f) = frame.get() else { return };

    let score = ScoreKey {
        entity: *readouts.0,
        second: f.tick / u64::from(HZ),
        limit: g.clock.seconds(),
        alive: std::array::from_fn(|i| f.tanks.get(i).map(|t| (t.seat.team, t.alive))),
    };
    let roster_changed = g.show_roster
        && (!cache.roster_open
            || cache
                .score
                .as_ref()
                .is_none_or(|old| old.entity != score.entity || old.alive != score.alive));
    cache.roster_open = g.show_roster;
    if cache.score.as_ref() != Some(&score) {
        cache.score = Some(score);
        let mut teams = std::collections::BTreeMap::new();
        for t in &f.tanks {
            let n = teams.entry(t.seat.team).or_insert(0);
            *n += usize::from(t.alive);
        }

        let counts = teams
            .iter()
            .map(|(t, n)| format!("{} {}: {}", tr("team"), t.0, n))
            .collect::<Vec<_>>()
            .join("   ·   ");
        if let Ok(mut text) = texts.get_mut(*readouts.0) {
            text.0 = format!("{}   |   {}", clock(f.tick, g.clock), counts);
        }
    }
    let incomplete = matches!(
        g.record.as_ref().map(Recording::status),
        Some(RecordStatus::Incomplete(_))
    );
    let cue = (readouts.1.0, f.cue, incomplete);
    if cache.cue != Some(cue) {
        cache.cue = Some(cue);
        let mut msg = event(f.cue);
        if incomplete {
            msg = format!("{}  {msg}", tr("incomplete"));
        }
        if let Ok(mut v) = visibility.get_mut(readouts.1.1.parent()) {
            v.set_if_neq(if msg.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Visible
            });
        }
        if let Ok(mut text) = texts.get_mut(readouts.1.0) {
            text.set_if_neq(Text::new(msg));
        }
    }
    if let Ok(mut v) = visibility.get_mut(members.1) {
        v.set_if_neq(if g.show_roster {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
    if roster_changed && let Ok(mut text) = texts.get_mut(members.0[0]) {
        let roster = f
            .tanks
            .iter()
            .zip(&g.names)
            .map(|(t, n)| {
                format!(
                    "{} {}   ·   #{} {}   ·   {}",
                    tr("team"),
                    t.seat.team.0,
                    t.seat.slot.0,
                    n,
                    tr(if t.alive { "alive" } else { "out" })
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        text.set_if_neq(Text::new(roster));
    }
}
