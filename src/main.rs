//! `bmo-explorer`, the unit command (docs/api.md, 「The unit command」): `mcp` serves the MCP server on standard
//! input and output, and `openapi` prints the contract document. It prints what it did on standard output and
//! why it refused on standard error, and ends 0 where it did what was asked, 1 where it refused, and 2 where its
//! words do not read.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use bmo_explorer::client::Explorer;
use bmo_explorer::mcp::ExplorerServer;
use bmo_explorer::openapi::openapi_json;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
  name = "bmo-explorer",
  version,
  about = "Web reading: the MCP server and the contract document"
)]
struct Cli {
  #[command(subcommand)]
  verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
  /// Serve the MCP server on standard input and output until the harness closes it.
  Mcp {
    /// The data root the store stands under; `BMO_DATA_DIR` where none is named.
    #[arg(long, env = "BMO_DATA_DIR")]
    data_root: PathBuf,
  },
  /// Print the OpenAPI document, as docs/openapi.json holds it.
  Openapi,
  /// Move the old archive's web-reading records from the item's export into the store (TODO #1923).
  Import {
    /// The export directory the item's export hand wrote.
    #[arg(long)]
    export: PathBuf,
    /// The store's SQLite file; made and given its tables where it does not stand.
    #[arg(long)]
    database: PathBuf,
    /// Read the export and report what would be written, writing nothing.
    #[arg(long)]
    dry_run: bool,
  },
}

fn main() -> ExitCode {
  let cli = match Cli::try_parse() {
    Ok(cli) => cli,
    Err(why) => {
      let _ = why.print();

      return ExitCode::from(if why.use_stderr() { 2 } else { 0 });
    }
  };
  let done = match cli.verb {
    Verb::Mcp { data_root } => mcp(data_root),
    Verb::Openapi => {
      print!("{}", openapi_json());
      Ok(())
    }
    Verb::Import {
      export,
      database,
      dry_run,
    } => imported(export, database, dry_run),
  };

  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-explorer: {why}");
      ExitCode::from(1)
    }
  }
}

/// The old archive's web-reading records moved into the store: the report printed, and a failure where the
/// count-and-sample compare found one.
fn imported(export: PathBuf, database: PathBuf, dry_run: bool) -> Result<(), String> {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .map_err(|why| format!("the runtime did not start: {why}"))?;

  runtime.block_on(async move {
    let report = bmo_explorer::import::run(&database, &export, dry_run).await?;
    for line in report.lines() {
      println!("{line}");
    }
    if report.sample.failures.is_empty() {
      Ok(())
    } else {
      Err(format!("{} rows failed to compare", report.sample.failures.len()))
    }
  })
}

/// The MCP server, run until its harness closes standard input, its web reading ended after it.
fn mcp(data_root: PathBuf) -> Result<(), String> {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .map_err(|why| format!("the runtime did not start: {why}"))?;

  runtime.block_on(async move {
    let client = Arc::new(Explorer::open(&data_root));
    let running = rmcp::ServiceExt::serve(ExplorerServer::new(client.clone()), rmcp::transport::stdio())
      .await
      .map_err(|why| format!("the MCP session did not start: {why}"))?;
    let ended = running.waiting().await;

    // A browser the reading started ends with the session rather than outliving it.
    client.close().await;
    ended.map(|_| ()).map_err(|why| format!("the MCP session ended in a failure: {why}"))
  })
}
