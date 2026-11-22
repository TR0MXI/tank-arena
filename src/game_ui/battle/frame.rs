use super::*;
use std::sync::Arc;
use tank_arena::runner::Frame;

/// 同一次 Bevy 更新中的状态栏、特效、赛果和战场共用这份不可变快照。
#[derive(Resource, Default)]
pub(in crate::game_ui) struct LiveFrame(Option<Arc<Frame>>);

impl LiveFrame {
    pub(in crate::game_ui) fn get(&self) -> Option<&Frame> {
        self.0.as_deref()
    }
}

pub(super) fn capture(time: Res<Time>, mut g: ResMut<LiveSession>, mut frame: ResMut<LiveFrame>) {
    let Some(l) = &g.live else { return };

    // 先看线程状态再取快照；不能拿尚未终局的旧帧当作已退出线程的赛果。
    let finished = l.finished();
    let f = l.latest();
    if !frame.0.as_ref().is_some_and(|old| Arc::ptr_eq(old, &f)) {
        frame.0 = Some(f.clone());
    }
    if finished || f.outcome.is_some() {
        g.tail += time.delta_secs();
    }
    if finished && g.tail >= arena_view::tail_duration(f.tick, &f.hit_ticks, HZ) {
        let ok = g.live.take().unwrap().join().is_ok();
        g.confirm_stop = None;
        g.finished = Some(Summary::new(f, ok));
    }
}

pub(super) fn leave(mut game: ResMut<LiveSession>, mut frame: ResMut<LiveFrame>) {
    frame.0 = None;
    *game = LiveSession::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tank_arena::{
        bot::Script,
        runner::Entrant,
        sim::{Seat, SlotId, TeamId},
    };

    // 初始化就弃权：两场的最终步数都是 0，不能按步数 判定是否为旧帧。
    fn ended_match() -> LiveMatch {
        let script = Script::from_source("invalid", "return false").unwrap();
        let entrants = (1..=2)
            .map(|n| {
                Entrant::from_script(
                    Seat {
                        slot: SlotId(n),
                        team: TeamId(n),
                    },
                    script.clone(),
                )
            })
            .collect();
        let live = LiveMatch::start(
            Arc::new(SWITCHYARD.parse().unwrap()),
            ClockRules::unlimited(),
            entrants,
            (),
        )
        .unwrap();
        let end = Instant::now() + Duration::from_secs(4);
        while !live.finished() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }

        live
    }

    #[test]
    fn terminal_frame_is_shared_with_results_and_replaced_on_in_page_rematch() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<LiveSession>()
            .init_resource::<LiveFrame>()
            .add_systems(Update, capture)
            .add_systems(OnExit(Page::Live), leave);
        for _ in 0..2 {
            let live = ended_match();
            let last = live.latest();
            assert_eq!(last.tick, 0);
            assert!(last.outcome.is_some());
            let mut game = app.world_mut().resource_mut::<LiveSession>();
            game.live = Some(live);
            game.finished = None;
            app.update();
            let w = app.world();
            assert!(Arc::ptr_eq(
                &last,
                &w.resource::<LiveSession>().finished.as_ref().unwrap().frame
            ));
            assert!(std::ptr::eq(
                last.as_ref(),
                w.resource::<LiveFrame>().get().unwrap()
            ));
            assert!(w.resource::<LiveSession>().live.is_none());
            app.update();

            assert!(std::ptr::eq(
                last.as_ref(),
                app.world().resource::<LiveFrame>().get().unwrap()
            ));
        }

        app.world_mut().run_schedule(OnExit(Page::Live));

        assert!(app.world().resource::<LiveFrame>().get().is_none());
        assert!(app.world().resource::<LiveSession>().finished.is_none());
    }
}
