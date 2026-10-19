use bmo_explorer::web::jar::*;
use url::Url;

#[test]
fn keeps_host_path_and_secure_cookies_and_refuses_one_planted_for_a_parent_domain() {
  let mut jar = Jar::default();
  let page = Url::parse("https://shop.example.com/a/b").unwrap();

  jar.capture(
    &page,
    [
      "sid=1; Path=/; Secure",
      "deep=2",
      "parent=3; Domain=example.com",
      "__Host-ok=4; Secure; Path=/",
      "__Host-bad=5; Path=/a",
      "gone=6; Max-Age=0",
    ],
    1_000,
  );

  assert_eq!(jar.header(&page, 1_000).as_deref(), Some("deep=2; sid=1; __Host-ok=4"));
  assert_eq!(jar.header(&Url::parse("http://shop.example.com/").unwrap(), 1_000), None);
  assert_eq!(jar.header(&Url::parse("https://example.com/").unwrap(), 1_000), None);
  assert_eq!(
    jar.header(&Url::parse("https://shop.example.com/").unwrap(), 1_000).as_deref(),
    Some("sid=1; __Host-ok=4")
  );
}

#[test]
fn hands_its_live_cookies_to_a_store_and_takes_them_back() {
  let mut jar = Jar::default();
  let page = Url::parse("https://search.example/q").unwrap();

  jar.capture(&page, ["a=1; Max-Age=60", "b=2; Max-Age=1"], 0);

  let held = jar.held(2_000);

  assert_eq!(held.len(), 1);
  assert_eq!(held[0].expires_at, Some(60_000));

  let mut again = Jar::default();

  again.hold(&held, 2_000);
  assert_eq!(again.header(&page, 2_000).as_deref(), Some("a=1"));
}
