//! What the launching tools share (docs/api.md, 「The tools」): the turn a call is made in, as its session's
//! variables carry it, the mode its run's ending comes back in, and the run handed to the unit command, which
//! opens it, launches its runner and, for a sync call, waits for its ending.

use std::collections::BTreeMap;
use std::process::Stdio;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::tool::{Answer, Session};

/// The unit command a run is handed to, as the `PATH` the tool runs under finds it.
pub const UNIT_COMMAND: &str = "bmo-explorer";

/// The variable naming the room a turn answers.
pub const CHAT_VARIABLE: &str = "BMO_TURN_CHAT";

/// The variable naming the road a turn was read on.
pub const VIA_VARIABLE: &str = "BMO_TURN_VIA";

/// The refusal of a call naming no room in a turn that answers none.
pub const NO_ROOM: &str = "no room is named: name one with chat, or call this inside a room's turn";

/// When a call's result comes back: in the call's own answer, for this turn to go on with, or to the next turn
/// as a quiet instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Mode {
  Sync,
  Async,
}

impl Mode {
  /// The mode as a command's argument names it.
  pub fn word(self) -> &'static str {
    match self {
      Self::Sync => "sync",
      Self::Async => "async",
    }
  }
}

/// The message a call acts on; the tool host checks the grant of a call by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActedOn(pub i64);

impl JsonSchema for ActedOn {
  fn inline_schema() -> bool {
    true
  }

  fn schema_name() -> std::borrow::Cow<'static, str> {
    "ActedOn".into()
  }

  fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
      "type": "integer",
      "description": "The message this call acts on: the id of the person's message in this turn that asked for it. Omitted only in a turn whose every new message is the owner's, or that no person's message opened.",
    })
  }
}

/// The turn a call is made in, as its session carries it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
  /// Every variable of the turn, read afresh.
  pub variables: BTreeMap<String, String>,
}

impl Turn {
  pub fn of(session: &Session) -> Self {
    Self {
      variables: session.turn_environment(),
    }
  }

  fn set(&self, name: &str) -> Option<&str> {
    self.variables.get(name).map(|value| value.trim()).filter(|value| !value.is_empty())
  }

  /// The room the turn answers.
  pub fn chat(&self) -> Option<i64> {
    self.set(CHAT_VARIABLE).and_then(|chat| chat.parse().ok())
  }

  pub fn via(&self) -> Option<&str> {
    self.set(VIA_VARIABLE)
  }
}

/// One run as the unit command is asked to launch it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
  /// The unit command's words.
  pub args: Vec<String>,
  /// What it reads on standard input.
  pub input: String,
}

/// Where a run reports, who asked it and when its ending comes back: the room the call names or the turn's
/// own, set off by the turn's room, and the mode the call asked for, or `default`, in the turn's own room and
/// async in another, whose ending is that room's session's to answer.
pub fn settled(
  turn: &Turn,
  named: Option<i64>,
  asked: Option<Mode>,
  default: Mode,
) -> Result<(i64, String, Mode), String> {
  let room = named.or_else(|| turn.chat()).ok_or(NO_ROOM)?;
  let own_room = turn.chat().is_none_or(|chat| chat == room);
  let mode = if own_room { asked.unwrap_or(default) } else { Mode::Async };
  let by = format!("chat:{}", turn.chat().unwrap_or(room));

  Ok((room, by, mode))
}

/// The words every launch carries: the room, who set it off, the road the turn was read on and the mode.
pub fn common_args(turn: &Turn, room: i64, by: &str, mode: Mode) -> Vec<String> {
  let mut args = vec!["--chat".to_owned(), room.to_string(), "--by".to_owned(), by.to_owned()];

  if let Some(via) = turn.via() {
    args.extend(["--via".to_owned(), via.to_owned()]);
  }

  args.extend(["--mode".to_owned(), mode.word().to_owned()]);
  args
}

/// The run handed to the unit command under the turn's variables: its lines answer the call, and a refusal or a
/// command that would not start is the call's refusal.
pub async fn launched(turn: &Turn, launch: &Launch) -> Answer {
  let mut command = tokio::process::Command::new(UNIT_COMMAND);

  command
    .args(&launch.args)
    .envs(&turn.variables)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

  let mut child = match command.spawn() {
    Ok(child) => child,
    Err(why) => return Answer::refusal(format!("ERROR: {UNIT_COMMAND} could not be started: {why}")),
  };

  if let Some(mut stdin) = child.stdin.take() {
    // A launch that has stopped reading says why on its own error stream, read below.
    let _ = stdin.write_all(launch.input.as_bytes()).await;
  }

  let ended = match child.wait_with_output().await {
    Ok(ended) => ended,
    Err(why) => return Answer::refusal(format!("ERROR: {UNIT_COMMAND} did not end: {why}")),
  };
  let said = String::from_utf8_lossy(&ended.stdout).trim().to_owned();

  if ended.status.success() {
    Answer::text(said)
  } else {
    let why = String::from_utf8_lossy(&ended.stderr).trim().to_owned();

    Answer::refusal([said, why].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n"))
  }
}
