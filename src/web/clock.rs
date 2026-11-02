//! The time a web read keeps: times are epoch milliseconds, as the store's pacing holds them, and a test
//! hands in a clock of its own so a wait costs it no real time.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

// Now on the machine's own clock stands in the web's forms crate, which a site tool reads it from too.
pub use crate::forms::clock::now_ms;

/// What a wait is.
pub type Sleeping = Pin<Box<dyn Future<Output = ()> + Send>>;

pub trait Clock: Send + Sync {
  /// Now, in epoch milliseconds.
  fn now(&self) -> u64;
  /// A wait of this many milliseconds.
  fn sleep(&self, ms: u64) -> Sleeping;
}

/// The machine's own clock.
pub struct SystemClock;

impl Clock for SystemClock {
  fn now(&self) -> u64 {
    now_ms()
  }

  fn sleep(&self, ms: u64) -> Sleeping {
    Box::pin(tokio::time::sleep(Duration::from_millis(ms)))
  }
}

/// The clock every read takes where its caller hands none.
pub fn system() -> Arc<dyn Clock> {
  Arc::new(SystemClock)
}

/// A clock a test moves: a wait moves it on by the wait's length and returns at once.
#[derive(Default)]
pub struct StandingClock {
  at: AtomicU64,
}

impl StandingClock {
  pub fn at(ms: u64) -> Arc<Self> {
    Arc::new(Self {
      at: AtomicU64::new(ms),
    })
  }

  pub fn advance(&self, ms: u64) {
    self.at.fetch_add(ms, Ordering::SeqCst);
  }
}

impl Clock for StandingClock {
  fn now(&self) -> u64 {
    self.at.load(Ordering::SeqCst)
  }

  fn sleep(&self, ms: u64) -> Sleeping {
    self.advance(ms);
    Box::pin(std::future::ready(()))
  }
}
