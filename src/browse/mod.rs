//! A browse run (docs/browse-runs.md): what a search cannot answer because it has to be seen, looked at by a
//! run of its own on the `browse` research profile, and handed back to the agent that asked.
//!
//! - `launch` — `bmo-explorer browse`: the run's row opened, its task written for the runner, the runner
//!   launched by bmo-agency, and for a sync launch its ending waited for.
//! - `runner` — `bmo-explorer runner browse`: the task taken, the model carried apart, and the ending handed
//!   back.
//! - `display` — a virtual X server of the run's own, for a Chrome that is not headless.
//! - `secrets` — the credential shapes masked out of what is asked about the task.
//!
//! What it stands on beside its own work is here: the profile it launched on, its clock, what its web tools are
//! told, its branches' seats in the common co-worker pool, the registration it was set off under where that
//! keeps its research, and the knowledge its words call for.

pub mod display;
pub mod launch;
pub mod runner;
pub mod secrets;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::clients::agency::{Agency, Mapping, Reservation, ReservationAsking};
use crate::clients::gate::{Gate, Piece};

/// The agent a browse is counted and logged under, the line it goes out on, its research key and work key.
pub const AGENT: &str = "browse";

/// The line its branches go out on: the search line, as every research branch does.
pub const BRANCH_LINE: &str = "search";

/// When now is, in epoch milliseconds.
pub fn now_ms() -> i64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}

fn whole(value: Option<&Value>) -> Option<i64> {
  value.and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|float| float as i64)))
}

/// The browse profile as a run launches on it: the values bmo-artifact's research settings hold for the key.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
  pub values: Value,
}

impl Profile {
  /// The profile bmo-artifact answered, or a launcher's copy read back: refused where it is not the browse key's
  /// or holds no clock, since a browse is the one research run held to one.
  pub fn of(values: Value) -> Result<Self, String> {
    if values.get("key").and_then(Value::as_str) != Some(AGENT) {
      return Err("the profile handed over is not the browse profile".to_owned());
    }

    let profile = Self { values };

    match (profile.cap_ms(), profile.synthesis_reserve_ms()) {
      (Some(cap), Some(reserve)) if cap > 0 && reserve >= 0 && reserve < cap => Ok(profile),
      _ => Err("the browse profile carries no ceiling".to_owned()),
    }
  }

  /// A launcher's copy read back.
  pub fn parse(snapshot: &str) -> Result<Self, String> {
    Self::of(
      serde_json::from_str(snapshot).map_err(|_| "the browse profile's copy is not JSON".to_owned())?,
    )
  }

  /// The copy a launcher hands its runner, so a run keeps the settings it started on.
  pub fn snapshot(&self) -> String {
    self.values.to_string()
  }

  pub fn cap_ms(&self) -> Option<i64> {
    whole(self.values.get("capMs"))
  }

  pub fn synthesis_reserve_ms(&self) -> Option<i64> {
    whole(self.values.get("synthesisReserveMs"))
  }

  pub fn child_role(&self) -> &str {
    self.values.get("childRole").and_then(Value::as_str).unwrap_or_default()
  }

  pub fn max_children(&self) -> i64 {
    whole(self.values.get("maxChildren")).unwrap_or_default()
  }

  pub fn strategy(&self) -> &str {
    self.values.get("strategy").and_then(Value::as_str).unwrap_or_default()
  }

  pub fn host_interval_ms(&self) -> i64 {
    whole(self.values.get("hostIntervalMs")).unwrap_or_default()
  }

  pub fn engine_interval_ms(&self) -> i64 {
    whole(self.values.get("engineIntervalMs")).unwrap_or_default()
  }

  /// The profile a run kept on its caller's registration launches on: no branch at all, so it reserves and
  /// waits for nothing in the pool and its tools serve no branch.
  pub fn kept_on(mut self, registration: Option<&str>) -> Self {
    if registration.is_some()
      && let Value::Object(fields) = &mut self.values
    {
      fields.insert("maxChildren".to_owned(), json!(0));
    }

    self
  }
}

/// A browse's clock (docs/browse-runs.md, 「The browse clock」): when it started, when its research closes,
/// and its ceiling, each in epoch milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
  pub started_at: i64,
  pub closes_at: i64,
  pub cap_at: i64,
}

/// The clock of a run on this profile started now.
pub fn clock(profile: &Profile, started_at: i64) -> Option<Clock> {
  let (cap, reserve) = (profile.cap_ms()?, profile.synthesis_reserve_ms()?);

  Some(Clock {
    started_at,
    closes_at: started_at + cap - reserve,
    cap_at: started_at + cap,
  })
}

/// What a research run's web tools are told about the run (docs/web-access.md, 「Settings from the research
/// run」): each value set is one variable, and one left unset is no variable at all.
#[derive(Clone, Debug, Default)]
pub struct Told {
  pub closes_at: Option<i64>,
  pub owner: Option<String>,
  pub source: Option<String>,
  pub role: Option<String>,
  pub max_children: Option<i64>,
  pub chat_id: Option<i64>,
  pub strategy: Option<String>,
  pub host_interval_ms: Option<i64>,
  pub engine_interval_ms: Option<i64>,
  /// Whether the tools serve the branch tool.
  pub branch_tool: bool,
}

impl Told {
  /// The variables the tools read them from, as a run apart's environment carries them.
  pub fn environment(&self) -> BTreeMap<String, String> {
    let mut told = BTreeMap::new();
    let mut set = |name: &str, value: Option<String>| {
      if let Some(value) = value.filter(|value| !value.is_empty()) {
        told.insert(name.to_owned(), value);
      }
    };

    set("BMO_RESEARCH_TOOLS", self.branch_tool.then(|| "1".to_owned()));
    set("BMO_RESEARCH_CLOSES_AT", self.closes_at.map(|at| at.to_string()));
    set("BMO_RESEARCH_OWNER", self.owner.clone());
    set("BMO_RESEARCH_SOURCE", self.source.clone());
    set("BMO_RESEARCH_ROLE", self.role.clone());
    set("BMO_RESEARCH_BRANCHES", self.max_children.map(|count| count.to_string()));
    set("BMO_RESEARCH_CHAT", self.chat_id.map(|chat| chat.to_string()));
    set("BMO_WEB_STRATEGY", self.strategy.clone());
    set("BMO_WEB_HOST_INTERVAL_MS", self.host_interval_ms.map(|ms| ms.to_string()));
    set("BMO_WEB_ENGINE_INTERVAL_MS", self.engine_interval_ms.map(|ms| ms.to_string()));
    told
  }
}

/// The owner a research run's places are held under in the pool.
pub fn owner(run_id: i64) -> String {
  format!("{AGENT}-run:{run_id}")
}

/// The provider a line starts on: the first enabled one carrying it.
pub async fn line_provider(agency: &Agency, line: &str) -> Result<String, String> {
  agency
    .providers()
    .await?
    .into_iter()
    .find(|provider| provider.enabled && provider.carries(line))
    .map(|provider| provider.name)
    .ok_or_else(|| format!("No enabled provider carries the {line} line"))
}

/// The route a work key's assigned tier takes on a provider, refused where no tier is assigned or no route
/// serves it.
pub async fn key_mapping(agency: &Agency, key: &str, provider: &str) -> Result<Mapping, String> {
  let tier = agency
    .assignments()
    .await?
    .into_iter()
    .find(|assignment| assignment.key == key)
    .map(|assignment| assignment.tier)
    .ok_or_else(|| format!("Missing tier assignment: {key}"))?;

  if !agency.tiers().await?.contains(&tier) {
    return Err(format!("Unknown model tier: {tier}"));
  }

  agency
    .resolve(key, provider, &tier)
    .await?
    .ok_or_else(|| format!("{provider} has no route for {key} at {tier}"))
}

/// The seats a browse's branches will use, held from before its clock starts until it ends, on the provider the
/// branch line starts on, at the branch role's own model.
pub async fn reserve_places(
  agency: &Agency,
  owner: &str,
  role: &str,
  count: i64,
) -> Result<Reservation, String> {
  let provider = line_provider(agency, BRANCH_LINE).await?;
  let lending = agency.providers().await?.into_iter().find(|one| one.name == provider);

  if !lending.is_some_and(|one| one.coworkers) {
    return Err(format!("{provider}: lends no co-workers"));
  }

  let mapping = key_mapping(agency, &format!("role:{role}"), &provider).await?;
  let asking = ReservationAsking {
    reservation_key: format!("{owner}:branches"),
    owner: owner.to_owned(),
    source: AGENT.to_owned(),
    role: role.to_owned(),
    provider: mapping.provider,
    model: mapping.model,
    effort: mapping.effort,
    tier: mapping.tier,
    wanted: count,
  };

  agency.reserve(&asking).await
}

/// Everything one run still holds, given back when it ends however it ended: its branches' reservation, and
/// every request of its own; how many were.
pub async fn finish_leases(agency: &Agency, owner: &str, outcome: &str) -> Result<i64, String> {
  let mut finished = 0;

  for held in agency.reservations(Some(owner)).await? {
    if agency.release(&held.token, outcome).await? {
      finished += 1;
    }
  }

  for request in agency.requests(Some(owner)).await? {
    if request.owner != owner || request.reservation.is_some() {
      continue;
    }

    if agency.finish(&request.token, outcome, request.state == "waiting").await? {
      finished += 1;
    }
  }

  Ok(finished)
}

// Whether the research run an owner names has ended: its run closed and its unit no longer running, or no run at
// all. Time alone proves nothing, since a branch inside a standing unit may still be running.
async fn ended(agency: &Agency, owner: &str, source: &str) -> Result<bool, String> {
  let Some((kind, id)) = owner.split_once("-run:") else { return Ok(false) };

  if !matches!(kind, "search" | "browse") || !matches!(source, "search" | "browse") {
    return Ok(false);
  }

  let Ok(id) = id.parse::<i64>() else { return Ok(false) };
  let Some(run) = agency.run(id).await? else { return Ok(true) };

  if run.finished_at.is_none() {
    return Ok(false);
  }

  // A unit nobody could say is running is held as running: nothing is taken from a run that may stand.
  let alive = agency.alive(std::slice::from_ref(&run.unit)).await?;

  Ok(alive.is_some_and(|alive| !alive.contains(&run.unit)))
}

/// What research runs left behind when their process died before they could give it back: only a run closed in
/// bmo-agency's ledger whose unit no longer runs is reclaimed; how many places came back.
pub async fn reclaim_leases(agency: &Agency) -> Result<i64, String> {
  let mut reclaimed = 0;

  for held in agency.reservations(None).await? {
    if ended(agency, &held.asking.owner, &held.asking.source).await?
      && agency.release(&held.token, "research run ended without giving its seats back").await?
    {
      reclaimed += 1;
    }
  }

  for request in agency.requests(None).await? {
    if request.reservation.is_some() || !ended(agency, &request.owner, &request.source).await? {
      continue;
    }

    if agency
      .finish(
        &request.token,
        "research run ended without giving its place back",
        request.state == "waiting",
      )
      .await?
    {
      reclaimed += 1;
    }
  }

  Ok(reclaimed)
}

/// The registration a launch is made under, where it keeps its research (docs/browse-runs.md): the one
/// bmo-agency says owns the home the launching process was handed. None for a launch with no home, and for one
/// whose registration does not keep its research.
pub async fn kept_research_on(agency: &Agency, home: Option<&str>) -> Result<Option<String>, String> {
  let Some(home) = home.filter(|home| !home.is_empty()) else { return Ok(None) };

  Ok(
    agency
      .registration_by_home(home)
      .await?
      .filter(|held| held.keeps_research)
      .map(|held| held.name),
  )
}

/// A quiet instruction as one of the lines a piece of work's knowledge is picked for.
pub fn instruction_line(text: &str) -> String {
  format!("quiet instruction from the bot: {text}")
}

/// The picked pieces in the shape a turn's reference stands in, so nothing in them can pass for the work's own
/// instructions, under one line saying whose they are and which way they cut; none where nothing was picked.
pub fn knowledge_block(pieces: &[Piece]) -> Option<String> {
  if pieces.is_empty() {
    return None;
  }

  let mut lines = vec![format!(
    "Reference for this browse ({}): what the agent's memory holds on its matter — facts as they stand now and the rules every room keeps. Where a document or a comment says otherwise, this is what is true.",
    pieces.len()
  )];

  for piece in pieces {
    lines.push(format!("[{}]", piece.path));
    lines.push(piece.text.trim().to_owned());
  }

  lines.push("END OF REFERENCE".to_owned());
  Some(lines.join("\n"))
}

/// The knowledge bmo-gate picks for a browse in a room, for its own lines; none where the gate picks nothing or
/// cannot be asked, which costs the run its reference and nothing else.
pub async fn work_knowledge(gate: &Gate, chat_id: i64, lines: &[String]) -> (Option<String>, Vec<String>) {
  match gate.work_knowledge(chat_id, lines).await {
    Ok(pieces) => (knowledge_block(&pieces), pieces.into_iter().map(|piece| piece.path).collect()),
    Err(why) => {
      eprintln!("chat {chat_id}: the knowledge for a browse could not be picked: {why}");
      (None, Vec::new())
    }
  }
}
