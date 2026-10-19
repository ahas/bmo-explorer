//! The web helper's executable: started by the tool host for one record, handed its start on standard input,
//! and serving the record's web tools on the socket it names until it is asked to end (the tool host's
//! docs/mcp-host.md, 「Shared helpers」).

use bmo_explorer::tool::protocol::{Asked, Handed};

#[tokio::main]
async fn main() -> std::process::ExitCode {
  let handed: Handed = match serde_json::from_reader(std::io::stdin().lock()) {
    Ok(handed) => handed,
    Err(why) => {
      eprintln!("web helper: its start does not read: {why}");
      return std::process::ExitCode::FAILURE;
    }
  };
  let Asked::Helper { socket } = &handed.asked else {
    eprintln!("web helper: it was asked something other than a helper's start");
    return std::process::ExitCode::FAILURE;
  };

  match bmo_explorer::web_helper::serve::serve(&handed.session, socket).await {
    Ok(()) => std::process::ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("web helper: {why}");
      std::process::ExitCode::FAILURE
    }
  }
}
