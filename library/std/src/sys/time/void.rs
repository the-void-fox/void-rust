//! Монотонное время VOID: rdtime/rdtsc из U-mode (ядро открывает счётчики
//! процессам), в наносекундах через TICK_NS.
//!
//! SystemTime (Веха 32 — ls из uutils зовёт `SystemTime::now`) — те же
//! счётчики: RTC в VOID не читается, «настенные часы» = ФИКТИВНАЯ база
//! (2026-07-01T00:00:00Z) + время с загрузки. Дата неверна, но арифметика
//! жива: софт вроде ls делает `now − 6 месяцев`, и с базой около нуля это
//! паника на underflow. Порядок и разности в пределах сессии корректны;
//! mtime у файлов персоналия всё равно не хранит (modified() — unsupported).

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

/// Фиктивная база часов: 2026-07-01T00:00:00Z в секундах Unix.
const WALL_BASE_SECS: u64 = 1_782_864_000;

impl SystemTime {
    pub const MAX: SystemTime = SystemTime(Duration::MAX);
    pub const MIN: SystemTime = SystemTime(Duration::ZERO);

    pub fn now() -> SystemTime {
        SystemTime(Duration::from_secs(WALL_BASE_SECS) + Duration::from_nanos(abi::now_ns()))
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
