//! The time a web read is stamped with: epoch milliseconds, as the session record and the store hold them.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now on the machine's own clock, in epoch milliseconds.
pub fn now_ms() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}
