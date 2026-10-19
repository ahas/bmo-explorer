//! A research run's branches (docs/browse-runs.md, 「The profile and the branches」): the allowance and the
//! run's own close held here, and each branch carried by the documented command `bmo research-branch`
//! (docs/api.md, 「bmo-artifact」) — its place in the co-worker pool taken, its run launched, and its notes
//! handed back — in a process of its own that the run ending stops.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::Ordering;

use tokio::io::AsyncWriteExt;

use crate::web_helper::run::{RESEARCH_CLOSED, WebRun};

/// The command a branch is asked of: `bmo`, as the `PATH` the helper runs under finds it, which hands
/// `research-branch` to the unit that carries it.
pub const COMMAND: &str = "bmo";

/// The verb that carries one branch, and the arguments it is handed for this run's branch `index`.
pub fn branch_command(run: &WebRun, index: u64) -> (PathBuf, Vec<String>) {
  let research = &run.research;
  let mut args = vec![
    "research-branch".to_owned(),
    "--owner".into(),
    research.owner.clone().unwrap_or_default(),
    "--source".into(),
    research.source.clone().unwrap_or_default(),
    "--role".into(),
    research.role.clone().unwrap_or_default(),
    "--chat".into(),
    research.chat_id.to_string(),
    "--index".into(),
    index.to_string(),
  ];
  let mut named = |flag: &str, value: Option<String>| {
    if let Some(value) = value {
      args.push(flag.into());
      args.push(value);
    }
  };

  named("--closes-at", research.closes_at.map(|at| at.to_string()));
  named("--strategy", research.strategy.clone());
  named("--host-interval-ms", research.host_interval_ms.map(|ms| ms.to_string()));
  named("--engine-interval-ms", research.engine_interval_ms.map(|ms| ms.to_string()));

  (PathBuf::from(COMMAND), args)
}

/// One branch opened for the angle, or why not: refused by the run's allowance or its close here, and
/// otherwise carried by the verb, whose answer is the tool's.
pub async fn open(run: &WebRun, angle: &str) -> String {
  let research = &run.research;

  if run.closing.load(Ordering::SeqCst) {
    return "BRANCH REFUSED: this research run is ending.".into();
  }

  if research.owner.is_none() || research.source.is_none() || research.role.is_none() {
    return "BRANCH REFUSED: this run was not launched with a branch allowance. Research the question yourself with the web tools.".into();
  }

  if angle.trim().is_empty() {
    return "BRANCH REFUSED: a branch needs its whole angle written out.".into();
  }

  if run.guard.closed().is_some() {
    return RESEARCH_CLOSED.into();
  }

  let max = u64::from(research.max_children);
  let index = run.opened.fetch_add(1, Ordering::SeqCst) + 1;

  if index > max {
    run.opened.fetch_sub(1, Ordering::SeqCst);

    return if max == 0 {
      "BRANCH REFUSED: this run is allowed no branches. Research the question yourself with the web tools."
        .into()
    } else {
      format!(
        "BRANCH REFUSED: this run has already opened the {max} branches it is allowed. Every further request is refused the same way; write the result from what you already have."
      )
    };
  }

  let (program, args) = branch_command(run, index);
  let spawned = tokio::process::Command::new(&program)
    .args(&args)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn();
  let mut child = match spawned {
    Ok(child) => child,
    Err(why) => {
      return format!(
        "BRANCH {index} FAILED: {} could not be started: {why}. Write the result from what you already have.",
        program.display()
      );
    }
  };
  let pid = child.id();

  if let Some(pid) = pid {
    run.branches.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(pid);
  }

  if let Some(mut stdin) = child.stdin.take() {
    let _ = stdin.write_all(angle.as_bytes()).await;
  }

  let finished = child.wait_with_output().await;

  if let Some(pid) = pid {
    run
      .branches
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .retain(|one| *one != pid);
  }

  match finished {
    Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    Ok(output) => {
      let said = String::from_utf8_lossy(&output.stderr);
      let why = said.trim().lines().last().unwrap_or("the branch's verb ended without its notes");

      format!("BRANCH {index} FAILED: {why}. Write the result from what you already have.")
    }
    Err(why) => format!("BRANCH {index} FAILED: {why}. Write the result from what you already have."),
  }
}
