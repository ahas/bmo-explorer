use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tool::protocol::Sockets;
use bmo_explorer::tool::{Answer, Served, Session};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use bmo_explorer::tools::research::Research;

// The record's web helper as a socket that answers every line with one text, keeping what it heard.
fn helper(socket: &Path) -> Arc<Mutex<Vec<Value>>> {
  let heard = Arc::new(Mutex::new(Vec::new()));
  let kept = heard.clone();
  let listener = UnixListener::bind(socket).unwrap();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      let (reading, mut writing) = stream.into_split();
      let mut line = String::new();

      BufReader::new(reading).read_line(&mut line).await.unwrap();
      kept.lock().unwrap().push(serde_json::from_str(&line).unwrap());

      let said = format!("{}\n", serde_json::to_value(Answer::text("BRANCH 1 REPORT")).unwrap());

      writing.write_all(said.as_bytes()).await.unwrap();
    }
  });

  heard
}

#[test]
fn the_manifest_declares_the_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/research"), "research").unwrap();

  assert_eq!(manifest.differences(bmo_explorer::tools::research::TOOLS), Vec::<String>::new());
}

#[tokio::test]
async fn a_call_asks_the_record_s_web_helper_for_one_branch_over_its_angle() {
  let dir = tempfile::tempdir().unwrap();
  let socket = dir.path().join("web.sock");
  let heard = helper(&socket);
  let session = Session {
    holder_name: "search".into(),
    capabilities: ["web.branches".to_owned()].into(),
    ..Default::default()
  };

  session.kept.get_or_keep(|| Sockets {
    callback: None,
    shared: BTreeMap::from([("web".to_owned(), socket)]),
  });

  let answer = Research.call(&session, json!({ "angle": "what to find out" })).await.unwrap();

  assert_eq!(answer.plain(), "BRANCH 1 REPORT");
  assert_eq!(*heard.lock().unwrap(), [json!({ "kind": "branch", "angle": "what to find out" })]);
  assert!(Research.call(&session, json!({ "angle": "" })).await.is_err());
  assert!(Research.call(&session, json!({})).await.is_err());
  assert_eq!(heard.lock().unwrap().len(), 1);
}
