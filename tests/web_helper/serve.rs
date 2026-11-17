use std::collections::BTreeMap;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bmo_explorer::site::contract::{Arrival, Reach};
use bmo_explorer::site::helper::{HelperReach, WebAsked};
use bmo_explorer::tool::protocol::{Sockets, ask_line};
use bmo_explorer::tool::{Answer, Session};
use bmo_explorer::web::address::{Check, Lookup};
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::session::{SessionParts, WebSession};
use bmo_explorer::web::{Failure, WebError};
use bmo_explorer::web_helper::run::WebRun;
use tokio::net::UnixListener;

use bmo_explorer::web_helper::run::{BUDGET_SPENT, Research, SEARCH_STOPPED};
use bmo_explorer::web_helper::serve::*;

fn run() -> Arc<WebRun> {
  run_of(Research::default())
}

fn run_of(research: Research) -> Arc<WebRun> {
  run_checked(research, Check::default())
}

fn run_checked(research: Research, check: Check) -> Arc<WebRun> {
  Arc::new(WebRun::with(
    research,
    WebSession::new(SessionParts {
      store: Arc::new(MemoryPacing::default()),
      transport: bmo_explorer::web::client::network(),
      check,
      host_interval_ms: 0,
      browsers: None,
    }),
  ))
}

// A resolver that never answers, and says when the lookup it was asked for has been let go of.
struct Silent(Arc<AtomicBool>);

struct LetGo(Arc<AtomicBool>);

impl Drop for LetGo {
  fn drop(&mut self) {
    self.0.store(true, Ordering::SeqCst);
  }
}

impl Lookup for Silent {
  fn lookup<'a>(
    &'a self,
    _: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    let let_go = LetGo(self.0.clone());

    Box::pin(async move {
      let _held = let_go;

      std::future::pending().await
    })
  }
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
  let (_dir, socket) = served(run()).await;
  let fetch = WebAsked::Fetch {
    url: "http://localhost/".into(),
    offset: None,
    max_chars: Some(100),
    raw: false,
  };
  let refused: Answer = ask_line(&socket, &fetch).await.unwrap();

  assert!(refused.is_error);
  assert!(refused.plain().starts_with("ERROR: Private web address is not allowed: localhost"));
}

// A read is waited for until it ends, and an asker that ends its connection first lets go of it: the helper drops
// the read with what it set off (docs/web-access.md, 「A read its caller lets go of」).
#[tokio::test]
async fn a_read_whose_asker_goes_away_is_dropped_with_what_it_set_off() {
  let let_go = Arc::new(AtomicBool::new(false));
  let check = Check {
    lookup: Arc::new(Silent(let_go.clone())),
    ..Check::default()
  };
  let (_dir, socket) = served(run_checked(Research::default(), check)).await;
  let mut asking = tokio::net::UnixStream::connect(&socket).await.unwrap();
  let fetch = WebAsked::Fetch {
    url: "https://silent.example/".into(),
    offset: None,
    max_chars: None,
    raw: false,
  };

  tokio::io::AsyncWriteExt::write_all(
    &mut asking,
    format!("{}\n", serde_json::to_string(&fetch).unwrap()).as_bytes(),
  )
  .await
  .unwrap();

  // The site's name is never answered: the read waits, held by nothing but its asker.
  for _ in 0..20 {
    tokio::task::yield_now().await;
  }
  assert!(!let_go.load(Ordering::SeqCst));

  drop(asking);

  // The test's own bound on how long it watches for the helper to let go.
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !let_go.load(Ordering::SeqCst) {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn a_line_that_does_not_read_is_answered_as_a_failure() {
  let (_dir, socket) = served(run()).await;
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

// A site tool's call is held to the search refusals where it reads through the engines (docs/site-contract.md,
// 「What a tool is given」).
#[tokio::test]
async fn a_site_tool_s_call_is_refused_by_the_guard_the_helper_keeps() {
  let stopped = run();
  let unavailable = "QUERY: q\nSEARCH UNAVAILABLE: engines refused or failed.";

  stopped.guard.note_search(unavailable);
  stopped.guard.note_search(unavailable);

  let (_stopped_dir, stopped) = served(stopped).await;
  let (_open_dir, open) = served(run()).await;

  assert_eq!(reach(&stopped).opening(true).await, Ok(Some(SEARCH_STOPPED.to_owned())));
  assert_eq!(reach(&open).opening(true).await, Ok(None));
}

#[tokio::test]
async fn a_site_tool_s_outcome_is_answered_as_the_reading_answers_it() {
  let (_dir, socket) = served(run()).await;
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

  let forcing = WebAsked::Captcha {
    url: "https://gate.example/asked".into(),
    offset: Some(40),
    max_chars: None,
  };
  let written = serde_json::to_value(&forcing).unwrap();

  assert_eq!(written["kind"], "captcha");
  assert_eq!(serde_json::from_value::<WebAsked>(written).unwrap(), forcing);
}

// A record whose browser stands in, for the reads the browser answers.
fn run_browsers(research: Research, browser: Arc<crate::web::testing::FakeBrowser>) -> Arc<WebRun> {
  use bmo_explorer::web::browser::BrowserFetcher;
  use bmo_explorer::web::clock::StandingClock;

  let made: Arc<dyn BrowserFetcher> = browser;

  Arc::new(WebRun::with(
    research,
    WebSession::new(SessionParts {
      store: Arc::new(MemoryPacing::default()),
      transport: Arc::new(crate::web::testing::Web::default()),
      check: crate::web::testing::check(StandingClock::at(1_000_000)),
      host_interval_ms: 0,
      browsers: Some(Arc::new(move |_| made.clone())),
    }),
  ))
}

fn forcing_page(
  text: &str,
  challenge: bmo_explorer::web::browser::ChallengeMark,
) -> bmo_explorer::web::browser::BrowserDocument {
  bmo_explorer::web::browser::BrowserDocument {
    url: "https://fixture.test/post".into(),
    status: 200,
    content_type: "text/html".into(),
    title: "Post".into(),
    text: text.into(),
    html: format!("<html><body><p>{text}</p></body></html>"),
    links: Vec::new(),
    user_agent: "Chrome".into(),
    webdriver: false,
    truncated: false,
    challenge,
  }
}

fn gate_page() -> bmo_explorer::web::browser::BrowserDocument {
  bmo_explorer::web::browser::BrowserDocument {
    status: 403,
    title: "Just a moment...".into(),
    text: "Verify you are human".into(),
    html: "<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div></body></html>".into(),
    ..forcing_page("", bmo_explorer::web::browser::ChallengeMark::Remains)
  }
}

// The forcing read: the browser answers the record's web helper, and the answer carries the attempt's line
// before the page (docs/api.md, 「The captcha read」).
#[tokio::test]
async fn a_captcha_request_is_answered_with_the_attempt_line_before_the_page() {
  use bmo_explorer::web::browser::ChallengeMark;

  let browser = crate::web::testing::FakeBrowser::answering([forcing_page(
    "the page behind the challenge",
    ChallengeMark::Cleared,
  )]);
  let (_dir, socket) = served(run_browsers(Research::default(), browser.clone())).await;
  let answer: Answer = ask_line(
    &socket,
    &WebAsked::Captcha {
      url: "https://fixture.test/post".into(),
      offset: None,
      max_chars: None,
    },
  )
  .await
  .unwrap();
  let said = answer.plain();

  assert!(!answer.is_error);
  assert!(said.starts_with("CHALLENGE CLEARED\n"), "{said}");
  assert!(said.contains("the page behind the challenge"));
  let asked = browser.asked();
  assert_eq!(asked.len(), 1, "the forcing read reads the page once");
  assert!(asked[0].solve, "and asks the browser for the attempt");
}

#[tokio::test]
async fn a_forcing_read_that_met_nothing_is_headed_as_it_says() {
  use bmo_explorer::web::browser::ChallengeMark;

  let browser =
    crate::web::testing::FakeBrowser::answering([forcing_page("plain page", ChallengeMark::NotAttempted)]);
  let (_dir, socket) = served(run_browsers(Research::default(), browser)).await;
  let answer: Answer = ask_line(
    &socket,
    &WebAsked::Captcha {
      url: "https://fixture.test/post".into(),
      offset: None,
      max_chars: None,
    },
  )
  .await
  .unwrap();

  assert!(answer.plain().starts_with("NO CHALLENGE\n"), "{}", answer.plain());
}

// A challenge that stands after the one attempt is the refusal it is: named, said to have been attempted,
// and final (docs/web-access.md, 「When the browser meets a challenge」).
#[tokio::test]
async fn a_captcha_request_whose_challenge_stands_is_refused_once_and_says_it_was_attempted() {
  let browser = crate::web::testing::FakeBrowser::answering([gate_page()]);
  let (_dir, socket) = served(run_browsers(Research::default(), browser.clone())).await;
  let answer: Answer = ask_line(
    &socket,
    &WebAsked::Captcha {
      url: "https://fixture.test/post".into(),
      offset: None,
      max_chars: None,
    },
  )
  .await
  .unwrap();

  assert!(answer.is_error);
  assert!(answer.plain().contains("attempted once and did not clear"), "{}", answer.plain());
  assert_eq!(browser.calls(), 1, "the helper does not send the read again");
}

// The forcing read takes a page of the record's budget like a fetch of that address, and the spent budget
// refuses it before the browser is opened.
#[tokio::test]
async fn the_page_budget_holds_a_captcha_request_as_it_holds_a_fetch() {
  let browser = crate::web::testing::FakeBrowser::answering([gate_page()]);
  let spent = run_browsers(
    Research {
      pages: Some(1),
      ..Research::default()
    },
    browser.clone(),
  );

  spent.guard.take_page(Some("https://other.test/"));

  let (_dir, socket) = served(spent).await;
  let answer: Answer = ask_line(
    &socket,
    &WebAsked::Captcha {
      url: "https://fixture.test/post".into(),
      offset: None,
      max_chars: None,
    },
  )
  .await
  .unwrap();

  assert_eq!(answer.plain(), BUDGET_SPENT);
  assert_eq!(browser.calls(), 0);
}

// A record that reads the web directly only is refused the forcing read naming the strategy, as the `rendered`
// road refuses it.
#[tokio::test]
async fn a_captcha_request_of_a_direct_only_record_is_refused_naming_the_strategy() {
  let browser = crate::web::testing::FakeBrowser::answering([gate_page()]);
  let (_dir, socket) = served(run_browsers(
    Research {
      strategy: Some("direct-only".into()),
      ..Research::default()
    },
    browser.clone(),
  ))
  .await;
  let answer: Answer = ask_line(
    &socket,
    &WebAsked::Captcha {
      url: "https://fixture.test/post".into(),
      offset: None,
      max_chars: None,
    },
  )
  .await
  .unwrap();

  assert!(answer.is_error);
  assert!(answer.plain().contains("reads the web directly only"), "{}", answer.plain());
  assert_eq!(browser.calls(), 0);
}

// The page budget holds a site tool's call and a fetch alike (docs/web-access.md, 「Settings from the session
// record」): a tool reading no engine takes a page as it opens, a fetch past the budget is refused before any site is
// asked, and a tool reading through the engines takes none.
#[tokio::test]
async fn the_page_budget_holds_site_tools_and_fetches_alike() {
  let (_dir, socket) = served(run_of(Research {
    pages: Some(1),
    ..Research::default()
  }))
  .await;
  let fetch = WebAsked::Fetch {
    url: "https://example.org/".into(),
    offset: None,
    max_chars: Some(100),
    raw: false,
  };

  assert_eq!(reach(&socket).opening(false).await, Ok(None));
  assert_eq!(reach(&socket).opening(true).await, Ok(None));
  assert_eq!(reach(&socket).opening(false).await, Ok(Some(BUDGET_SPENT.to_owned())));

  let spent: Answer = ask_line(&socket, &fetch).await.unwrap();

  assert_eq!(spent.plain(), BUDGET_SPENT);
}
