//! 锁定同一单调时钟上的边界；macOS 可能把普通休眠的唤醒推迟到下一帧。

use crate::sim::HZ;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct TickPacer {
    last_deadline: Option<Instant>,
}

impl TickPacer {
    pub(super) fn deadline(&mut self, tick: u64, published: Instant) -> Instant {
        // 相邻边界之差保留 60 Hz 的纳秒余数，不把每次正常唤醒的小偏差累积起来。
        let at = |t: u64| {
            let hz = u64::from(HZ);
            Duration::new(t / hz, ((t % hz) * 1_000_000_000 / hz) as u32)
        };
        let step = at(tick + 1) - at(tick);
        let planned = self.last_deadline.unwrap_or(published) + step;
        // 若旧时间表留给本次观察的时间已不足半步，从实际发布时刻重新给一整步。
        // 丢弃的是墙钟欠下的时间，不是模拟步或已封存的输入。
        let deadline = if planned.saturating_duration_since(published) < step / 2 {
            published + step
        } else {
            planned
        };
        self.last_deadline = Some(deadline);
        deadline
    }
}

/// 只提升发布快照、封存输入的裁判线程；在所有虚拟机线程创建后调用，不让脚本继承此等级。
/// 系统仍然可以晚调度；提示未获接受时，仍用同一套卡顿恢复规则。
pub(super) fn configure_clock_thread() {
    #[cfg(target_os = "macos")]
    {
        // 安全性：此调用只设置当前线程的服务等级。参数来自系统枚举，0 是该等级的默认相对优先级。
        let _ = unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
        };
    }
}

#[cfg(target_os = "macos")]
pub(super) fn wait_until(deadline: Instant) {
    use mach2::mach_time::{mach_absolute_time, mach_timebase_info, mach_wait_until};
    use std::sync::OnceLock;

    static BASE: OnceLock<Option<mach_timebase_info>> = OnceLock::new();

    let base = BASE.get_or_init(|| {
        let mut b = mach_timebase_info::default();
        // 安全性：系统仅通过有效的可变引用将时间单位写入局部结构体。
        let ok = unsafe { mach_timebase_info(&mut b) } == 0;
        (ok && b.numer > 0 && b.denom > 0).then_some(b)
    });

    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if left.is_zero() {
            break;
        }
        let Some(b) = base else {
            std::thread::sleep(left);
            break;
        };

        let units = (left.as_nanos() * u128::from(b.denom)).div_ceil(u128::from(b.numer)) as u64;

        // 安全性：两个调用均不接收指针；等待目标是同一 Mach 时基上的绝对时刻。
        if unsafe { mach_wait_until(mach_absolute_time().saturating_add(units)) } != 0 {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            break;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn wait_until(deadline: Instant) {
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_jitter_does_not_slow_the_simulation_clock() {
        let start = Instant::now();
        let mut clock = TickPacer::default();
        let mut published = start;
        for tick in 0..u64::from(HZ) {
            let deadline = clock.deadline(tick, published);
            assert!(deadline > published);
            published = deadline + Duration::from_millis(1);
        }
        assert_eq!(clock.last_deadline, Some(start + Duration::from_secs(1)));
    }

    #[test]
    fn stalled_host_resumes_with_a_fresh_window_instead_of_catching_up() {
        for lag in [Duration::from_millis(15), Duration::from_secs(60)] {
            let mut clock = TickPacer::default();
            let first = clock.deadline(0, Instant::now());
            let resumed = first + lag;
            let next = clock.deadline(1, resumed);
            let mut on_time = TickPacer::default();
            let reference = on_time.deadline(0, first);
            let second_step = on_time.deadline(1, reference) - reference;
            assert_eq!(next, resumed + second_step);
            assert!(clock.deadline(2, next) > next);
        }
    }
}
