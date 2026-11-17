use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bmo_explorer::tool::Session;
use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tool::protocol::Sockets;
use bmo_explorer::tool::{Answer, Served};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

use bmo_explorer::tools::web_captcha::WebCaptcha;

type Heard = Arc<Mutex<Vec<Value>>>;

// The helper's socket: answers each line with what it was told to say, keeping what it heard.
fn helper(socket: &Path, said: fn(&Value) -> Value) -> Heard {
  let heard = Heard::default();
  let kept = heard.clone();
  let listener = UnixListener::bind(socket).unwrap();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      let (reading, mut writing) = stream.into_split();
      let mut line = String::new();

      BufReader::new(reading).read_line(&mut line).await.unwrap();

      let asked: Value = serde_json::from_str(&line).unwrap();

      kept.lock().unwrap().push(asked.clone());
      writing.write_all(format!("{}\n", said(&asked)).as_bytes()).await.unwrap();
    }
  });

  heard
}

fn session(socket: &Path) -> Session {
  let session = Session {
    holder_name: "search".into(),
    capabilities: ["web.read".to_owned()].into(),
    ..Default::default()
  };

  session.kept.get_or_keep(|| Sockets {
    callback: None,
    shared: BTreeMap::from([("web".to_owned(), socket.to_path_buf())]),
  });

  session
}

async fn called(session: &Session, arguments: Value) -> Answer {
  WebCaptcha.call(session, arguments).await.unwrap()
}

#[test]
fn the_manifest_declares_the_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/web_captcha"), "web_captcha")
      .unwrap();

  assert_eq!(manifest.differences(bmo_explorer::tools::web_captcha::TOOLS), Vec::<String>::new());
}

// The entry asks the record's web helper for the forcing read and answers with what the helper said, the
// attempt's line and all (docs/api.md, 「The captcha read」).
#[tokio::test]
async fn the_call_asks_the_helper_for_the_forcing_read_and_answers_with_its_line() {
  let dir = tempfile::tempdir().unwrap();
  let socket = dir.path().join("web.sock");
  let heard = helper(&socket, |asked| {
    serde_json::to_value(Answer::text(format!(
      "CHALLENGE CLEARED\nPAGE {}",
      asked["url"].as_str().unwrap_or_default()
    )))
    .unwrap()
  });
  let session = session(&socket);
  let answer = called(&session, json!({ "url": "https://gate.example/asked", "max_chars": 500 })).await;

  assert!(!answer.is_error);
  assert_eq!(answer.plain(), "CHALLENGE CLEARED\nPAGE https://gate.example/asked");
  assert_eq!(
    *heard.lock().unwrap(),
    [json!({ "kind": "captcha", "url": "https://gate.example/asked", "max_chars": 500 })]
  );
}

// The helper's refusal of a standing challenge reaches the caller as the refusal it is.
#[tokio::test]
async fn a_refused_challenge_is_answered_as_a_refusal() {
  let dir = tempfile::tempdir().unwrap();
  let socket = dir.path().join("web.sock");
  helper(&socket, |_| {
    serde_json::to_value(Answer::failure(
      "ERROR: The real browser still reached CAPTCHA or browser challenge (HTTP 403) at \
       https://gate.example/asked; the challenge was attempted once and did not clear",
    ))
    .unwrap()
  });
  let session = session(&socket);
  let answer = called(&session, json!({ "url": "https://gate.example/asked" })).await;

  assert!(answer.is_error);
  assert!(answer.plain().contains("attempted once and did not clear"));
}

#[tokio::test]
async fn a_call_handed_no_helper_socket_says_so() {
  let answer = called(&Session::default(), json!({ "url": "https://gate.example/asked" })).await;

  assert!(answer.is_error);
  assert_eq!(answer.plain(), "ERROR: this call was handed no socket of the shared helper `web`");
}
