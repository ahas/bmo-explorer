//! `browse` (docs/browse-runs.md, 「The browse tool」): a browsing run that opens pages, launched through the
//! unit command; what it saw comes back in the call (sync) or to the next turn (async).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::launching::{ActedOn, Launch, Mode, Turn, common_args, launched, settled};
use crate::tool::{Answer, Grant, Hints, Served, Session, Tool};

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&Browse];

// The line a login offered apart from the task opens with, in the text a browse run is handed.
const OFFERED_LOGIN: &str = "\n\nCredentials their holder offered for this task: ";

/// What a browse run is handed: the task, and the login its holder offered for it, where one was given apart
/// from the task. The runner's rules say what a login in the task may be used for.
pub fn task_text(task: &str, secrets: Option<&str>) -> String {
  let login = secrets.map(str::trim).unwrap_or_default();

  if login.is_empty() {
    task.trim().to_owned()
  } else {
    format!("{}{OFFERED_LOGIN}{login}", task.trim())
  }
}

/// The task a browse run was handed, without the login offered apart from it.
pub fn task_words(text: &str) -> String {
  text.find(OFFERED_LOGIN).map_or(text, |at| &text[..at]).trim().to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowseInput {
  /// The room, by chat id, where it is not this turn's own.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub chat: Option<i64>,
  /// What to do. Never a login.
  pub task: String,
  /// The login its holder offered, if the task needs one.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub secrets: Option<String>,
  /// sync: what it saw comes back in this call, waited for however long it takes.
  /// async, where not given: it answers your next turn.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mode: Option<Mode>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<ActedOn>,
}

/// The launch a call hands the unit command: `browse` with the run's words, and the task on standard input,
/// since it may carry a login and arguments are visible in the process list.
pub fn launch(turn: &Turn, input: &BrowseInput) -> Result<Launch, String> {
  // A browse opens pages for minutes, mostly past the wait.
  let (room, by, mode) = settled(turn, input.chat, input.mode, Mode::Async)?;
  let mut args = vec!["browse".to_owned()];

  args.extend(common_args(turn, room, &by, mode));
  Ok(Launch {
    args,
    input: task_text(&input.task, input.secrets.as_deref()),
  })
}

/// A browsing run that opens pages.
pub struct Browse;

impl Tool for Browse {
  const NAME: &'static str = "browse";
  const NAMESPACE: &'static str = "bmo";
  const DESCRIPTION: &'static str = "Launch a browsing run that opens pages; what it saw comes back in this call (sync) or to your next turn (async).";
  // A launch reaches past this machine, takes nothing away, and a second call starts a second run.
  const HINTS: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: None,
    open_world: true,
  };
  const CAPABILITIES: &'static [&'static str] = &["room.tools"];
  const GRANT: Option<Grant> = Some(Grant::Search);

  type Input = BrowseInput;

  async fn call(&self, session: &Session, input: BrowseInput) -> Answer {
    let turn = Turn::of(session);

    match launch(&turn, &input) {
      Ok(launch) => launched(&turn, &launch).await,
      Err(why) => Answer::refusal(why),
    }
  }
}
