//! The MCP server's test (docs/api.md, 「The MCP server」): `bmo-explorer mcp` started as a harness starts it,
//! asked over its standard input, lists exactly the six tools and answers a call that fails in the error shape.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// How long the server may take over everything it is asked here.
const BOUND: Duration = Duration::from_secs(30);

#[tokio::test]
async fn the_server_starts_lists_the_six_tools_and_answers_a_refusal_in_the_error_shape() {
  let root = tempfile::tempdir().unwrap();
  let mut server = Command::new(env!("CARGO_BIN_EXE_bmo-explorer"))
    .args(["mcp", "--data-root"])
    .arg(root.path())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
  let mut input = server.stdin.take().unwrap();
  let mut output = BufReader::new(server.stdout.take().unwrap()).lines();
  let asked = [
    json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
      "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "tester", "version": "0" },
    }}),
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
      "name": "explorer_fetch_page", "arguments": { "url": "http://127.0.0.1/" },
    }}),
    json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
      "name": "explorer_web_search", "arguments": { "q": "words" },
    }}),
  ];

  for line in &asked {
    input.write_all(format!("{line}\n").as_bytes()).await.unwrap();
  }

  let answers = tokio::time::timeout(BOUND, async {
    let mut answers: Vec<Value> = Vec::new();

    while answers.len() < 4 {
      let line = output.next_line().await.unwrap().expect("the server answered every request");
      let said: Value = serde_json::from_str(&line).unwrap();

      if said.get("id").is_some() {
        answers.push(said);
      }
    }

    answers.sort_by_key(|said| said["id"].as_i64());
    answers
  })
  .await
  .expect("the server answered within its bound");

  drop(input);

  let ended = tokio::time::timeout(BOUND, server.wait())
    .await
    .expect("the server ended once its input closed");
  let tools: Vec<&str> = answers[1]["result"]["tools"]
    .as_array()
    .unwrap()
    .iter()
    .map(|tool| tool["name"].as_str().unwrap())
    .collect();
  let refusal = |said: &Value| -> Value {
    assert_eq!(said["result"]["isError"], true, "{said}");
    serde_json::from_str(said["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
  };

  assert_eq!(answers[0]["result"]["serverInfo"]["version"], bmo_explorer::VERSION);
  assert_eq!(
    tools,
    [
      "explorer_web_search",
      "explorer_web_fetch",
      "explorer_link_preview",
      "explorer_fetch_page",
      "explorer_fetch_raster",
      "explorer_search_engines",
    ]
  );
  assert_eq!(refusal(&answers[2])["code"], "unsafe-address");
  assert_eq!(refusal(&answers[3])["code"], "bad-request");
  assert!(ended.unwrap().success());
}
