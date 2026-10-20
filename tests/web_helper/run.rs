use bmo_explorer::tool::Session;
use bmo_explorer::web::clock::StandingClock;

use bmo_explorer::web_helper::run::*;

// Carried from bot/bot/research-guard.test.ts: refuses new web work once research has closed and gives requests the close as their deadline
#[test]
fn refuses_new_web_work_once_research_has_closed() {
  let clock = StandingClock::at(1_000);
  let guard = ResearchGuard::new(Some(5_000), clock.clone());

  assert_eq!(guard.closed(), None);
  assert_eq!(guard.deadline(), Some(5_000));
  clock.advance(4_000);
  assert_eq!(guard.closed(), Some(RESEARCH_CLOSED));
}

// Carried from bot/bot/research-guard.test.ts: a server outside a research run has no close
#[test]
fn a_server_outside_a_research_run_has_no_close() {
  let guard = ResearchGuard::new(None, StandingClock::at(1_000));

  assert_eq!(guard.closed(), None);
  assert_eq!(guard.deadline(), None);
}

// Carried from bot/bot/research-guard.test.ts: stops asking the engines after two unavailable searches in a row, and not after one
#[test]
fn stops_asking_the_engines_after_two_unavailable_searches_in_a_row() {
  let guard = ResearchGuard::new(None, StandingClock::at(1_000));
  let unavailable = "QUERY: q\nSEARCH UNAVAILABLE: engines refused or failed — naver (CAPTCHA).";

  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), None);
  guard.note_search("QUERY: q\n1. A");
  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), None);
  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), Some(SEARCH_STOPPED));
}

// What a run is told travels in its session record.
#[test]
fn what_a_run_is_told_is_read_off_its_record() {
  // A run's settings travel in its session record, written by the launcher and read here.
  let session: Session = serde_json::from_value(serde_json::json!({
    "line": "search",
    "capabilities": ["web.read"],
    "dataRoot": "/nowhere",
    "archive": "/nowhere/bmo.db",
    "research": {
      "closesAt": 123,
      "owner": "search-run:7",
      "source": "search",
      "role": "scout",
      "branches": 4,
      "chat": -5,
      "strategy": "browser-first",
      "hostIntervalMs": 1_000,
      "engineIntervalMs": 2_000,
      "rulebooks": ["product-search", "restaurant-search"],
    },
  }))
  .unwrap();

  assert_eq!(
    Research::of(&session),
    Research {
      closes_at: Some(123),
      strategy: Some("browser-first".into()),
      host_interval_ms: Some(1_000),
      engine_interval_ms: Some(2_000),
    }
  );

  let bare: Session = serde_json::from_value(
    serde_json::json!({ "line": "chat", "capabilities": [], "dataRoot": "/", "archive": "/bmo.db" }),
  )
  .unwrap();

  assert_eq!(Research::of(&bare), Research::default());
}
