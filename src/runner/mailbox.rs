//! 状态机只使用调用方传入的截止时刻，测试不需要实际休眠或启动虚拟机。

use crate::sim::{MAX_SLOTS, SlotId};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ticket {
    pub match_id: u64,
    pub slot: SlotId,
    pub seq: u64,
    pub observed: u64,
}

enum Phase<T> {
    Idle,
    Thinking(Ticket),
    Ready(Ticket, Instant, T),
}

struct Lane<T> {
    phase: Phase<T>,
    seq: u64,
    last: Option<u64>,
}

pub(super) struct Mailbox<T> {
    id: u64,
    lanes: [Lane<T>; MAX_SLOTS],
    pub open: bool,
}

impl<T> Mailbox<T> {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            lanes: std::array::from_fn(|_| Lane {
                phase: Phase::Idle,
                seq: 0,
                last: None,
            }),
            open: true,
        }
    }

    pub fn request(&mut self, i: usize, slot: SlotId, tick: u64) -> Option<Ticket> {
        let lane = &mut self.lanes[i];
        if !self.open || !matches!(lane.phase, Phase::Idle) || lane.last == Some(tick) {
            return None;
        }

        lane.seq += 1;
        let t = Ticket {
            match_id: self.id,
            slot,
            seq: lane.seq,
            observed: tick,
        };
        lane.phase = Phase::Thinking(t);
        lane.last = Some(tick);
        Some(t)
    }

    pub fn submit(&mut self, i: usize, t: Ticket, at: Instant, value: T) {
        let lane = &mut self.lanes[i];
        if self.open && matches!(lane.phase, Phase::Thinking(active) if active == t) {
            lane.phase = Phase::Ready(t, at, value);
        }
    }

    pub fn seal(&mut self, deadline: Instant) -> [Option<(Ticket, T)>; MAX_SLOTS] {
        std::array::from_fn(|i| {
            let phase = &mut self.lanes[i].phase;
            if matches!(phase, Phase::Ready(_, at, _) if *at <= deadline)
                && let Phase::Ready(t, _, v) = std::mem::replace(phase, Phase::Idle)
            {
                return Some((t, v));
            }

            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn seven_fast_lanes_do_not_wait_for_the_eighth() {
        let start = Instant::now();
        let ms = |n| start + Duration::from_millis(n);
        let mut m = Mailbox::new(9);
        let slow = m.request(7, SlotId(8), 0).unwrap();
        for k in 0..3 {
            for i in 0..7 {
                let t = m.request(i, SlotId(i as u32 + 1), k).unwrap();
                m.submit(i, t, ms(5 + 17 * k), true);
                assert!(m.request(i, t.slot, k).is_none());
            }

            if k == 1 {
                m.submit(7, slow, ms(40), true);
            }
            let out = m.seal(ms([16, 33, 50][k as usize]));
            assert!(out[..7].iter().all(Option::is_some));
            assert_eq!(out[7].is_some(), k == 2);
            if k == 2 {
                assert_eq!(out[7].unwrap().0.observed, 0);
            }
        }

        m.submit(7, slow, ms(51), true);

        assert!(m.seal(ms(66)).iter().all(Option::is_none));
        let next = m.request(7, SlotId(8), 3).unwrap();
        m.submit(
            7,
            Ticket {
                match_id: 8,
                ..next
            },
            ms(67),
            true,
        );

        assert!(m.seal(ms(80))[7].is_none());
        m.open = false;
        m.submit(7, next, ms(90), true);
        assert!(m.seal(ms(100))[7].is_none());
    }
}
