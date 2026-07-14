//! Монотонное время VOID: rdtime/rdtsc из U-mode (ядро открывает счётчики
//! процессам), в наносекундах через TICK_NS. SystemTime (настенные часы)
//! в VOID пока нет — RTC не читается; см. unsupported в time/mod.rs.

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
