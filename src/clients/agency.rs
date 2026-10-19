//! bmo-agency (docs/api.md, 「bmo-agency」): the run ledger, the run apart a browse's model is carried in, the
//! co-worker pool its branches' seats are held in, the run counts, the notes in a run's log and the
//! registration a launch is made under. Bodies are JSON with snake_case keys, as bmo-agency takes them.

use std::collections::BTreeMap;

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Unit, json, segment};

/// One provider, in the provider order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
  pub name: String,
  pub enabled: bool,
  /// The work lines its program carries.
  #[serde(default)]
  pub lines: Vec<String>,
  /// Whether it lends co-workers.
  #[serde(default)]
  pub coworkers: bool,
}

impl Provider {
  pub fn carries(&self, line: &str) -> bool {
    self.lines.iter().any(|one| one == line)
  }
}

/// The tier a work key is assigned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
  pub key: String,
  pub tier: String,
}

/// The model and effort a route resolves to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
  pub provider: String,
  pub tier: String,
  pub model: String,
  #[serde(default)]
  pub effort: Option<String>,
}

/// Seats asked to be held for one owner's branches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationAsking {
  pub reservation_key: String,
  pub owner: String,
  pub source: String,
  pub role: String,
  pub provider: String,
  pub model: String,
  #[serde(default)]
  pub effort: Option<String>,
  pub tier: String,
  pub wanted: i64,
}

/// Seats as the pool holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
  #[serde(flatten)]
  pub asking: ReservationAsking,
  pub token: String,
  #[serde(default)]
  pub state: String,
}

/// A request for a place as the pool holds it: what reclaiming and giving back read of it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRequest {
  pub token: String,
  pub owner: String,
  pub source: String,
  #[serde(default)]
  pub reservation: Option<String>,
  /// `waiting`, `active` or `ended`.
  pub state: String,
}

/// A run's row as it is opened in the run ledger, its times in epoch seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRun {
  pub agent: String,
  pub chat_id: Option<i64>,
  pub unit: String,
  pub depth: i64,
  pub cap_ms: Option<i64>,
  pub started_at: i64,
  pub started_by: Option<String>,
}

/// One run of the ledger, its times in epoch seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
  pub id: i64,
  #[serde(default)]
  pub unit: String,
  #[serde(default)]
  pub finished_at: Option<i64>,
  #[serde(default)]
  pub outcome: Option<String>,
}

/// How a run ended, in epoch seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunClose {
  pub finished_at: i64,
  pub outcome: String,
  pub context_tokens: Option<i64>,
}

/// The process a run is: the program, its words, the folder it starts in and the variables it is handed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
  pub program: String,
  pub args: Vec<String>,
  pub cwd: String,
  pub environment: BTreeMap<String, String>,
}

/// A run's runner as bmo-agency is asked to launch it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
  pub title: String,
  pub agent: String,
  pub command: Command,
}

/// What a run apart goes out on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Road {
  /// A tier's model on a provider.
  Key {
    provider: String,
    model: String,
    effort: Option<String>,
    tier: String,
  },
  /// A registration, with its own model.
  Registration { name: String },
}

/// The instructions bmo-agency composes for a run apart's line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instructions {
  pub name: String,
  pub text: Option<String>,
}

/// The run log every line a run apart writes is fed to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApartLog {
  pub run: String,
  pub chat_id: Option<i64>,
}

/// A run apart asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApartAsking {
  /// The agent it is counted under.
  pub agent: String,
  pub line: String,
  pub title: String,
  pub work_key: String,
  pub road: Road,
  pub instructions: Instructions,
  /// The variables its tools are told.
  pub environment: BTreeMap<String, String>,
  pub prompt: String,
  pub log: Option<ApartLog>,
  /// The clock it is held to: past this many milliseconds bmo-agency asks before it cuts the run.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub bound_ms: Option<i64>,
}

/// What a run apart answered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApartReply {
  pub text: String,
  #[serde(default)]
  pub context_tokens: Option<i64>,
  /// The names of its own work still running as it ended.
  #[serde(default)]
  pub live_tasks: Vec<String>,
}

/// A run apart as it stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Apart {
  pub id: String,
  /// `running`, `answered`, `failed` or `stopped`.
  pub state: String,
  #[serde(default)]
  pub reply: Option<ApartReply>,
  #[serde(default)]
  pub failure: Option<String>,
}

/// A note in a run's log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
  pub agent: String,
  pub run: String,
  pub chat_id: Option<i64>,
  pub text: String,
}

/// The registration that owns a home.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
  pub name: String,
  pub keeps_research: bool,
}

#[derive(Deserialize)]
struct Opened {
  id: serde_json::Value,
}

#[derive(Deserialize)]
struct Released {
  released: bool,
}

#[derive(Deserialize)]
struct Finished {
  finished: bool,
}

#[derive(Deserialize)]
struct Alive {
  #[serde(default)]
  alive: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Tier {
  name: String,
}

/// bmo-agency at the address the settings name, under the token they name.
#[derive(Clone, Debug)]
pub struct Agency {
  unit: Unit,
}

impl Agency {
  pub fn new(address: Option<String>, token: Option<String>) -> Self {
    Self {
      unit: Unit::new("bmo-agency", "agency", address, token),
    }
  }

  /// The providers, in the provider order.
  pub async fn providers(&self) -> Result<Vec<Provider>, String> {
    self.unit.ask(Method::GET, "/providers", &[], None).await
  }

  /// Every work key's tier.
  pub async fn assignments(&self) -> Result<Vec<Assignment>, String> {
    self.unit.ask(Method::GET, "/tiers/assignments", &[], None).await
  }

  /// The model tiers' names.
  pub async fn tiers(&self) -> Result<Vec<String>, String> {
    let tiers: Vec<Tier> = self.unit.ask(Method::GET, "/tiers", &[], None).await?;

    Ok(tiers.into_iter().map(|tier| tier.name).collect())
  }

  /// The route a work key's tier takes on a provider, none where none serves it.
  pub async fn resolve(
    &self,
    work_key: &str,
    provider: &str,
    tier: &str,
  ) -> Result<Option<Mapping>, String> {
    let body = json!({ "work_key": work_key, "provider": provider, "tier": tier });
    let (status, said) = self.unit.call(Method::POST, "/routes/resolve", &[], Some(body)).await?;

    match status {
      StatusCode::NOT_FOUND => Ok(None),
      status if status.is_success() => self.unit.read(&said).map(Some),
      status => Err(self.unit.refused(status, &said)),
    }
  }

  /// Whether a provider and a model accept an effort; refused with the sentence where they do not.
  pub async fn check_effort(&self, provider: &str, model: &str, effort: &str) -> Result<(), String> {
    let body = json!({ "provider": provider, "model": model, "effort": effort });

    self.unit.done(Method::POST, "/routes/check-effort", Some(body)).await
  }

  /// Seats held for one owner's branches.
  pub async fn reserve(&self, asking: &ReservationAsking) -> Result<Reservation, String> {
    self.unit.ask(Method::POST, "/pool/reservations", &[], json(asking)).await
  }

  /// The reservations held, of one owner or of every one.
  pub async fn reservations(&self, owner: Option<&str>) -> Result<Vec<Reservation>, String> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();

    self.unit.ask(Method::GET, "/pool/reservations", &query, None).await
  }

  /// A reservation given back; whether it was this call that gave it back.
  pub async fn release(&self, token: &str, reason: &str) -> Result<bool, String> {
    let path = format!("/pool/reservations/{}/release", segment(token));
    let released: Released =
      self.unit.ask(Method::POST, &path, &[], Some(json!({ "reason": reason }))).await?;

    Ok(released.released)
  }

  /// The requests waiting or active, of one owner or of every one.
  pub async fn requests(&self, owner: Option<&str>) -> Result<Vec<PoolRequest>, String> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();

    self.unit.ask(Method::GET, "/pool/requests", &query, None).await
  }

  /// A request ended, cancelled where it never ran; whether it was this call that ended it.
  pub async fn finish(&self, token: &str, outcome: &str, cancel: bool) -> Result<bool, String> {
    let path = format!("/pool/requests/{}/finish", segment(token));
    let body = json!({ "outcome": outcome, "cancel": cancel });
    let finished: Finished = self.unit.ask(Method::POST, &path, &[], Some(body)).await?;

    Ok(finished.finished)
  }

  /// Which of the names are running; none where nobody could say.
  pub async fn alive(&self, names: &[String]) -> Result<Option<Vec<String>>, String> {
    let alive: Alive =
      self.unit.ask(Method::POST, "/runs/alive", &[], Some(json!({ "names": names }))).await?;

    Ok(alive.alive)
  }

  /// A run's row opened in the ledger: its number.
  pub async fn open_run(&self, run: &NewRun) -> Result<i64, String> {
    let opened: Opened = self.unit.ask(Method::POST, "/runs", &[], json(run)).await?;

    opened
      .id
      .as_i64()
      .or_else(|| opened.id.as_str().and_then(|id| id.parse().ok()))
      .ok_or_else(|| "bmo-agency answered a run with no number".to_owned())
  }

  /// A run's runner launched; bmo-agency's sentence where it did not come up.
  pub async fn launch(&self, run_id: i64, launch: &Launch) -> Result<(), String> {
    self.unit.done(Method::POST, &format!("/runs/{run_id}/launch"), json(launch)).await
  }

  /// One run of the ledger, none where it holds no such run.
  pub async fn run(&self, run_id: i64) -> Result<Option<Run>, String> {
    self.unit.found(&format!("/runs/{run_id}"), &[]).await
  }

  /// A run's row closed: whether this was the ending that closed it, false where it was closed already.
  pub async fn close_run(&self, run_id: i64, close: &RunClose) -> Result<bool, String> {
    let (status, said) =
      self.unit.call(Method::POST, &format!("/runs/{run_id}/close"), &[], json(close)).await?;

    match status {
      StatusCode::CONFLICT => Ok(false),
      status if status.is_success() => Ok(true),
      status => Err(self.unit.refused(status, &said)),
    }
  }

  /// A run apart started: its id.
  pub async fn start_apart(&self, asking: &ApartAsking) -> Result<String, String> {
    let opened: Opened = self.unit.ask(Method::POST, "/apart", &[], json(asking)).await?;

    Ok(opened.id.as_str().map_or_else(|| opened.id.to_string(), str::to_owned))
  }

  /// A run apart as it stands.
  pub async fn apart(&self, id: &str) -> Result<Apart, String> {
    self.unit.ask(Method::GET, &format!("/apart/{}", segment(id)), &[], None).await
  }

  /// A run apart stopped.
  pub async fn stop_apart(&self, id: &str) -> Result<(), String> {
    self.unit.done(Method::POST, &format!("/apart/{}/stop", segment(id)), None).await
  }

  /// A run counted on its agent.
  pub async fn count_run(&self, agent: &str) -> Result<(), String> {
    self.unit.done(Method::POST, &format!("/agents/{}/runs", segment(agent)), None).await
  }

  /// A note written in a run's log.
  pub async fn note(&self, note: &Note) -> Result<(), String> {
    self.unit.done(Method::POST, "/log/notes", json(note)).await
  }

  /// The registration that owns a home, none where no registration does.
  pub async fn registration_by_home(&self, home: &str) -> Result<Option<Registration>, String> {
    self.unit.found("/registrations/by-home", &[("home", home)]).await
  }
}
