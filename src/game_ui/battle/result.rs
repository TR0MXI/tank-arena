use super::*;
use crate::game_ui::match_text::{duration, outcome};
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use std::sync::Arc;
use tank_arena::{bot::FaultKind, replay::RecordStatus};
use tank_arena::{
    replay::{SaveFailure, library},
    runner::Frame,
};

type SaveReport = Result<(PathBuf, Option<SaveFailure>), SaveFailure>;

pub(in crate::game_ui) struct Summary {
    pub(super) frame: Arc<Frame>,
    title: String,
    saved: Option<PathBuf>,
    error: Option<String>,
    writing: Option<Task<SaveReport>>,
    dirty: bool,
}

impl Summary {
    pub(super) fn new(frame: Arc<Frame>, ok: bool) -> Self {
        let title = frame
            .outcome
            .filter(|_| ok)
            .map_or_else(|| tr("workerLost"), outcome);
        Self {
            frame,
            title,
            saved: None,
            error: None,
            writing: None,
            dirty: true,
        }
    }
}

#[derive(Component)]
pub(super) struct ResultLayer;

#[derive(Component)]
pub(super) struct Footer;

pub(super) fn overlay(
    mut c: Commands,
    g: Res<LiveSession>,
    window: Single<&Window>,
    old: Query<Entity, With<ResultLayer>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<menu::FrostedMenu>>,
) {
    let Some(summary) = &g.finished else {
        for e in &old {
            c.entity(e).despawn();
        }
        return;
    };
    if !old.is_empty() {
        return;
    }

    let scope = || (ResultLayer, DespawnOnExit(Page::Live));
    let (_, backdrop) = menu::frost(&mut c, &window, &mut images, &mut materials, scope());
    c.spawn((scope(), menu::modal_layer())).with_children(|p| {
        p.spawn(backdrop);
        p.spawn(menu::modal_card(674.)).with_children(|p| {
            let f = &summary.frame;
            p.spawn((
                spread(),
                children![
                    eyebrow(&tr("result")),
                    label(duration(f.tick / u64::from(HZ)), 15., quiet())
                ],
            ));
            p.spawn(headline(&summary.title, 36.));
            p.spawn(Node {
                width: percent(100),
                padding: UiRect::axes(px(14), px(10)),
                border_radius: BorderRadius::all(px(12)),
                ..column(5.)
            })
            .insert(BackgroundColor(Color::srgb_u8(229, 235, 222)))
            .with_children(|p| {
                let cell = |value: String, w: f32, header: bool| {
                    (
                        Node {
                            width: px(w),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        children![(
                            label(
                                value,
                                if header { 13. } else { 15. },
                                if header { quiet() } else { ink() }
                            ),
                            TextLayout::no_wrap()
                        )],
                    )
                };
                let cols = [
                    (228., "roster"),
                    (72., "team"),
                    (90., "hits"),
                    (96., "friendly"),
                    (56., "selfHit"),
                ];
                p.spawn(row()).with_children(|p| {
                    for (w, key) in cols {
                        p.spawn(cell(tr(key), w, true));
                    }
                });
                for (i, (t, s)) in f.tanks.iter().zip(&f.stats).enumerate() {
                    let mut name = format!("#{}   {}", t.seat.slot.0, g.names[i]);
                    if let Some(e) = &s.fault {
                        name = format!(
                            "{} · {name}",
                            tr(match e.kind {
                                FaultKind::Load => "loadFault",
                                FaultKind::Runtime => "runtimeFault",
                                FaultKind::Action => "actionFault",
                                FaultKind::Memory => "memoryFault",
                            })
                        );
                    }
                    let values = [
                        name,
                        t.seat.team.0.to_string(),
                        s.enemy_hits.to_string(),
                        s.friendly_hits.to_string(),
                        s.self_hits.to_string(),
                    ];
                    p.spawn(row()).with_children(|p| {
                        for ((w, _), v) in cols.into_iter().zip(values) {
                            p.spawn(cell(v, w, false));
                        }
                    });
                }
            });
            p.spawn((
                Footer,
                Node {
                    width: percent(100),
                    ..column(10.)
                },
            ));
        });
    });
}

fn save(_: On<Activate>, mut g: ResMut<LiveSession>) {
    let (Some(record), Some(stage)) = (g.record.clone(), g.stage.clone()) else {
        return;
    };
    let Some(summary) = g.finished.as_mut() else {
        return;
    };
    if summary.saved.is_some() || summary.writing.is_some() {
        return;
    }
    let RecordStatus::Saved(src) = record.status() else {
        return;
    };

    summary.error = None;
    summary.dirty = true;
    summary.writing = Some(IoTaskPool::get().spawn(async move {
        let _keep_stage_alive = stage;
        let dir = Folder::Replays.path()?;
        let saved = library::save_staged(&src, &dir)?;
        let stats_error = match record.result_status() {
            RecordStatus::Saved(path) => library::save_staged(&path, &dir).err().map(Into::into),
            RecordStatus::Incomplete(e) => Some(e),
            _ => None,
        };
        Ok((saved, stats_error))
    }));
}

pub(super) fn actions(
    mut c: Commands,
    mut g: ResMut<LiveSession>,
    footer: Single<Entity, With<Footer>>,
) {
    if !g
        .finished
        .as_ref()
        .is_some_and(|s| s.dirty || s.writing.is_some())
    {
        return;
    }

    let LiveSession {
        record, finished, ..
    } = &mut *g;
    let Some(s) = finished.as_mut() else { return };

    if let Some(task) = &mut s.writing
        && let Some(done) = check_ready(task)
    {
        s.writing = None;
        s.dirty = true;
        match done {
            Ok((path, err)) => {
                s.saved = Some(path);
                s.error =
                    err.map(|e| format!("{} · {}", tr("resultNotSaved"), locale::save_error(&e)));
            }
            Err(e) => s.error = Some(locale::save_error(&e)),
        }
    }
    if !std::mem::take(&mut s.dirty) {
        return;
    }

    let status = record.as_ref().map(Recording::status);
    c.entity(*footer).despawn_children().with_children(|p| {
        if let Some(err) = &s.error {
            p.spawn(label(err, 15., quiet()));
        }
        if let Some(RecordStatus::Incomplete(e)) = &status {
            p.spawn(label(
                format!("{} · {}", tr("incomplete"), locale::save_error(e)),
                15.,
                quiet(),
            ));
        }
        p.spawn(spread()).with_children(|p| {
            p.spawn(row()).with_children(|p| {
                if let Some(path) = &s.saved {
                    p.spawn((compact(&tr("watchMatch")), player::open(path.clone())));
                    p.spawn(label(tr("recorded"), 14., quiet()));
                } else if s.writing.is_some() {
                    p.spawn(disabled(&tr("saving")));
                } else if matches!(status, Some(RecordStatus::Saved(_))) {
                    p.spawn((button(&tr("saveReplay")), observe(save)));
                }
            });
            p.spawn(row()).with_children(|p| {
                if s.writing.is_some() {
                    return;
                }
                p.spawn((compact(&tr("home")), navigate(Page::Menu)));
                p.spawn((compact(&tr("editLineup")), navigate(Page::Setup)));
                p.spawn((primary(&tr("again")), setup::launch()));
            });
        });
    });
}
