//! 参赛身份和控制器分派；策略实现不参与信箱、线程或截止线管理。
use super::Frame;
use crate::{
    bot::{
        BotConfig, Fault, FaultKind, LuauBot, Script,
        avalon::{self, Avalon, Ball, Car, View},
    },
    sim::{ArenaMap, ControlUpdate, Seat},
};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

pub const AVALON_NAME: &str = "Avalon";

#[derive(Clone, Debug)]
pub enum Pilot {
    Avalon,
    Luau(Script),
}

#[derive(Clone, Debug)]
pub struct Entrant {
    pub seat: Seat,
    pub name: String,
    pub pilot: Result<Pilot, Fault>,
}

impl Entrant {
    pub fn avalon(seat: Seat) -> Self {
        Self {
            seat,
            name: AVALON_NAME.into(),
            pilot: Ok(Pilot::Avalon),
        }
    }

    pub fn script(&self) -> Option<&Script> {
        match self.pilot.as_ref().ok()? {
            Pilot::Luau(s) => Some(s),
            Pilot::Avalon => None,
        }
    }

    pub fn fingerprint(&self) -> Option<blake3::Hash> {
        Some(match self.pilot.as_ref().ok()? {
            Pilot::Avalon => avalon::fingerprint(),
            Pilot::Luau(s) => s.blake3(),
        })
    }

    /// 同名的车库脚本与内置策略在名单上仍可区分。
    pub fn luau_label(name: &str) -> &str {
        if name == AVALON_NAME {
            "Avalon (Luau)"
        } else {
            name
        }
    }

    pub fn from_script(seat: Seat, script: Script) -> Self {
        Self {
            seat,
            name: Self::luau_label(script.name()).into(),
            pilot: Ok(Pilot::Luau(script)),
        }
    }

    pub fn from_file(seat: Seat, path: &Path) -> Self {
        let script = Script::read(path).map_err(|e| Fault {
            kind: FaultKind::Load,
            message: format!("{}: {e}", path.display()),
        });
        match script {
            Ok(script) => Self::from_script(seat, script),
            Err(fault) => Self {
                seat,
                name: Self::luau_label(&crate::bot::catalog::tank_name(path)).into(),
                pilot: Err(fault),
            },
        }
    }

    pub(super) fn load<'a>(
        &self,
        config: BotConfig<'a>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Controller<'a>, Fault> {
        match &self.pilot {
            Ok(Pilot::Avalon) => Ok(Controller::Avalon {
                map: config.map,
                policy: Box::default(),
                cars: Vec::new(),
                balls: Vec::new(),
            }),
            Ok(Pilot::Luau(script)) => LuauBot::load(script, config, cancel)
                .map(|bot| Controller::Luau(Box::new(bot)))
                .map_err(|e| Fault::classify(e, true)),
            Err(fault) => Err(fault.clone()),
        }
    }
}

pub(super) enum Controller<'a> {
    Luau(Box<LuauBot>),
    Avalon {
        map: &'a ArenaMap,
        policy: Box<Avalon>,
        cars: Vec<Car>,
        balls: Vec<Ball>,
    },
}

impl Controller<'_> {
    pub(super) fn control(
        &mut self,
        i: usize,
        frame: &Frame,
        measure: impl FnOnce(Instant, Duration),
    ) -> Result<ControlUpdate, Fault> {
        match self {
            Self::Luau(bot) => bot
                .control(
                    frame.tick,
                    frame.sight[i],
                    &frame.tanks,
                    &frame.shells,
                    &frame.items,
                    measure,
                )
                .map_err(|e| Fault::classify(e, false)),
            Self::Avalon {
                map,
                policy,
                cars,
                balls,
            } => {
                let observe = |t| Car::observe(t, &frame.items, frame.tick);
                cars.clear();
                cars.extend(frame.tanks.iter().filter(|t| t.alive).map(observe));
                balls.clear();
                balls.extend(frame.shells.iter().map(|s| Ball::observe(s, &frame.items)));
                let view = View {
                    map,
                    tick: frame.tick,
                    me: observe(&frame.tanks[i]),
                    sight: frame.sight[i],
                    weapon_slots_used: frame.items.gear(frame.tanks[i].seat.slot).queue.len(),
                    cars,
                    balls,
                    crates: &frame.items.crates,
                };
                let entered = Instant::now();
                let (controls, _) = policy.decide(&view);
                measure(entered, entered.elapsed());
                Ok(controls)
            }
        }
    }
}
