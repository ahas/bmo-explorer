//! `research`, the web namespace's research branch (docs/api.md, 「The research branch」): one branch opened
//! by the record's web helper, which holds the run's allowance and its close and ends the branches still running
//! as the record ends.

use crate::site::helper::{WebAsked, ask};
use crate::tool::{Answer, Hints, NonEmpty, Served, Session, Tool};
use schemars::JsonSchema;
use serde::Deserialize;

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&Research];

#[derive(Deserialize, JsonSchema)]
pub struct BranchInput {
  /// The whole angle this branch researches, self-contained: what to find out and what a finished answer to
  /// it is.
  angle: NonEmpty,
}

pub struct Research;

impl Tool for Research {
  const NAME: &'static str = "research";
  const NAMESPACE: &'static str = "web";
  const TITLE: Option<&'static str> = Some("Research");
  const DESCRIPTION: &'static str = "Opens one research branch: a separate run that researches the angle you give it with the web tools and returns its notes when it has finished, which takes a minute or two. It takes a place in the common co-worker pool first and waits for one. The run's allowance and the pool decide, and a refusal is final for that request. Several branches may be opened in parallel with separate calls, and opening them together is how a turn's angles run side by side rather than one after another. What a branch returns is unchecked working notes.";
  // It changes nothing on this machine and says so: Claude Code sends the calls of a tool side by side only
  // when it is marked read-only, and one at a time otherwise, which would run a turn's branches in series
  // however many places the pool has free.
  const HINTS: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: None,
    open_world: true,
  };
  const CAPABILITIES: &'static [&'static str] = &["web.branches"];

  type Input = BranchInput;

  async fn call(&self, session: &Session, input: BranchInput) -> Answer {
    let asked = WebAsked::Branch {
      angle: input.angle.to_string(),
    };

    ask(session, &asked).await
  }
}
