//! `remote_browser` (docs/remote-browser.md, 「The launch」): a browser on a remote node driven through a list of
//! steps, launched through the unit command; what it did comes back in the call (sync) or to the next turn
//! (async).

use std::io::Write;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::launching::{ActedOn, Launch, Mode, Turn, common_args, launched, settled};
use crate::tool::{Answer, Grant, Hints, Served, Session, Tool};

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&RemoteBrowser];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteBrowserInput {
  /// The room, by chat id, where it is not this turn's own.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub chat: Option<i64>,
  /// The remote node.
  pub node: String,
  /// The steps, as the JSON a steps file holds.
  pub plan: String,
  /// The values the steps name, as NAME=value lines.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub secrets: Option<String>,
  /// sync, where not given: what it did comes back in this call, waited for however long it takes.
  /// async: it answers your next turn.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mode: Option<Mode>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<ActedOn>,
}

/// The launch a call hands the unit command: `remote-browser` with the run's words and the steps file, and the
/// secrets on standard input, since arguments are visible in the process list.
pub fn launch(turn: &Turn, input: &RemoteBrowserInput, steps: &std::path::Path) -> Result<Launch, String> {
  // A list of steps is mostly done in well under a minute.
  let (room, by, mode) = settled(turn, input.chat, input.mode, Mode::Sync)?;
  let mut args = vec!["remote-browser".to_owned()];

  args.extend(common_args(turn, room, &by, mode));
  args.extend([
    "--node".to_owned(),
    input.node.clone(),
    "--steps".to_owned(),
    steps.display().to_string(),
  ]);
  Ok(Launch {
    args,
    input: input.secrets.clone().unwrap_or_default(),
  })
}

/// A browser on a remote node driven through a list of steps.
pub struct RemoteBrowser;

impl Tool for RemoteBrowser {
  const NAME: &'static str = "remote_browser";
  const NAMESPACE: &'static str = "bmo";
  const DESCRIPTION: &'static str = "Drive a browser on a remote node through a list of steps; what it did comes back in this call (sync) or to your next turn (async).";
  // A launch reaches past this machine, takes nothing away, and a second call starts a second run.
  const HINTS: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: None,
    open_world: true,
  };
  const CAPABILITIES: &'static [&'static str] = &["room.tools"];
  const GRANT: Option<Grant> = Some(Grant::Launch);

  type Input = RemoteBrowserInput;

  async fn call(&self, session: &Session, input: RemoteBrowserInput) -> Answer {
    let turn = Turn::of(session);
    // The steps cross to the command in a file only its owner reads, taken away once the launch has read it.
    let steps = tempfile::Builder::new().prefix("remote-browser-steps-").suffix(".json").tempfile();
    let written = steps.and_then(|mut steps| steps.write_all(input.plan.as_bytes()).map(|()| steps));
    let steps = match written {
      Ok(steps) => steps,
      Err(why) => return Answer::refusal(format!("ERROR: the steps could not be written: {why}")),
    };

    match launch(&turn, &input, steps.path()) {
      Ok(launch) => launched(&turn, &launch).await,
      Err(why) => Answer::refusal(why),
    }
  }
}
