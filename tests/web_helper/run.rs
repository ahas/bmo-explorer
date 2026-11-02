use bmo_explorer::tool::Session;

use bmo_explorer::web_helper::run::*;

// Carried from bot/bot/research-guard.test.ts: stops asking the engines after two unavailable searches in a row, and not after one
#[test]
fn stops_asking_the_engines_after_two_unavailable_searches_in_a_row() {
  let guard = ResearchGuard::new(None);
  let unavailable = "QUERY: q\nSEARCH UNAVAILABLE: engines refused or failed — naver (CAPTCHA).";

  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), None);
  guard.note_search("QUERY: q\n1. A");
  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), None);
  guard.note_search(unavailable);
  assert_eq!(guard.search_stopped(), Some(SEARCH_STOPPED));
}

// What a research run is held to travels in its session record, under its issuer's label.
#[test]
fn what_a_run_is_held_to_is_read_off_its_record_s_label() {
  let record = |labels: serde_json::Value| -> Session {
    serde_json::from_value(serde_json::json!({
      "holderName": "search", "namespaces": ["web"], "capabilities": ["web.read"], "grants": [], "createdAt": 1,
      "labels": labels,
    }))
    .unwrap()
  };
  // The issuer's other keys stand beside the ones read here and are left unread, a close of the issuer's own
  // among them: nothing here reads a clock out of the label.
  let session = record(serde_json::json!({
    "research": {
      "closesAt": 123,
      "branches": 4,
      "strategy": "browser-first",
      "hostIntervalMs": 1_000,
      "engineIntervalMs": 2_000,
      "pages": 8,
      "branchPages": 4,
      "run": 7,
      "role": "researcher",
      "source": "search",
      "chat": -5,
      "branchesAt": "http://127.0.0.1:1/",
      "branchesToken": "a-token",
      "owner": "search-run:7",
    },
    "owner": "search-run:7",
  }));

  assert_eq!(
    Research::of(&session),
    Ok(Research {
      strategy: Some("browser-first".into()),
      host_interval_ms: Some(1_000),
      engine_interval_ms: Some(2_000),
      pages: Some(8),
      run: Some(7),
      branch_of: None,
      branches_at: Some("http://127.0.0.1:1/".into()),
      branches_token: Some("a-token".into()),
    })
  );
  assert_eq!(Research::of(&record(serde_json::json!({}))), Ok(Research::default()));
  assert!(Research::of(&record(serde_json::json!({ "research": { "closesAt": "soon" } }))).is_ok());
  assert!(Research::of(&record(serde_json::json!({ "research": { "pages": "many" } }))).is_err());
}

// The page budget is a hard ceiling (docs/web-access.md, 「Settings from the session record」): the pages it names are
// read and every further one is refused, an address read already reading on for nothing, a site tool's page taking
// one each call; a record naming no budget is never refused on it.
#[test]
fn the_page_budget_refuses_every_page_past_it() {
  let guard = ResearchGuard::new(Some(2));

  assert_eq!(guard.take_page(Some("https://a.example/")), None);
  assert_eq!(guard.take_page(Some("https://a.example/")), None);
  assert_eq!(guard.take_page(None), None);
  assert_eq!(guard.take_page(Some("https://b.example/")), Some(BUDGET_SPENT));
  assert_eq!(guard.take_page(None), Some(BUDGET_SPENT));
  assert_eq!(guard.take_page(Some("https://a.example/")), None);

  let unbounded = ResearchGuard::new(None);

  for page in 0..50 {
    assert_eq!(unbounded.take_page(Some(&format!("https://c.example/{page}"))), None);
  }
}
