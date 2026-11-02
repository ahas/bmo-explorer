//! The store (docs/store.md): made under the data root by its first reader, and the web layer's pacing kept in
//! it — a host's pacing slots, an engine's health, its recipe and its cookie jar.

use bmo_explorer::store;
use bmo_explorer::web::jar::StoredCookie;
use bmo_explorer::web::pacing::{
  ENGINE_COOKIE_LIFETIME_MS, EngineHealth, EngineState, Pacing, Recipe, Route, StorePacing,
};
use sqlx::Row;

#[tokio::test]
async fn the_store_is_made_in_its_folder_of_the_data_root_with_every_table() {
  let root = tempfile::tempdir().unwrap();
  let place = store::place(root.path());

  assert_eq!(place, root.path().join("explorer").join("explorer.db"));

  let mut connection = store::open(&place).await.unwrap();
  let tables: Vec<String> =
    sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
      .fetch_all(&mut connection)
      .await
      .unwrap()
      .iter()
      .map(|row| row.get("name"))
      .collect();

  assert_eq!(
    tables,
    ["web_engine_cookie", "web_engine_health", "web_engine_recipe", "web_host_pacing"]
  );

  // A store that stands already opens with what it keeps, and a table it does not keep is dropped.
  sqlx::query("CREATE TABLE left_behind (id INTEGER PRIMARY KEY)")
    .execute(&mut connection)
    .await
    .unwrap();
  sqlx::query("INSERT INTO web_host_pacing (key, next_at, updated_at) VALUES ('example.org', 1, 1)")
    .execute(&mut connection)
    .await
    .unwrap();
  drop(connection);

  let mut connection = store::open(&place).await.unwrap();
  let tables: Vec<String> =
    sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
      .fetch_all(&mut connection)
      .await
      .unwrap()
      .iter()
      .map(|row| row.get("name"))
      .collect();
  let paced: i64 = sqlx::query("SELECT COUNT(*) AS count FROM web_host_pacing")
    .fetch_one(&mut connection)
    .await
    .unwrap()
    .get("count");

  assert_eq!(
    tables,
    ["web_engine_cookie", "web_engine_health", "web_engine_recipe", "web_host_pacing"]
  );
  assert_eq!(paced, 1);
}

#[tokio::test]
async fn the_web_store_keeps_its_pacing_health_recipes_and_cookies() {
  let root = tempfile::tempdir().unwrap();
  let store = StorePacing::new(store::place(root.path()));

  // A host's slots, one interval apart, each taken however far off it falls.
  assert_eq!(store.reserve("page:a", 1_000, 10_000).await.unwrap(), 10_000);
  assert_eq!(store.reserve("page:a", 1_000, 10_000).await.unwrap(), 11_000);
  store.defer("page:a", 50_000, 11_000).await.unwrap();
  assert_eq!(store.reserve("page:a", 1_000, 11_000).await.unwrap(), 50_000);
  assert_eq!(store.reserve("page:a", 1_000, 11_000).await.unwrap(), 51_000);

  let health = EngineHealth {
    engine: "naver".into(),
    state: EngineState::Suspended,
    reason: Some("CAPTCHA".into()),
    observed_at: Some(1_000),
    suspended_until: Some(5_000),
    elapsed_ms: Some(320),
    result_count: Some(0),
  };

  assert_eq!(store.health("naver").await.unwrap(), None);
  store.set_health(health.clone()).await.unwrap();
  assert_eq!(store.health("naver").await.unwrap(), Some(health));

  let recipe = Recipe {
    engine: "naver".into(),
    identity: "chrome-windows".into(),
    interval_ms: 4_000,
    route: Route::Browser,
    warm_up: true,
    cause: Some("challenge".into()),
    evidence: Some("HTTP 403".into()),
    set_at: 2_000,
  };

  store.set_recipe(recipe.clone()).await.unwrap();
  assert_eq!(store.recipe("naver").await.unwrap(), Some(recipe));

  let cookie = |value: &str, expires_at| StoredCookie {
    host: "Search.Example".into(),
    path: "/".into(),
    name: "s".into(),
    value: value.into(),
    secure: true,
    expires_at,
  };

  store
    .set_cookies("naver", vec![cookie("old", None), cookie("new", Some(u64::MAX))], 0)
    .await
    .unwrap();

  let jar = store.cookies("naver", 0).await.unwrap();

  assert_eq!(jar.len(), 1);
  assert_eq!((jar[0].value.as_str(), jar[0].host.as_str()), ("new", "search.example"));
  assert_eq!(jar[0].expires_at, Some(ENGINE_COOKIE_LIFETIME_MS));
  assert!(store.cookies("naver", ENGINE_COOKIE_LIFETIME_MS).await.unwrap().is_empty());
}

#[tokio::test]
async fn two_stores_on_one_place_are_handed_different_slots() {
  let root = tempfile::tempdir().unwrap();
  let one = StorePacing::new(store::place(root.path()));
  let other = StorePacing::new(store::place(root.path()));
  let (first, second) =
    tokio::join!(one.reserve("engine:naver", 1_000, 10_000), other.reserve("engine:naver", 1_000, 10_000));
  let mut slots = [first.unwrap(), second.unwrap()];

  slots.sort_unstable();
  assert_eq!(slots, [10_000, 11_000]);
}
