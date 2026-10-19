//! bmo-gate (docs/api.md, 「bmo-gate」): the knowledge a piece of work is handed, picked by the gate the way it
//! picks a turn's reference.

use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use super::Unit;

/// One piece of knowledge: where it is kept, and what it says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Piece {
  pub path: String,
  pub text: String,
}

#[derive(Deserialize)]
struct Picked {
  #[serde(default)]
  pieces: Vec<Piece>,
}

/// bmo-gate at the address the settings name, under the token they name.
#[derive(Clone, Debug)]
pub struct Gate {
  unit: Unit,
}

impl Gate {
  pub fn new(address: Option<String>, token: Option<String>) -> Self {
    Self {
      unit: Unit::new("bmo-gate", "gate", address, token),
    }
  }

  /// The pieces of knowledge a piece of work in a room is handed for its own lines.
  pub async fn work_knowledge(&self, chat_id: i64, lines: &[String]) -> Result<Vec<Piece>, String> {
    let body = json!({ "chat_id": chat_id, "lines": lines });
    let picked: Picked = self.unit.ask(Method::POST, "/knowledge/work", &[], Some(body)).await?;

    Ok(picked.pieces)
  }
}
