//! The services this repository calls (docs/api.md, 「What this repository calls」), each reached over the
//! routes its own boundary documents and nothing else.
//!
//! - `agency` — the run ledger, runs apart, the co-worker pool, run counts, notes and registrations.
//! - `artifact` — the research settings a browse launches on.
//! - `gate` — the knowledge a piece of work is handed.
//! - `receiver` — the queue a run's ending is handed back to its room's session through.

pub mod agency;
pub mod artifact;
pub mod gate;
pub mod receiver;

use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// How long one call waits for its answer.
const CALL_WAIT: Duration = Duration::from_secs(60);

/// A refusal's answer: the sentence why, and its code.
#[derive(serde::Deserialize)]
struct Refusal {
  #[serde(default)]
  error: String,
  #[serde(default)]
  code: Option<String>,
}

/// One service at the address the settings name, under the token they name. A body is JSON both ways, a path
/// segment is percent-encoded, and a refusal's sentence is handed on with the service's name.
#[derive(Clone, Debug)]
pub struct Unit {
  name: &'static str,
  setting: &'static str,
  address: Option<String>,
  token: Option<String>,
  http: reqwest::Client,
}

/// A path's segment, percent-encoded where it needs to be.
pub fn segment(text: &str) -> String {
  url::form_urlencoded::byte_serialize(text.as_bytes())
    .collect::<String>()
    .replace('+', "%20")
}

impl Unit {
  pub fn new(
    name: &'static str,
    setting: &'static str,
    address: Option<String>,
    token: Option<String>,
  ) -> Self {
    Self {
      name,
      setting,
      address: address.filter(|address| !address.trim().is_empty()),
      token: token.filter(|token| !token.trim().is_empty()),
      http: reqwest::Client::builder().timeout(CALL_WAIT).build().unwrap_or_default(),
    }
  }

  /// One call: the status and the body; refused without a word sent where the address is not set.
  pub async fn call(
    &self,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<Value>,
  ) -> Result<(StatusCode, Vec<u8>), String> {
    let Some(address) = &self.address else {
      return Err(format!(
        "{}'s address is not set: set {} under [units] in the settings",
        self.name, self.setting
      ));
    };
    let mut asked = self
      .http
      .request(method, format!("{}{path}", address.trim_end_matches('/')))
      .query(query);

    if let Some(token) = &self.token {
      asked = asked.bearer_auth(token);
    }
    if let Some(body) = body {
      asked = asked.json(&body);
    }

    let answer = asked.send().await.map_err(|why| format!("{} could not be reached: {why}", self.name))?;
    let status = answer.status();
    let bytes = answer
      .bytes()
      .await
      .map_err(|why| format!("{} answered nothing readable: {why}", self.name))?;

    Ok((status, bytes.to_vec()))
  }

  /// A refusal as a sentence: the service's own where it answered one, else its status.
  pub fn refused(&self, status: StatusCode, body: &[u8]) -> String {
    match serde_json::from_slice::<Refusal>(body) {
      Ok(refusal) if !refusal.error.is_empty() => match refusal.code {
        Some(code) => format!("{} refused it ({code}): {}", self.name, refusal.error),
        None => format!("{} refused it: {}", self.name, refusal.error),
      },
      _ => format!("{} answered {status}", self.name),
    }
  }

  /// An answer read as its shape.
  pub fn read<T: DeserializeOwned>(&self, body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|why| format!("{} answered out of its shape: {why}", self.name))
  }

  /// One call answered with a body: the body read as its shape; any other answer refused.
  pub async fn ask<T: DeserializeOwned>(
    &self,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<Value>,
  ) -> Result<T, String> {
    let (status, said) = self.call(method, path, query, body).await?;

    if status.is_success() { self.read(&said) } else { Err(self.refused(status, &said)) }
  }

  /// A read of one thing: none where the service holds no such thing.
  pub async fn found<T: DeserializeOwned>(
    &self,
    path: &str,
    query: &[(&str, &str)],
  ) -> Result<Option<T>, String> {
    let (status, said) = self.call(Method::GET, path, query, None).await?;

    match status {
      StatusCode::NOT_FOUND => Ok(None),
      status if status.is_success() => self.read(&said).map(Some),
      status => Err(self.refused(status, &said)),
    }
  }

  /// One call answered with no body that matters: done, or refused.
  pub async fn done(&self, method: Method, path: &str, body: Option<Value>) -> Result<(), String> {
    let (status, said) = self.call(method, path, &[], body).await?;

    if status.is_success() { Ok(()) } else { Err(self.refused(status, &said)) }
  }
}

/// A body written as JSON.
pub fn json(body: &impl Serialize) -> Option<Value> {
  serde_json::to_value(body).ok()
}
