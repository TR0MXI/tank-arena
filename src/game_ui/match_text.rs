//! 实况与回放共用的比赛读数；不依赖任何页面或会话资源。

use super::{HZ, tr};
use tank_arena::sim::{ClockRules, Event, HitKind, Outcome};

pub(super) fn outcome(o: Outcome) -> String {
    match o {
        Outcome::Winner(t) => format!("{} {} {}", tr("team"), t.0, tr("winner")),
        Outcome::Timeout => tr("timeout"),
        Outcome::MutualElimination => tr("mutual"),
        Outcome::Stopped => tr("stopped"),
        Outcome::WorkerLost => tr("workerLost"),
        Outcome::AllForfeited => tr("allForfeited"),
    }
}

pub(super) fn duration(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn clock(t: u64, c: ClockRules) -> String {
    let played = t / u64::from(HZ);
    match c.seconds() {
        Some(limit) => duration(limit.saturating_sub(played)),
        None => format!("∞   {}", duration(played)),
    }
}

pub(super) fn event(e: Option<Event>) -> String {
    match e {
        Some(Event::Hit {
            owner,
            target,
            kind,
            ..
        }) => format!(
            "#{}   {}   #{}",
            owner.0,
            tr(match kind {
                HitKind::SelfHit => "self",
                HitKind::Enemy => "hit",
                HitKind::Friendly => "friendly",
            }),
            target.0
        ),
        Some(Event::Bounce { shell, .. }) => {
            format!("{} {}  ·  {}", tr("shell"), shell + 1, tr("bounce"))
        }
        Some(Event::Forfeit(s)) => format!("#{}  {}", s.0, tr("forfeited")),
        _ => String::new(),
    }
}
