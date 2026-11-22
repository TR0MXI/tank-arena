mod library;
mod loading;
mod speed;

use super::*;
use bevy::ui::RelativeCursorPosition;
use library::Library;
pub(super) use loading::open;
pub(super) use speed::Rate;
use std::path::PathBuf;
use tank_arena::replay::Playback;

#[derive(Resource)]
pub(super) struct ReplaySession {
    pub(super) player: Playback,
    playing: bool,
    pub(super) carry: f64,
    pub(super) tail: f32,
    pub(super) show_roster: bool,
    speed: Rate,
    notice: Option<std::io::Error>,
}

impl ReplaySession {
    fn new(player: Playback) -> Self {
        Self {
            player,
            playing: true,
            carry: 0.,
            tail: 0.,
            show_roster: false,
            speed: Rate::default(),
            notice: None,
        }
    }
}

fn prepare_art(
    g: Res<ReplaySession>,
    mut images: arena_view::VehicleImages,
    mut art: ResMut<arena_view::MatchArt>,
) {
    *art = images.for_roster(
        g.player.header.roster.iter().map(|b| b.seat.slot),
        g.player.skins(),
    );
}

#[derive(Component)]
struct Readout;

#[derive(Component)]
struct ReplayCue;

#[derive(Component)]
struct Fill;

pub(super) fn install(app: &mut App) {
    speed::install(app);
    loading::install(app);
    library::install(app);
    app.add_systems(
        OnEnter(Page::Replay),
        (prepare_art, (arena_view::draw, controls)).chain(),
    )
    .add_systems(
        Update,
        (timeline, play, transport_face, show_roster)
            .chain()
            .run_if(in_state(Page::Replay)),
    )
    .add_systems(OnExit(Page::Replay), |mut c: Commands| {
        c.remove_resource::<ReplaySession>();
    });
}

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct ReplayRoster;

fn show_roster(g: Res<ReplaySession>, mut v: Single<&mut Visibility, With<ReplayRoster>>) {
    v.set_if_neq(if g.show_roster {
        Visibility::Visible
    } else {
        Visibility::Hidden
    });
}

const DOCK_HEIGHT: f32 = 44.;

fn dock_tint() -> Color {
    hud_bg().with_alpha(0.87)
}

fn dock_highlight() -> Color {
    Color::WHITE.with_alpha(0.09)
}

fn transport_bar() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(9),
            height: px(DOCK_HEIGHT),
            padding: UiRect::right(px(15)),
            border_radius: BorderRadius::MAX,
            // 倍速列表会展开到胶囊外框之外。
            overflow: Overflow::visible(),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(dock_tint()),
    )
}

fn controls(mut c: Commands, g: Res<ReplaySession>, art: Res<Art>) {
    let lineup = g
        .player
        .header
        .roster
        .iter()
        .map(|b| {
            format!(
                "{} {} · #{} {}",
                tr("team"),
                b.seat.team.0,
                b.seat.slot.0,
                b.name
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    c.spawn((
        DespawnOnExit(Page::Replay),
        beside_menu(),
        BackgroundColor(hud_bg()),
        children![
            label(tr("replay"), 16., lime()),
            (
                dark(&tr("teams")),
                observe(
                    |_: On<Activate>, mut g: ResMut<ReplaySession>| g.show_roster = !g.show_roster
                )
            )
        ],
    ));
    c.spawn((
        ReplayRoster,
        DespawnOnExit(Page::Replay),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(78),
            padding: UiRect::all(px(12)),
            border_radius: BorderRadius::all(px(9)),
            ..default()
        },
        BackgroundColor(hud_bg()),
        children![label(lineup, 16., Color::WHITE)],
    ));
    c.spawn((
        DespawnOnExit(Page::Replay),
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            top: px(12),
            ..row()
        },
        children![(dark(&tr("library")), navigate(Page::Library))],
    ));

    // 操作条和战报分居两侧，避免长句遮住底边的真实反弹点。
    c.spawn((
        DespawnOnExit(Page::Replay),
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            bottom: px(9),
            padding: UiRect::axes(px(12), px(8)),
            border_radius: BorderRadius::all(px(10)),
            max_width: percent(40),
            ..default()
        },
        BackgroundColor(hud_bg()),
        children![(ReplayCue, label("", 15., Color::WHITE))],
    ));
    c.spawn((DespawnOnExit(Page::Replay), transport_bar()))
        .with_children(|p| {
            let initial = transport_action(&g);
            p.spawn((
                PlayButton,
                art.icon_button(
                    initial,
                    DOCK_HEIGHT,
                    BorderRadius::left(px(DOCK_HEIGHT / 2.)),
                    dock_highlight(),
                ),
                observe(toggle_playback),
            ));
            p.spawn((
                art.icon_button(
                    Icon::Step,
                    DOCK_HEIGHT,
                    BorderRadius::ZERO,
                    dock_highlight(),
                ),
                observe(|_: On<Activate>, mut g: ResMut<ReplaySession>| {
                    g.playing = false;
                    g.carry = 0.;
                    let p = &mut g.player;
                    if p.ended() {
                        let (duration, hz) = (end_tail(p), p.header.tick_hz);
                        g.tail = (g.tail + 1. / hz as f32).min(duration);
                    } else if let Err(e) = p.advance() {
                        g.notice = Some(e);
                    }
                }),
            ));
            speed::picker(p, g.speed);
            p.spawn((
                RelativeCursorPosition::default(),
                Node {
                    width: px(212),
                    height: px(DOCK_HEIGHT),
                    margin: UiRect::axes(px(8), px(0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![(
                    Node {
                        width: percent(100),
                        height: px(8),
                        border_radius: BorderRadius::MAX,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                    BackgroundColor(Color::srgba_u8(210, 227, 218, 77)),
                    children![(
                        Fill,
                        Node {
                            height: percent(100),
                            width: percent(0),
                            ..default()
                        },
                        BackgroundColor(lime()),
                        Pickable::IGNORE
                    )]
                )],
            ));
            p.spawn((
                Readout,
                label("", 15., Color::WHITE),
                TextLayout::no_wrap().with_justify(Justify::Right),
                Node {
                    min_width: px(91),
                    ..default()
                },
            ));
        });
}

fn timeline(
    mouse: Res<ButtonInput<MouseButton>>,
    pos: Single<&RelativeCursorPosition>,
    mut g: ResMut<ReplaySession>,
) {
    if mouse.pressed(MouseButton::Left)
        && pos.cursor_over()
        && let Some(x) = pos.normalized.map(|p| (p.x + 0.5).clamp(0., 1.))
    {
        g.playing = false;
        g.tail = 0.;
        g.carry = 0.;
        {
            let p = &mut g.player;
            let n = (x as f64 * p.length() as f64).round() as u64;
            if n != p.world().tick()
                && let Err(e) = p.seek(n)
            {
                g.notice = Some(e);
            }
        }
    }
}

fn end_tail(p: &Playback) -> f32 {
    arena_view::tail_duration(p.world().tick(), p.world().hit_ticks(), p.header.tick_hz)
}

fn replay_finished(g: &ReplaySession) -> bool {
    g.player.ended() && g.tail >= end_tail(&g.player)
}

fn transport_action(g: &ReplaySession) -> Icon {
    if replay_finished(g) {
        Icon::Restart
    } else if g.playing {
        Icon::Pause
    } else {
        Icon::Play
    }
}

fn toggle_playback(_: On<Activate>, mut g: ResMut<ReplaySession>) {
    match transport_action(&g) {
        Icon::Restart => {
            if let Err(e) = g.player.seek(0) {
                g.notice = Some(e);
                return;
            }

            g.tail = 0.;
            g.carry = 0.;
            g.playing = true;
        }
        Icon::Play => g.playing = true,
        Icon::Pause => g.playing = false,
        _ => unreachable!("not a playback action"),
    }
}

fn transport_face(g: Res<ReplaySession>, mut icon: Single<&mut Icon, With<PlayButton>>) {
    icon.set_if_neq(transport_action(&g));
}

type CueHud<'w, 's> = (
    Single<'w, 's, (Entity, &'static ChildOf), With<ReplayCue>>,
    Query<'w, 's, &'static mut Visibility>,
);

fn play(
    time: Res<Time<Real>>,
    mut g: ResMut<ReplaySession>,
    hud: Single<Entity, With<Readout>>,
    (cue, mut visibility): CueHud,
    mut texts: Query<&mut Text>,
    mut fill: Single<&mut Node, With<Fill>>,
) {
    if !g.playing && !g.is_changed() && !fill.is_added() {
        return;
    }

    let hz = g.player.header.tick_hz;
    if g.playing {
        g.carry += time.delta_secs_f64() * f64::from(hz) * g.speed.multiplier();
        let count = (g.carry as u32).min(512);
        for _ in 0..count {
            let p = &mut g.player;
            if p.ended() {
                break;
            }
            if let Err(e) = p.advance() {
                g.notice = Some(e);
                g.playing = false;
                break;
            }

            g.carry -= 1.;
        }

        let p = &g.player;
        if p.ended() {
            let duration = end_tail(p);
            g.tail = (g.tail + (g.carry / f64::from(hz)) as f32).min(duration);
            g.carry = 0.;
            g.playing = g.tail < duration;
        }
    }
    let p = &g.player;
    let w = p.world();
    let sec = |ticks: u64| match_text::duration(ticks / u64::from(hz));
    let event = match_text::event(w.cue());
    let detail = match w.outcome() {
        Some(o) if !event.is_empty() => format!("{}  ·  {event}", match_text::outcome(o)),
        Some(o) => match_text::outcome(o),
        None => event,
    };
    let message = g.notice.as_ref().map_or(detail, locale::replay_error);
    if let Ok(mut v) = visibility.get_mut(cue.1.parent()) {
        v.set_if_neq(if message.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        });
    }
    if let Ok(mut t) = texts.get_mut(cue.0)
        && t.0 != message
    {
        t.0 = message;
    }
    let readout = format!("{} / {}", sec(w.tick()), sec(p.length()));
    if let Ok(mut t) = texts.get_mut(*hud)
        && t.0 != readout
    {
        t.0 = readout;
    }
    let width = percent(if p.length() == 0 {
        100.
    } else {
        100. * w.tick() as f32 / p.length() as f32
    });
    if fill.width != width {
        fill.width = width;
    }
}

#[cfg(test)]
mod tests;
