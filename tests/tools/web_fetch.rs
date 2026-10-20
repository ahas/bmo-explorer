use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tool::protocol::Sockets;
use bmo_explorer::tool::{Answer, Session};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use bmo_explorer::tools::web_fetch::WebFetch;

type Heard = Arc<Mutex<Vec<Value>>>;

// A socket answering each line it is asked by `answer`, keeping what it heard.
fn stand_in(socket: &Path, answer: fn(&Value) -> Value) -> Heard {
  let heard = Heard::default();
  let kept = heard.clone();
  let listener = UnixListener::bind(socket).unwrap();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      let (reading, mut writing) = stream.into_split();
      let mut line = String::new();

      BufReader::new(reading).read_line(&mut line).await.unwrap();

      let asked: Value = serde_json::from_str(&line).unwrap();
      let said = format!("{}\n", answer(&asked));

      kept.lock().unwrap().push(asked);
      writing.write_all(said.as_bytes()).await.unwrap();
    }
  });

  heard
}

// The host's call-back: gasazip.com claimed by `web/gasazip`, whose call answers the arguments it was handed.
fn host(asked: &Value) -> Value {
  match (asked["kind"].as_str(), asked["address"].as_str()) {
    (Some("claim"), Some(address)) if address.contains("gasazip.com") => {
      json!({ "entry": "web/gasazip", "arguments": { "query": "20501" } })
    }
    (Some("claim"), _) => json!({ "entry": null }),
    _ => {
      let arguments = &asked["arguments"];

      serde_json::to_value(Answer::text(format!(
        "read {} from {}",
        arguments["query"], arguments["offset"]
      )))
      .unwrap()
    }
  }
}

fn helper(asked: &Value) -> Value {
  serde_json::to_value(Answer::text(format!("PAGE {}", asked["url"]))).unwrap()
}

fn session(dir: &Path) -> (Session, Heard, Heard) {
  let callback = dir.join("callback.sock");
  let web = dir.join("web.sock");
  let session = Session {
    holder_name: "search".into(),
    capabilities: ["web.read".to_owned()].into(),
    ..Default::default()
  };
  let host_heard = stand_in(&callback, host);
  let helper_heard = stand_in(&web, helper);

  session.kept.get_or_keep(|| Sockets {
    callback: Some(callback),
    shared: BTreeMap::from([("web".to_owned(), web)]),
  });

  (session, host_heard, helper_heard)
}

async fn fetched(session: &Session, arguments: Value) -> String {
  bmo_explorer::tool::Served::call(&WebFetch, session, arguments).await.unwrap().plain()
}

#[test]
fn the_manifest_declares_the_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/web_fetch"), "web_fetch").unwrap();

  assert_eq!(manifest.differences(bmo_explorer::tools::web_fetch::TOOLS), Vec::<String>::new());
}

// A page a site tool claims is answered by that tool through the host, headed with its name, the slice asked
// for handed on (docs/site-tools.md, 「A page a tool claims」).
#[tokio::test]
async fn a_claimed_page_is_answered_by_its_site_tool_through_the_host_and_headed_with_its_name() {
  let dir = tempfile::tempdir().unwrap();
  let (session, host_heard, helper_heard) = session(dir.path());

  assert_eq!(
    fetched(&session, json!({ "url": "https://gasazip.com/20501", "offset": 40 })).await,
    "ANSWERED-BY: gasazip (the site tool for this address: a searcher calls web/gasazip itself next time)\nread \"20501\" from 40"
  );
  assert_eq!(
    *host_heard.lock().unwrap(),
    [
      json!({ "kind": "claim", "address": "https://gasazip.com/20501" }),
      json!({ "kind": "call", "entry": "web/gasazip", "arguments": { "query": "20501", "offset": 40 } }),
    ]
  );
  assert!(helper_heard.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unclaimed_page_is_read_by_the_web_helper() {
  let dir = tempfile::tempdir().unwrap();
  let (session, _, helper_heard) = session(dir.path());

  assert_eq!(
    fetched(&session, json!({ "url": "https://example.org/a", "max_chars": 500 })).await,
    "PAGE \"https://example.org/a\""
  );
  assert_eq!(
    *helper_heard.lock().unwrap(),
    [json!({ "kind": "fetch", "url": "https://example.org/a", "max_chars": 500, "raw": false })]
  );
}

#[tokio::test]
async fn raw_markup_is_never_claimed() {
  let dir = tempfile::tempdir().unwrap();
  let (session, host_heard, helper_heard) = session(dir.path());

  fetched(&session, json!({ "url": "https://gasazip.com/20501", "raw": true })).await;

  assert!(host_heard.lock().unwrap().is_empty());
  assert_eq!(helper_heard.lock().unwrap()[0]["raw"], true);
}

#[tokio::test]
async fn a_call_handed_no_helper_socket_says_so() {
  let session = Session::default();
  let answer = bmo_explorer::tool::Served::call(
    &WebFetch,
    &session,
    json!({ "url": "https://example.org/", "raw": true }),
  )
  .await
  .unwrap();

  assert!(answer.is_error);
  assert_eq!(answer.plain(), "ERROR: this call was handed no socket of the shared helper `web`");
}
