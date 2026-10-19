use bmo_explorer::web::clock::{Clock, StandingClock};
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::jar::StoredCookie;
use bmo_explorer::web::pacing::*;

#[tokio::test]
async fn a_slot_is_waited_for_and_none_is_taken_past_the_deadline() {
  let store = MemoryPacing::default();
  let clock = StandingClock::at(10_000);

  assert_eq!(wait_for_host(&store, "page:a", 1_000, 20_000, &*clock).await.unwrap(), 10_000);
  assert_eq!(wait_for_host(&store, "page:a", 1_000, 20_000, &*clock).await.unwrap(), 11_000);
  assert_eq!(clock.now(), 11_000);

  store.defer("page:a", 50_000, clock.now()).await.unwrap();
  assert_eq!(
    wait_for_host(&store, "page:a", 1_000, 20_000, &*clock).await.unwrap_err().kind,
    Failure::Deadline
  );
  assert_eq!(store.reserve("page:a", 1_000, 60_000, clock.now()).await.unwrap(), Some(50_000));
}

#[tokio::test]
async fn an_engine_jar_keeps_each_cookie_no_longer_than_a_week_and_the_later_of_two() {
  let store = MemoryPacing::default();
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

  let kept = store.cookies("naver", 0).await.unwrap();

  assert_eq!(kept.len(), 1);
  assert_eq!(kept[0].value, "new");
  assert_eq!(kept[0].host, "search.example");
  assert_eq!(kept[0].expires_at, Some(ENGINE_COOKIE_LIFETIME_MS));
}
