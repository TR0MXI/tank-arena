//! 锁定同一单调时钟上的边界；macOS 可能把普通休眠的唤醒推迟到下一帧。

use std::time::Instant;

/// 只提升发布快照、封存输入的裁判线程；在所有虚拟机线程创建后调用，不让脚本继承此等级。
/// 系统仍然可以晚调度；提示未获接受时沿用普通调度和完全相同的过载判定。
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
