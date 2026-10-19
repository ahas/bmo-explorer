//! `bmo-explorer`, the unit command (docs/api.md, 「The unit command」): asked `nouns`, it prints the nouns it
//! carries; asked one of them, it carries it. It prints what it did on standard output and why it refused on
//! standard error, and ends 0 where it did what was asked, 1 where it refused, and 2 where its words do not read.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use bmo_explorer::browse::launch::{AGENT_VARIABLE, Asked, HOME_VARIABLE, launch};
use bmo_explorer::browse::runner::{Started, browse_runner};
use bmo_explorer::remote_browser::{self, launch_remote_browser, remote_browser_runner};
use bmo_explorer::settings::Settings;
use bmo_explorer::tools::launching::Mode;
use clap::{Args, Parser, Subcommand};

/// The nouns this command carries, one a line, as `bmo` asks them.
pub const NOUNS: &[&str] = &[
  "browse",
  "runner browse",
  "browse-runner",
  "remote-browser",
  "runner remote-browser",
  "remote-browser-runner",
];

#[derive(Parser)]
#[command(
  name = "bmo-explorer",
  about = "The browse and remote-browser runs and the web tools' own commands"
)]
struct Cli {
  /// The data root; `BMO_DATA_DIR` where none is named.
  #[arg(long, global = true)]
  data_root: Option<PathBuf>,
  #[command(subcommand)]
  verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
  /// The nouns this command carries.
  Nouns,
  /// Launch a browse for a room; the task comes in on standard input and never as an argument.
  #[command(allow_negative_numbers = true)]
  Browse {
    #[arg(long)]
    chat: Option<i64>,
    /// Who set it off: `console`, or `chat:<chatId>`.
    #[arg(long, allow_hyphen_values = true)]
    by: Option<String>,
    /// The road the turn was read on, which the ending goes back on.
    #[arg(long)]
    via: Option<String>,
    /// `sync` to wait for the run's ending and print it; `async`, where none is named, to answer once it is
    /// launched.
    #[arg(long)]
    mode: Option<String>,
  },
  /// The runs bmo-agency launches.
  Runner {
    #[command(subcommand)]
    run: Runner,
  },
  /// The run of a browse its launcher started.
  #[command(name = "browse-runner", allow_negative_numbers = true)]
  BrowseRunner(BrowseRunner),
  /// Drive a browser on a remote node through a whole list of steps without holding up whoever asked; a secret the
  /// steps name comes in on standard input as NAME=value lines.
  #[command(name = "remote-browser", allow_negative_numbers = true)]
  RemoteBrowser {
    #[arg(long)]
    chat: Option<i64>,
    /// `chat:<chatId>`, who set it off.
    #[arg(long, allow_hyphen_values = true)]
    by: Option<String>,
    #[arg(long)]
    via: Option<String>,
    #[arg(long)]
    mode: Option<String>,
    /// The node, by its name in the node list.
    #[arg(long)]
    node: Option<String>,
    /// The file holding the steps.
    #[arg(long)]
    steps: Option<PathBuf>,
  },
  /// The run of a remote browser its launcher started.
  #[command(name = "remote-browser-runner", allow_negative_numbers = true)]
  RemoteBrowserRunner(RemoteBrowserRunner),
}

#[derive(Subcommand)]
enum Runner {
  /// The run of a browse its launcher started.
  #[command(allow_negative_numbers = true)]
  Browse(BrowseRunner),
  /// The run of a remote browser its launcher started.
  #[command(name = "remote-browser", allow_negative_numbers = true)]
  RemoteBrowser(RemoteBrowserRunner),
}

#[derive(Args)]
struct RemoteBrowserRunner {
  chat_id: i64,
  task_path: PathBuf,
  run_id: Option<i64>,
  unit: Option<String>,
  #[arg(long)]
  via: Option<String>,
  /// The file a sync launch waits on for the run's ending.
  #[arg(long)]
  ending: Option<PathBuf>,
}

#[derive(Args)]
struct BrowseRunner {
  chat_id: i64,
  task_path: PathBuf,
  run_id: Option<i64>,
  unit: Option<String>,
  /// The profile the run launched on, as its launcher handed it over.
  profile: Option<String>,
  /// The registration that set the run off, where it keeps its research.
  #[arg(long)]
  kept_on: Option<String>,
  #[arg(long)]
  via: Option<String>,
  /// The file a sync launch waits on for the run's ending.
  #[arg(long)]
  ending: Option<PathBuf>,
}

fn mode_of(word: Option<&str>) -> Result<Option<Mode>, String> {
  match word {
    None => Ok(None),
    Some("sync") => Ok(Some(Mode::Sync)),
    Some("async") => Ok(Some(Mode::Async)),
    Some(other) => Err(format!("--mode is sync or async, not {other}")),
  }
}

fn settings(data_root: Option<PathBuf>) -> Result<Settings, String> {
  Settings::read(&Settings::data_root(data_root)?)
}

fn variable(name: &str) -> Option<String> {
  std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

async fn carried(cli: Cli) -> Result<Vec<String>, String> {
  match cli.verb {
    Verb::Nouns => Ok(NOUNS.iter().map(|noun| (*noun).to_owned()).collect()),
    Verb::Browse {
      chat,
      by,
      via,
      mode,
    } => {
      let mode = mode_of(mode.as_deref())?;
      let Some(chat_id) = chat else {
        return Err(bmo_explorer::browse::launch::USAGE.to_owned());
      };
      // The task may carry someone's credentials, so it never travels as an argument: arguments are visible in
      // the process list, standard input is not. It is never printed or logged either.
      let mut task = String::new();

      std::io::stdin()
        .read_to_string(&mut task)
        .map_err(|why| format!("the task could not be read: {why}"))?;

      let asked = Asked {
        chat_id,
        task,
        started_by: by,
        asked_by: variable(AGENT_VARIABLE),
        via,
        mode,
        home: variable(HOME_VARIABLE),
      };

      launch(&settings(cli.data_root)?, &asked).await
    }
    Verb::Runner {
      run: Runner::Browse(run),
    }
    | Verb::BrowseRunner(run) => {
      let started = Started {
        chat_id: run.chat_id,
        task_path: run.task_path,
        run_id: run.run_id,
        unit: run.unit.unwrap_or_default(),
        profile: run.profile,
        kept_on: run.kept_on,
        via: run.via,
        ending: run.ending,
        asked_by: variable(AGENT_VARIABLE),
      };

      browse_runner(&settings(cli.data_root)?, &started).await
    }
    Verb::RemoteBrowser {
      chat,
      by,
      via,
      mode,
      node,
      steps,
    } => {
      let refuse = |why: &str| format!("remote-browser: {why}\n{}", remote_browser::USAGE);
      let mode = mode_of(mode.as_deref()).map_err(|why| refuse(&why))?;
      let (Some(chat_id), Some(node), Some(steps)) = (chat, node, steps) else {
        return Err(refuse("--chat, --node and --steps are all needed"));
      };
      let steps = std::fs::read_to_string(&steps)
        .map_err(|_| refuse(&format!("the steps file {} cannot be read", steps.display())))?;
      let asked = remote_browser::Asked {
        chat_id,
        node,
        steps,
        started_by: by,
        asked_by: variable(AGENT_VARIABLE),
        via,
        mode,
      };
      // The secrets the steps name come in on standard input, read only where the steps name one.
      let secrets = || {
        let mut text = String::new();
        let _ = std::io::stdin().read_to_string(&mut text);
        text
      };

      launch_remote_browser(&settings(cli.data_root)?, &asked, secrets).await
    }
    Verb::Runner {
      run: Runner::RemoteBrowser(run),
    }
    | Verb::RemoteBrowserRunner(run) => {
      let started = remote_browser::Started {
        chat_id: run.chat_id,
        task_path: run.task_path,
        run_id: run.run_id,
        unit: run.unit.unwrap_or_default(),
        via: run.via,
        ending: run.ending,
        asked_by: variable(AGENT_VARIABLE),
      };

      remote_browser_runner(&settings(cli.data_root)?, &started).await
    }
  }
}

#[tokio::main]
async fn main() -> ExitCode {
  let cli = Cli::parse();

  match carried(cli).await {
    Ok(lines) => {
      for line in lines {
        println!("{line}");
      }
      ExitCode::SUCCESS
    }
    Err(why) => {
      eprintln!("{why}");
      ExitCode::FAILURE
    }
  }
}
