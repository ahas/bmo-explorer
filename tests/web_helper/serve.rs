use std::collections::BTreeMap;
use std::sync::Arc;

use bmo_explorer::site::contract::{Arrival, Reach};
use bmo_explorer::site::helper::{HelperReach, WebAsked};
use bmo_explorer::tool::protocol::{Sockets, ask_line};
use bmo_explorer::tool::{Answer, Session};
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::session::{SessionParts, WebSession};
use bmo_explorer::web::{Failure, WebError};
use bmo_explorer::web_helper::run::WebRun;
use tokio::net::UnixListener;

use bmo_explorer::web_helper::run::{RESEARCH_CLOSED, Research};
use bmo_explorer::web_helper::serve::*;

fn run(closes_at: Option<u64>) -> Arc<WebRun> {
  run_of(Research {
    closes_at,
    ..Research::default()
  })
}

fn run_of(research: Research) -> Arc<WebRun> {
  let clock = StandingClock::at(1_000);

  Arc::new(WebRun::with(
    research,
    WebSession::new(SessionParts {
      store: Arc::new(MemoryPacing::default()),
      transport: bmo_explorer::web::client::network(),
      check: bmo_explorer::web::address::Check::default(),
      host_interval_ms: 0,
      browsers: None,
    }),
    clock,
  ))
}

// A socket answering each connection out of the reading, as the helper serves it.
async fn served(run: Arc<WebRun>) -> (tempfile::TempDir, std::path::PathBuf) {
  let dir = tempfile::tempdir().unwrap();
  let socket = dir.path().join("web.sock");
  let listener = UnixListener::bind(&socket).unwrap();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      tokio::spawn(one(run.clone(), stream));
    }
  });

  (dir, socket)
}

#[tokio::test]
async fn a_request_is_answered_on_its_line_out_of_the_record_s_reading() {
  let (_dir, socket) = served(run(Some(500))).await;
  let fetch = WebAsked::Fetch {
    url: "https://example.org/".into(),
    offset: None,
    max_chars: Some(100),
    raw: false,
  };
  let closed: Answer = ask_line(&socket, &fetch).await.unwrap();

  assert_eq!(closed.plain(), RESEARCH_CLOSED);
}

#[tokio::test]
async fn a_line_that_does_not_read_is_answered_as_a_failure() {
  let (_dir, socket) = served(run(None)).await;
  let answer: Answer = ask_line(&socket, &serde_json::json!({ "kind": "nothing" })).await.unwrap();

  assert!(answer.is_error);
  assert!(answer.plain().starts_with("ERROR: the web helper was asked what it does not read"));
}

// A placed site tool's reach, handed the socket as the host hands a call its helper.
fn reach(socket: &std::path::Path) -> HelperReach {
  let session = Session::default();

  session.kept.get_or_keep(|| Sockets {
    callback: None,
    shared: BTreeMap::from([("web".to_owned(), socket.to_path_buf())]),
  });

  HelperReach::of(&session)
}

// A site tool's call is held to the research close before it reads, and to the search refusals where it
// reads through the engines (docs/site-contract.md, 「What a tool is given」).
#[tokio::test]
async fn a_site_tool_s_call_is_refused_by_the_guard_the_helper_keeps() {
  let (_closed_dir, closed) = served(run(Some(500))).await;
  let (_open_dir, open) = served(run(None)).await;

  assert_eq!(reach(&closed).opening(false).await, Ok(Some(RESEARCH_CLOSED.to_owned())));
  assert_eq!(reach(&open).opening(true).await, Ok(None));
}

#[tokio::test]
async fn a_site_tool_s_outcome_is_answered_as_the_reading_answers_it() {
  let (_dir, socket) = served(run(None)).await;
  let reach = reach(&socket);
  let read = reach.ended(true, Ok("hits".into())).await;
  let failed = reach
    .ended(false, Err(WebError::new("example.org answered HTTP 503", Failure::Http)))
    .await;

  assert_eq!((read.is_error, read.plain()), (false, "hits".to_owned()));
  assert_eq!(
    (failed.is_error, failed.plain()),
    (true, "ERROR: example.org answered HTTP 503".to_owned())
  );
}

// A road's failure crosses the socket as the failure it is, which the tool reads as it would read it in-process.
#[tokio::test]
async fn a_road_s_failure_reaches_the_tool_as_the_failure_it_met() {
  let (_dir, socket) = served(run_of(Research {
    strategy: Some("direct-only".into()),
    ..Research::default()
  }))
  .await;
  let rendered = reach(&socket)
    .rendered("https://example.org/a", false, Some(Arrival::default()))
    .await
    .unwrap_err();
  let head = reach(&socket).head("not an address").await.unwrap_err();

  assert_eq!(rendered.kind, Failure::Blocked);
  assert_eq!(
    rendered.message,
    "example.org is served only to a real browser, and this run reads the web directly only"
  );
  // An address that does not read is refused by the reading's address check before anything is sent.
  assert_eq!(head.kind, Failure::UnsafeAddress);
}

#[tokio::test]
async fn a_road_asked_of_no_helper_fails_as_an_unavailable_program() {
  let dir = tempfile::tempdir().unwrap();
  let failed = reach(&dir.path().join("gone.sock")).head("https://example.org/").await.unwrap_err();

  assert_eq!(failed.kind, Failure::BrowserUnavailable);
}

#[test]
fn what_a_tool_asks_reads_as_the_helper_reads_it() {
  let asked = WebAsked::Search {
    query: "q".into(),
    count: Some(10),
    language: None,
    category: Some("news".into()),
  };
  let written = serde_json::to_value(&asked).unwrap();

  assert_eq!(
    written,
    serde_json::json!({ "kind": "search", "query": "q", "count": 10, "category": "news" })
  );
  assert_eq!(serde_json::from_value::<WebAsked>(written).unwrap(), asked);
}
