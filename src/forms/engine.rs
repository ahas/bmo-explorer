//! What a search engine was last seen to do, as a search's answer carries it: the state, the reason it
//! was seen in, when it was observed and how long it rests. A search's answer is one of the web's forms,
//! so this stands beside them rather than in the pacing that keeps it.

/// What an engine was last seen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
  Healthy,
  Degraded,
  Suspended,
  Unknown,
}

impl EngineState {
  pub fn name(self) -> &'static str {
    match self {
      Self::Healthy => "healthy",
      Self::Degraded => "degraded",
      Self::Suspended => "suspended",
      Self::Unknown => "unknown",
    }
  }

  pub fn named(name: &str) -> Option<Self> {
    [Self::Healthy, Self::Degraded, Self::Suspended, Self::Unknown]
      .into_iter()
      .find(|one| one.name() == name)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EngineHealth {
  pub engine: String,
  pub state: EngineState,
  pub reason: Option<String>,
  pub observed_at: Option<u64>,
  pub suspended_until: Option<u64>,
  pub elapsed_ms: Option<u64>,
  pub result_count: Option<u64>,
}

impl EngineHealth {
  /// An engine nothing is known of yet.
  pub fn unknown(engine: &str) -> Self {
    Self {
      engine: engine.to_owned(),
      state: EngineState::Unknown,
      reason: None,
      observed_at: None,
      suspended_until: None,
      elapsed_ms: None,
      result_count: None,
    }
  }
}
