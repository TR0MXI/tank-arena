//! 两种时间来源在此适配；绘制系统不读线程、播放器或磁盘。

use super::*;
use bevy::ecs::system::SystemParam;
use tank_arena::sim::Event;

#[derive(SystemParam)]
pub(super) struct Source<'w> {
    page: Res<'w, State<Page>>,
    field: Res<'w, Field>,
    session: Res<'w, battle::LiveSession>,
    live: Res<'w, battle::LiveFrame>,
    replay: Option<Res<'w, player::ReplaySession>>,
}

pub(super) struct Scene<'a> {
    pub map: &'a ArenaMap,
    pub tanks: &'a [Tank],
    pub items: &'a tank_arena::sim::Items,
    pub shells: &'a [Shell],
    pub cue: Option<Event>,
    pub tick: u64,
    pub hits: &'a [Option<u64>],
    pub hz: u32,
    pub extra: f32,
    pub roster: bool,
    pub reset: bool,
}

impl Source<'_> {
    pub(super) fn scene(&self) -> Option<Scene<'_>> {
        match self.page.get() {
            Page::Live => {
                let f = self.live.get()?;
                Some(Scene {
                    map: &self.field.map,
                    tanks: &f.tanks,
                    shells: &f.shells,
                    items: &f.items,
                    cue: f.cue,
                    tick: f.tick,
                    hits: &f.hit_ticks,
                    hz: HZ,
                    extra: self.session.tail,
                    roster: self.session.show_roster,
                    reset: self.page.is_changed() || self.live.is_changed(),
                })
            }
            Page::Replay => {
                let g = self.replay.as_ref()?;
                let p = &g.player;
                let w = p.world();
                let hz = p.header.tick_hz;
                Some(Scene {
                    map: w.map(),
                    tanks: w.tanks(),
                    shells: w.shells(),
                    items: w.items(),
                    cue: w.cue(),
                    tick: w.tick(),
                    hits: w.hit_ticks(),
                    hz,
                    extra: if p.ended() {
                        g.tail
                    } else {
                        g.carry.clamp(0., 1.) as f32 / hz as f32
                    },
                    roster: g.show_roster,
                    reset: self.page.is_changed() || g.is_added(),
                })
            }
            _ => None,
        }
    }
}
