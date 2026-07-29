//! Монотонное время VOID: rdtime/rdtsc из U-mode (ядро открывает счётчики
//! процессам), в наносекундах через TICK_NS.
//!
//! SystemTime (Веха 32 — ls из uutils зовёт `SystemTime::now`) — **настоящие
//! часы с Вехи 86**: `SYS_TIME` отдаёт время, которое ядро прочитало у
//! платформы на загрузке (CMOS RTC на x86, goldfish-rtc на riscv) и ведёт от
//! монотонного счётчика. Раньше здесь была ФИКТИВНАЯ база (2026-07-01) плюс
//! uptime — дата была заведомо неверной.
//!
//! Если часов на машине нет, ядро честно отдаёт время от эпохи Unix (то есть
//! uptime) — дата будет «1970». Осторожно: софт вроде ls вычитает из `now`
//! месяцы, и у времени около нуля такая арифметика уходит в underflow (ровно
//! ради этого до Вехи 86 здесь стояла выдуманная база). На машинах с RTC —
//! а это все, где мы загружаемся, — вопрос снят. mtime у файлов персоналия
//! пока не хранит (modified() — unsupported).

use crate::sys::pal::abi;
use crate::time::Duration;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Instant {
        Instant(Duration::from_nanos(abi::now_ns()))
    }

    pub fn checked_sub_instant(&self, other: &Instant) -> Option<Duration> {
        self.0.checked_sub(other.0)
    }

    pub fn checked_add_duration(&self, other: &Duration) -> Option<Instant> {
        Some(Instant(self.0.checked_add(*other)?))
    }

    pub fn checked_sub_duration(&self, other: &Duration) -> Option<Instant> {
        Some(Instant(self.0.checked_sub(*other)?))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct SystemTime(Duration);

pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::ZERO);

// Веха 86: фиктивной базы больше нет — настенное время приходит из ядра (`SYS_TIME`).

impl SystemTime {
    pub const MAX: SystemTime = SystemTime(Duration::MAX);
    pub const MIN: SystemTime = SystemTime(Duration::ZERO);

    pub fn now() -> SystemTime {
        SystemTime(Duration::from_nanos(abi::sys_time_ns()))
    }

    pub fn sub_time(&self, other: &SystemTime) -> Result<Duration, Duration> {
        self.0.checked_sub(other.0).ok_or_else(|| other.0 - self.0)
    }

    pub fn checked_add_duration(&self, other: &Duration) -> Option<SystemTime> {
        Some(SystemTime(self.0.checked_add(*other)?))
    }

    pub fn checked_sub_duration(&self, other: &Duration) -> Option<SystemTime> {
        Some(SystemTime(self.0.checked_sub(*other)?))
    }
}
