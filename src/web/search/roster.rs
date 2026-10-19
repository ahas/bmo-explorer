//! Every search engine the web layer asks, as one roster, and how long an engine rests after a refusal
//! (docs/web-access.md, 「Rosters」 and 「Resting an engine」), as `roster.toml` beside this module holds them.

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
  pub name: String,
  pub category: String,
  pub korean_native: bool,
  pub english_web: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rests {
  pub failure_ms: i64,
  pub network_ms: i64,
  pub refusal_ms: i64,
}

/// One line of the rest lengths as a reader is shown them, naming the rest it takes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cooldown {
  pub reason: String,
  pub rest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roster {
  pub engine: Vec<Engine>,
  pub rests: Rests,
  pub cooldown: Vec<Cooldown>,
}

impl Rests {
  /// The length of a rest by the name a cooldown line gives it.
  pub fn named(&self, rest: &str) -> Option<i64> {
    match rest {
      "failure" => Some(self.failure_ms),
      "network" => Some(self.network_ms),
      "refusal" => Some(self.refusal_ms),
      _ => None,
    }
  }
}

/// The roster as it is built into this repository.
pub static ROSTER: LazyLock<Roster> =
  LazyLock::new(|| toml::from_str(include_str!("roster.toml")).expect("roster.toml reads as the roster"));
