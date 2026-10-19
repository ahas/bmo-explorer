//! bmo-artifact (docs/api.md, 「bmo-artifact」): the research settings, of which a browse reads its own key's
//! profile.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use super::Unit;

#[derive(Deserialize)]
struct Settings {
  #[serde(default)]
  profiles: Vec<Value>,
}

/// bmo-artifact at the address the settings name, under the token they name.
#[derive(Clone, Debug)]
pub struct Artifact {
  unit: Unit,
}

impl Artifact {
  pub fn new(address: Option<String>, token: Option<String>) -> Self {
    Self {
      unit: Unit::new("bmo-artifact", "artifact", address, token),
    }
  }

  /// One research key's profile as the settings answer it, refused where they hold none for the key.
  pub async fn profile(&self, key: &str) -> Result<Value, String> {
    let settings: Settings = self.unit.ask(Method::GET, "/research-settings", &[], None).await?;

    settings
      .profiles
      .into_iter()
      .find(|profile| profile.get("key").and_then(Value::as_str) == Some(key))
      .ok_or_else(|| format!("bmo-artifact's research settings hold no {key} profile"))
  }
}
