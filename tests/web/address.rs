use bmo_explorer::web::address::*;
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::error::Failure;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use url::Url;

struct Answers(Vec<IpAddr>);

impl Lookup for Answers {
  fn lookup<'a>(
    &'a self,
    _: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    let found = self.0.clone();

    Box::pin(async move { Ok(found) })
  }
}

struct Never;

impl Lookup for Never {
  fn lookup<'a>(
    &'a self,
    _: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    Box::pin(std::future::pending())
  }
}

fn check(lookup: Arc<dyn Lookup>) -> Check {
  Check {
    allow_private: false,
    lookup,
    clock: StandingClock::at(1_000),
  }
}

#[test]
fn a_special_purpose_range_is_refused_by_its_own_prefix_and_not_the_public_block_beside_it() {
  for refused in [
    "10.1.2.3",
    "127.0.0.1",
    "169.254.169.254",
    "100.64.0.1",
    "192.0.0.8",
    "198.18.0.1",
    "224.0.0.1",
    "::1",
    "fd00::1",
    "fe80::1",
    "::ffff:10.0.0.1",
    "2001:db8::1",
  ] {
    assert!(is_forbidden(refused.parse().unwrap()), "{refused}");
  }
  for allowed in ["192.0.64.10", "8.8.8.8", "100.128.0.1", "2606:4700::1111"] {
    assert!(!is_forbidden(allowed.parse().unwrap()), "{allowed}");
  }
}

#[tokio::test]
async fn credentials_private_answers_and_other_schemes_are_refused_before_a_connection() {
  let private = check(Arc::new(Answers(vec!["10.0.0.5".parse().unwrap()])));

  for (url, kind) in [
    ("https://user:pw@example.com/", Failure::UnsafeAddress),
    ("ftp://example.com/", Failure::UnsafeAddress),
    ("https://example.com/", Failure::UnsafeAddress),
    ("http://localhost:8080/", Failure::UnsafeAddress),
  ] {
    assert_eq!(private.resolve(&Url::parse(url).unwrap(), None).await.unwrap_err().kind, kind, "{url}");
  }

  let public = check(Arc::new(Answers(vec!["93.184.216.34".parse().unwrap()])));
  let target = public.resolve(&Url::parse("https://example.com/a").unwrap(), None).await.unwrap();

  assert_eq!(target.address, Some("93.184.216.34".parse().unwrap()));
}

#[tokio::test]
async fn a_lookup_past_the_deadline_is_the_deadline_and_sends_nothing() {
  let slow = check(Arc::new(Never));
  let failed = slow
    .resolve(&Url::parse("https://slow.example/").unwrap(), Some(1_020))
    .await
    .unwrap_err();

  assert_eq!(failed.kind, Failure::Deadline);
}
