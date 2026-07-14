//! Потоки VOID: их нет — ядро планирует ПРОЦЕССЫ (spawn отдаёт unsupported,
//! см. thread/mod.rs). Но sleep/yield честные: SYS_YIELD уступает квант,
//! дедлайн меряется монотонным временем U-mode.

use crate::time::{Duration, Instant};

pub fn yield_now() {
    crate::sys::pal::abi::sys_yield();
}

pub fn sleep(dur: Duration) {
    let deadline = Instant::now().checked_add(dur);
    match deadline {
        Some(deadline) => sleep_until(deadline),
        None => loop {
            yield_now();
        },
    }
}

pub fn sleep_until(deadline: Instant) {
    while Instant::now() < deadline {
        yield_now();
    }
}
