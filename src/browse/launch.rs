//! `bmo-explorer browse` (docs/browse-runs.md, 「The browse tool」; docs/api.md, 「The unit command」): the
//! task written for the run, the places dead research runs left in the pool given back, the run's row opened
//! on the profile as it stands now — with no branch where the launch stands under a registration that keeps its
//! research — and the runner launched by bmo-agency. A sync launch then waits for the run's ending and prints
//! it. A launch whose run never came up closes its own row and takes the task file with it.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::runner::failed_text;
use super::{AGENT, Profile, kept_research_on, now_ms, reclaim_leases};
use crate::clients::agency::{Agency, Command, Launch, NewRun, RunClose};
use crate::clients::artifact::Artifact;
use crate::settings::{DATA_ROOT_VARIABLE, Settings};
use crate::tools::launching::Mode;

/// What a refusal of a launch's words says.
pub const USAGE: &str = "usage: bmo-explorer browse --chat <chatId> [--by <who>] [--via <road>] [--mode <sync|async>]  (the task is read from stdin)";

/// The variable a launching process's home is read from, by which bmo-agency says which registration it runs on.
pub const HOME_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The variable naming the agent whose turn asked, which the run's ending goes back to.
pub const AGENT_VARIABLE: &str = "BMO_TURN_AGENT";

/// How often a sync launch asks whether its run has ended.
const ENDING_POLL: Duration = Duration::from_secs(1);

/// One browse as it is asked: the room it reports to, what to do, who set it off, the agent whose turn asked,
/// the road the turn was read on, when its ending comes back and the home the launching process was handed.
#[derive(Clone, Debug, Default)]
pub struct Asked {
  pub chat_id: i64,
  pub task: String,
  pub started_by: Option<String>,
  pub asked_by: Option<String>,
  pub via: Option<String>,
  pub mode: Option<Mode>,
  pub home: Option<String>,
}

// A file of the run's own in the temporary directory, owner-only from the moment it exists.
fn private_file(path: &Path, text: &str) -> Result<(), String> {
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(path)
    .and_then(|mut file| file.write_all(text.as_bytes()))
    .map_err(|why| format!("{} could not be written: {why}", path.display()))
}

// The scratch files a launch made, taken away where its run never came up.
fn take_away(files: &[&PathBuf]) {
  for file in files {
    let _ = std::fs::remove_file(file);
  }
}

/// The runner's words for one run.
pub fn runner_args(
  asked: &Asked,
  task_path: &Path,
  run_id: i64,
  unit: &str,
  profile: &Profile,
  kept_on: Option<&str>,
  ending: Option<&Path>,
) -> Vec<String> {
  let mut args = vec![
    "runner".to_owned(),
    "browse".to_owned(),
    asked.chat_id.to_string(),
    task_path.display().to_string(),
    run_id.to_string(),
    unit.to_owned(),
    profile.snapshot(),
  ];

  if let Some(via) = &asked.via {
    args.extend(["--via".to_owned(), via.clone()]);
  }
  if let Some(registration) = kept_on {
    args.extend(["--kept-on".to_owned(), registration.to_owned()]);
  }
  if let Some(ending) = ending {
    args.extend(["--ending".to_owned(), ending.display().to_string()]);
  }

  args
}

/// The browse launched: the lines said of it — for a sync launch, its ending — or why it was refused.
pub async fn launch(settings: &Settings, asked: &Asked) -> Result<Vec<String>, String> {
  if asked.task.trim().is_empty() {
    return Err(USAGE.to_owned());
  }

  let units = &settings.units;
  let agency = Agency::new(units.agency.clone(), units.agency_token.clone());
  let artifact = Artifact::new(units.artifact.clone(), units.artifact_token.clone());
  // A unit of its own, however many launches start in one millisecond.
  let unit = format!("bmo-browse-{}-{}", now_ms(), std::process::id());
  let task_path = std::env::temp_dir().join(format!("{unit}.txt"));

  // The task may carry somebody's credentials, so it reaches the runner through a file only its owner reads,
  // which the runner deletes the moment it has read it, and never through a command line.
  private_file(&task_path, asked.task.trim())?;

  if let Err(why) = reclaim_leases(&agency).await {
    eprintln!("browse: the places dead research runs left could not be reclaimed: {why}");
  }

  let opened = async {
    let kept_on = kept_research_on(&agency, asked.home.as_deref()).await?;
    let profile = Profile::of(artifact.profile(AGENT).await?)?.kept_on(kept_on.as_deref());
    // The row is opened before the run is, so a browse is in the record from the moment it exists.
    let run_id = agency
      .open_run(&NewRun {
        agent: AGENT.to_owned(),
        chat_id: Some(asked.chat_id),
        unit: unit.clone(),
        depth: 0,
        cap_ms: profile.cap_ms(),
        started_at: now_ms() / 1000,
        started_by: asked.started_by.clone(),
      })
      .await?;

    Ok::<_, String>((kept_on, profile, run_id))
  };
  let (kept_on, profile, run_id) = match opened.await {
    Ok(opened) => opened,
    Err(why) => {
      take_away(&[&task_path]);
      return Err(format!("browse: {why}"));
    }
  };
  let sync = asked.mode == Some(Mode::Sync);
  let ending = sync.then(|| std::env::temp_dir().join(format!("{unit}.ending")));

  if let Some(ending) = &ending
    && let Err(why) = private_file(ending, "")
  {
    take_away(&[&task_path]);
    return Err(format!("browse: {why}"));
  }

  let mut environment =
    BTreeMap::from([(DATA_ROOT_VARIABLE.to_owned(), settings.data_root.display().to_string())]);

  if let Some(agent) = &asked.asked_by {
    environment.insert(AGENT_VARIABLE.to_owned(), agent.clone());
  }

  let command = Command {
    program: std::env::current_exe()
      .map_err(|why| format!("browse: this program's own path could not be read: {why}"))?
      .display()
      .to_string(),
    args: runner_args(asked, &task_path, run_id, &unit, &profile, kept_on.as_deref(), ending.as_deref()),
    cwd: std::env::current_dir()
      .map(|dir| dir.display().to_string())
      .unwrap_or_else(|_| "/".to_owned()),
    environment,
  };
  // The room it was asked from and never the task, which may carry somebody's credentials.
  let launch = Launch {
    title: format!("browse for chat {}", asked.chat_id),
    agent: AGENT.to_owned(),
    command,
  };

  if let Err(why) = agency.launch(run_id, &launch).await {
    take_away(&[&task_path]);
    if let Some(ending) = &ending {
      take_away(&[ending]);
    }

    // bmo-agency closes a run its host did not start; one it never heard of is closed here, as an error.
    let close = RunClose {
      finished_at: now_ms() / 1000,
      outcome: "error".to_owned(),
      context_tokens: None,
    };

    if let Err(closing) = agency.close_run(run_id, &close).await {
      eprintln!("browse: the run's row could not be closed: {closing}");
    }

    return Err(format!("{unit} failed to launch: {why}"));
  }

  let Some(ending) = ending else {
    return Ok(vec![format!(
      "browse launched as {unit}; it reports to chat {} by itself when it is done. Do not wait for it and do not answer from memory.",
      asked.chat_id
    )]);
  };

  Ok(vec![waited(&agency, run_id, &ending).await])
}

/// A sync launch's wait: until the run's row is closed, then the ending its runner wrote, or the failure's
/// words where it wrote none.
pub async fn waited(agency: &Agency, run_id: i64, ending: &Path) -> String {
  loop {
    match agency.run(run_id).await {
      Ok(Some(run)) if run.finished_at.is_some() => break,
      Ok(None) => break,
      Ok(Some(_)) => {}
      Err(why) => eprintln!("browse: the run could not be read: {why}"),
    }

    tokio::time::sleep(ENDING_POLL).await;
  }

  let said = std::fs::read_to_string(ending).unwrap_or_default();
  let _ = std::fs::remove_file(ending);

  if said.trim().is_empty() { failed_text() } else { said.trim().to_owned() }
}
