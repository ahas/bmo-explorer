//! bmo-receiver (docs/api.md, 「bmo-receiver」): the queue a run's ending is handed back to its room's session
//! through, as a quiet instruction.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::{Unit, json};

/// A run's ending handed to its room's session: the agent whose turn asked, the room, the words the session is
/// handed, the kind of ending it is, the road the question came in on and the run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandBack {
  pub agent: Option<String>,
  pub chat: i64,
  pub said: Said,
  pub sort: String,
  pub via: Option<String>,
  pub run: Option<i64>,
}

/// What an ending hands the session: the words of an order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Said {
  pub order: String,
}

#[derive(Deserialize)]
struct Queued {
  id: i64,
}

/// bmo-receiver at the address the settings name, under the token they name.
#[derive(Clone, Debug)]
pub struct Receiver {
  unit: Unit,
}

impl Receiver {
  pub fn new(address: Option<String>, token: Option<String>) -> Self {
    Self {
      unit: Unit::new("bmo-receiver", "receiver", address, token),
    }
  }

  /// A run's ending queued for its room's session: the instruction's number.
  pub async fn hand_back(&self, back: &HandBack) -> Result<i64, String> {
    let queued: Queued = self.unit.ask(Method::POST, "/instructions", &[], json(back)).await?;

    Ok(queued.id)
  }
}
