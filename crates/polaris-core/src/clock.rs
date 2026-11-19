//! Monotonic time as an injectable side effect, so time-based caches are
//! tested deterministically.

use std::time::Instant;

/// Provides the current monotonic instant.
pub trait Clock: Send + Sync + 'static {
    /// Returns the current instant.
    fn now(&self) -> Instant;
}

/// The real monotonic clock of the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
