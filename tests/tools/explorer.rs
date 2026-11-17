use std::path::Path;

use bmo_explorer::store;
use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tool::{Answer, DATA_ROOT_VARIABLE, Served, Session};
use bmo_explorer::tools::explorer::{
  EnginesOutput, FetchPage, FetchRaster, Paced, PreviewLink, SearchEngines, TOOLS,
};
use bmo_explorer::web::pacing::{EngineHealth, EngineState, Pacing, StorePacing};
use bmo_explorer::web::search::roster::ROSTER;
use serde_json::json;

fn session(data_root: &Path) -> Session {
  Session {
    holder_name: "search".into(),
    variables: [(DATA_ROOT_VARIABLE.to_owned(), data_root.display().to_string())].into(),
    ..Default::default()
  }
}

#[test]
fn the_manifest_declares_every_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/explorer"), "explorer").unwrap();

  assert_eq!(manifest.differences(TOOLS), Vec::<String>::new());
  assert!(manifest.entries.iter().all(|entry| entry.system), "every entry is for the system alone");
}

#[tokio::test]
async fn the_engines_come_in_the_roster_s_order_with_their_last_state_and_the_rests() {
  let root = tempfile::tempdir().unwrap();
  let health = EngineHealth {
    engine: "daum".into(),
    state: EngineState::Suspended,
    reason: Some("rate: HTTP 429".into()),
    observed_at: Some(1_000),
    suspended_until: Some(181_000),
    elapsed_ms: Some(320),
    result_count: Some(0),
  };

  StorePacing::new(store::place(root.path())).set_health(health).await.unwrap();

  let answer = SearchEngines.call(&session(root.path()), json!({})).await.unwrap();
  let engines: EnginesOutput = serde_json::from_value(answer.structured.unwrap()).unwrap();
  let names: Vec<&str> = engines.engines.iter().map(|engine| engine.name.as_str()).collect();
  let roster: Vec<&str> = ROSTER.engine.iter().map(|engine| engine.name.as_str()).collect();

  assert_eq!(names, roster);
  assert_eq!(
    serde_json::to_value(&engines.engines[0]).unwrap(),
    json!({ "name": "naver", "category": "general", "korean_native": true, "state": "unknown" })
  );
  assert_eq!(
    serde_json::to_value(&engines.engines[1]).unwrap(),
    json!({
      "name": "daum", "category": "general", "korean_native": true, "state": "suspended",
      "reason": "rate: HTTP 429", "observed_at": 1_000, "suspended_until": 181_000, "elapsed_ms": 320,
      "result_count": 0,
    })
  );
  assert_eq!(
    serde_json::to_value(&engines.cooldowns[0]).unwrap(),
    json!({ "reason": "network", "ms": 3_600_000 })
  );
  assert_eq!(engines.cooldowns.len(), ROSTER.cooldown.len());
}

#[tokio::test]
async fn a_page_head_a_preview_and_a_picture_are_refused_an_address_inside_this_network_before_anything_is_sent()
 {
  let root = tempfile::tempdir().unwrap();
  let session = session(root.path());
  let asked = json!({
    "address": format!("http://{}.{}.{}.{}/page", 10, 1, 2, 3),
    "pacing": "search:7",
  });

  for answer in [
    FetchPage.call(&session, asked.clone()).await.unwrap(),
    PreviewLink.call(&session, asked.clone()).await.unwrap(),
    FetchRaster.call(&session, asked.clone()).await.unwrap(),
    FetchRaster
      .call(&session, json!({ "address": asked["address"], "pacing": "search:7", "square": 240 }))
      .await
      .unwrap(),
  ] {
    let Answer { is_error, .. } = answer.clone();

    assert!(is_error, "{answer:?}");
    assert!(
      answer.plain().starts_with("ERROR: Private or non-routable web address is not allowed"),
      "{answer:?}"
    );
  }
}

#[tokio::test]
async fn one_pacing_name_s_requests_are_paced_together_and_apart_from_another_s() {
  let root = tempfile::tempdir().unwrap();
  let one = Paced::new("search:7", StorePacing::new(store::place(root.path())));
  let other = Paced::new("search:8", StorePacing::new(store::place(root.path())));

  assert_eq!(one.reserve("page:example.org", 250, 1_000).await.unwrap(), 1_000);
  assert_eq!(one.reserve("page:example.org", 250, 1_000).await.unwrap(), 1_250);
  assert_eq!(other.reserve("page:example.org", 250, 1_000).await.unwrap(), 1_000);
}

#[tokio::test]
async fn a_card_s_square_of_no_pixels_is_refused() {
  let root = tempfile::tempdir().unwrap();
  let asked = json!({ "address": "https://example.org/cover.jpg", "pacing": "search:7", "square": 0 });
  let answer = FetchRaster.call(&session(root.path()), asked).await.unwrap();

  assert!(answer.is_error, "{answer:?}");
  assert_eq!(answer.plain(), "ERROR: The square is 0; it is at least 1.");
}
