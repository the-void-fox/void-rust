//! Futex VOID (Веха 35) — примитив блокировки поверх `SYS_FUTEX` ядра. На нём стоят
//! Mutex/Condvar/RwLock/Parker std (futex-реализации, как у Linux/hermit). Ключ ожидания
//! в ядре — (адресное пространство, адрес слова), поэтому futex-слова процесса разделяются
//! его нитями.

use super::abi;
use crate::sync::atomic::Atomic;
use crate::time::Duration;

/// Атомик под futex шириной ≥ 32 бит.
pub type Futex = Atomic<Primitive>;
/// Базовый тип [`Futex`].
pub type Primitive = u32;

/// Атомик под futex шириной ≥ 8 бит (у нас та же ширина — ядро работает 32-битным словом).
pub type SmallFutex = Atomic<SmallPrimitive>;
/// Базовый тип [`SmallFutex`].
pub type SmallPrimitive = u32;

/// Уснуть, пока `*futex == expected`. `timeout` — относительный; `None` — бессрочно.
/// Возврат `false` только если истёк таймаут (иначе разбудили/значение сменилось).
pub fn futex_wait(futex: &Atomic<u32>, expected: u32, timeout: Option<Duration>) -> bool {
    // Перевести таймаут в тики ядра (`arch::now_ticks`). Переполнение → бессрочно (0).
    let ticks = match timeout {
        None => 0,
        Some(dur) => {
            let ns = dur.as_nanos();
            let t = (ns / abi::tick_ns() as u128).min(usize::MAX as u128) as usize;
            // 0 тиков означало бы «бессрочно»; крошечный ненулевой таймаут округляем к 1.
            t.max(1)
        }
    };
    // Ядро вернёт 1 при истечении таймаута, 0 при пробуждении/несовпадении значения.
    abi::sys_futex_wait(futex.as_ptr(), expected, ticks) != 1
}

/// Разбудить одну нить, спящую на `futex`. `true` — кого-то разбудили.
pub fn futex_wake(futex: &Atomic<u32>) -> bool {
    abi::sys_futex_wake(futex.as_ptr(), 1) > 0
}

/// Разбудить все нити, спящие на `futex`.
pub fn futex_wake_all(futex: &Atomic<u32>) {
    abi::sys_futex_wake(futex.as_ptr(), usize::MAX);
}
