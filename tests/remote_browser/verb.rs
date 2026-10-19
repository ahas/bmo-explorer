//! `bmo-explorer remote-browser` and its runner as a shell and bmo-agency run them (docs/remote-browser.md, 「The
//! launch」): every refusal made before a row or a run exists, a launch whose run never came up closing its own row
//! and taking the task file with it, and a runner whose node is gone telling the agent so.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::stand_in::{Answers, Asked, data_root, serve};

const BY: &str = "chat:-100";

fn explorer(data_root: &Path, args: &[&str], stdin: &str) -> (i32, String, String) {
  let mut child = Command::new(env!("CARGO_BIN_EXE_bmo-explorer"))
    .args(args)
    .env("BMO_DATA_DIR", data_root)
    .env("BMO_TURN_AGENT", "asker")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();

  let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());

  let done = child.wait_with_output().unwrap();

  (
    done.status.code().unwrap_or(-1),
    String::from_utf8_lossy(&done.stdout).into_owned(),
    String::from_utf8_lossy(&done.stderr).into_owned(),
  )
}

// The node list a remote node and an ssh node stand in, and bmo-agency refusing every launch.
fn answers(listed: bool) -> Answers {
  Arc::new(move |asked: &Asked| match (asked.method.as_str(), asked.path.as_str()) {
    ("GET", "/nodes") if listed => (
      StatusCode::OK,
      json!({ "nodes": [
        { "name": "agent-a", "label": "agent_a", "kind": "remote", "state": "up" },
        { "name": "pc-node", "label": "pc", "kind": "ssh", "state": "up" },
      ] }),
    ),
    ("GET", "/nodes") => (StatusCode::OK, json!({ "nodes": [] })),
    ("GET", "/nodes/agent-a/target") => (
      StatusCode::OK,
      json!({ "kind": "remote", "hubUrl": "http://127.0.0.1:9", "tokenFile": "/nowhere/hub.env", "agent": "agent_a" }),
    ),
    ("POST", "/runs") => (StatusCode::CREATED, json!({ "id": 7 })),
    ("POST", "/runs/7/launch") => (
      StatusCode::SERVICE_UNAVAILABLE,
      json!({ "error": "the host did not start it", "code": "launch-failed" }),
    ),
    ("POST", "/instructions") => (StatusCode::CREATED, json!({ "id": 3 })),
    _ => (StatusCode::NO_CONTENT, Value::Null),
  })
}

fn args<'a>(node: &'a str, steps: &'a str) -> Vec<&'a str> {
  vec!["remote-browser", "--chat", "-100", "--by", BY, "--node", node, "--steps", steps]
}

// Refuses a node that is not there, one that is not a remote node, steps that do not parse and a secret stdin did
// not give, and opens no row for any.
#[tokio::test(flavor = "multi_thread")]
async fn refuses_what_it_cannot_drive_and_opens_no_row_for_any() {
  let stand_in = serve(answers(true)).await;
  let root = data_root(&stand_in.address);
  let steps = root.path().join("steps.json");
  let broken = root.path().join("broken.json");

  std::fs::write(
    &steps,
    r##"[{"do":"page","new":"https://gw.example"},{"do":"type","selector":"#pw","secret":"GW_PASSWORD"}]"##,
  )
  .unwrap();
  std::fs::write(&broken, r#"[{"do":"fly"}]"#).unwrap();

  let steps = steps.display().to_string();
  let broken = broken.display().to_string();
  let refusals = [
    (args("nowhere", &steps), "GW_PASSWORD=x\n", "there is no node called nowhere"),
    (args("pc-node", &steps), "GW_PASSWORD=x\n", "pc-node is not a remote node"),
    (args("agent-a", &broken), "", r#"step 1: "do" must be one of"#),
    (
      args("agent-a", &steps),
      "OTHER=x\n",
      "the steps name GW_PASSWORD, and standard input gave no value for it",
    ),
  ];

  for (args, stdin, reason) in refusals {
    let (code, _, err) = explorer(root.path(), &args, stdin);

    assert_eq!(code, 1, "{err}");
    assert!(err.contains(reason), "{err}");
  }

  assert!(stand_in.calls("POST", "/runs").is_empty());
}

// Closes its own row and takes the task file with it when the run never came up.
#[tokio::test(flavor = "multi_thread")]
async fn closes_its_own_row_and_takes_the_task_file_with_it_when_the_run_never_came_up() {
  let stand_in = serve(answers(true)).await;
  let root = data_root(&stand_in.address);
  let steps = root.path().join("steps.json");

  std::fs::write(&steps, r##"[{"do":"type","selector":"#pw","secret":"GW_PASSWORD"}]"##).unwrap();

  let (code, out, err) =
    explorer(root.path(), &args("agent-a", &steps.display().to_string()), "GW_PASSWORD=s3cret\n");
  let opened = stand_in.calls("POST", "/runs");

  assert_eq!(code, 1, "{err}");
  assert!(err.contains("did not come up: bmo-agency refused it"), "{err}");
  assert_eq!(opened.len(), 1);

  let unit = opened[0].body["unit"].as_str().unwrap().to_owned();

  assert_eq!(opened[0].body["agent"], "remote-browser");
  assert!(unit.starts_with("bmo-remote-browser-"), "{unit}");
  assert_eq!(opened[0].body["cap_ms"], 1_200_000);
  assert_eq!(stand_in.calls("POST", "/runs/7/close")[0].body["outcome"], "error");
  assert_eq!(
    stand_in.calls("POST", "/runs/7/launch")[0].body["title"],
    "remote browser for chat -100 on agent_a"
  );
  // A secret never travels as an argument, and nothing prints it.
  assert!(!stand_in.calls("POST", "/runs/7/launch")[0].body.to_string().contains("s3cret"));
  assert!(!out.contains("s3cret") && !err.contains("s3cret"));

  let left: Vec<String> = std::fs::read_dir(std::env::temp_dir())
    .unwrap()
    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
    .filter(|name| name.starts_with(&unit))
    .collect();

  assert!(left.is_empty(), "{left:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_runner_whose_node_left_the_list_tells_the_agent_it_failed_and_closes_its_row() {
  let stand_in = serve(answers(false)).await;
  let root = data_root(&stand_in.address);
  let task = root.path().join("task.json");

  std::fs::write(
    &task,
    json!({ "node": "agent-a", "plan": { "port": 9222, "profile": "p", "steps": [{ "do": "quit" }] }, "secrets": {} })
      .to_string(),
  )
  .unwrap();

  let (code, _, err) = explorer(
    root.path(),
    &[
      "runner",
      "remote-browser",
      "-100",
      &task.display().to_string(),
      "7",
      "bmo-remote-browser-1",
    ],
    "",
  );

  assert_eq!(code, 1, "{err}");
  assert!(!task.exists(), "the task is taken off the disk the moment it is read");
  assert_eq!(stand_in.calls("POST", "/runs/7/close")[0].body["outcome"], "failed");

  let back = &stand_in.calls("POST", "/instructions")[0].body;

  assert_eq!((back["agent"].as_str(), back["sort"].as_str()), (Some("asker"), Some("remote_browser")));
  assert!(
    back["said"]["order"].as_str().unwrap().contains("no longer a remote node in the list"),
    "{back}"
  );
}
