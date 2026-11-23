use super::{
    Entrant, Frame,
    mailbox::{Mailbox, Ticket},
};
use crate::{
    bot::{BotConfig, Fault, LuauBot},
    sim::ControlUpdate,
};
use std::{
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::Instant,
};

pub(super) struct Answer {
    pub value: Result<ControlUpdate, Fault>,
    pub cost_us: Option<u64>,
    pub wake_us: u64,
}

pub(super) struct State {
    pub mailbox: Mailbox<Answer>,
    pub latest: Option<(Arc<Frame>, Instant)>,
}

pub(super) struct Exchange {
    pub state: Mutex<State>,
    pub changed: Condvar,
    pub cancel: Arc<AtomicBool>,
}

/// 即使裁判或记录回调意外展开栈，也先关闭信箱，避免离开作用域时一直等待休眠中的工作线程。
pub(super) struct CloseOnExit<'a>(pub &'a Exchange);

impl Drop for CloseOnExit<'_> {
    fn drop(&mut self) {
        self.0.cancel.store(true, Ordering::Release);
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.mailbox.open = false;
        state.latest = None;
        drop(state);
        self.0.changed.notify_all();
    }
}

impl Exchange {
    pub fn new(id: u64, cancel: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(State {
                mailbox: Mailbox::new(id),
                latest: None,
            }),
            changed: Condvar::new(),
            cancel,
        }
    }

    fn request(&self, i: usize) -> Option<(Ticket, Arc<Frame>, Instant)> {
        let mut state = self.state.lock().unwrap();
        loop {
            let State { mailbox, latest } = &mut *state;
            if !mailbox.open {
                return None;
            }

            if let Some((frame, at)) = latest.as_ref() {
                let t = frame.tanks[i];
                if !t.alive {
                    return None;
                }
                if let Some(req) = mailbox.request(i, t.seat.slot, frame.tick) {
                    return Some((req, Arc::clone(frame), *at));
                }
            }
            state = self.changed.wait(state).unwrap();
        }
    }
}

pub(super) fn work(
    i: usize,
    entrant: &Entrant,
    config: BotConfig<'_>,
    ex: &Exchange,
    ready: Sender<(usize, Result<(), Fault>)>,
) {
    let loaded = entrant
        .script
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|script| {
            LuauBot::load(script, config, ex.cancel.clone()).map_err(|e| Fault::classify(e, true))
        });
    let mut bot = match loaded {
        Ok(bot) => {
            if ready.send((i, Ok(()))).is_err() {
                return;
            }
            bot
        }
        Err(e) => {
            let _ = ready.send((i, Err(e)));
            return;
        }
    };
    drop(ready);
    while let Some((ticket, frame, published)) = ex.request(i) {
        let mut cost_us = None;
        let mut wake_us = 0;
        let value = bot
            .control(
                frame.tick,
                frame.sight[i],
                &frame.tanks,
                &frame.shells,
                &frame.items,
                |entered, elapsed| {
                    wake_us = entered.duration_since(published).as_micros() as u64;
                    cost_us = Some(elapsed.as_micros() as u64);
                },
            )
            .map_err(|e| Fault::classify(e, false));
        let failed = value.is_err();
        let mut s = ex.state.lock().unwrap();
        s.mailbox.submit(
            i,
            ticket,
            Instant::now(),
            Answer {
                value,
                cost_us,
                wake_us,
            },
        );
        if failed {
            break;
        }
    }
}
