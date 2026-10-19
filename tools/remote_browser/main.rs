//! The `remote_browser` tool's executable: one call handed by the tool host on standard input, answered on
//! standard output (the tool host's docs/mcp-host.md, 「A call」).

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
  bmo_explorer::tool::protocol::answer_standard_io(bmo_explorer::tools::remote_browser::TOOLS).await
}
