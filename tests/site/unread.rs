use bmo_explorer::forms::{Failure, WebError};
use bmo_explorer::site::unread::*;

fn after_chrome(message: &str, kind: Failure, status: Option<u16>) -> Option<&'static str> {
  fetch_failure_shape(&WebError::new(message, kind).at(status, None).after_chrome()).map(|(shape, _)| shape)
}

// Carried from packages/web/read-failures.test.ts: is the host with its www, m or mobile prefix dropped
#[test]
fn a_site_is_its_host_with_its_www_m_or_mobile_prefix_dropped() {
  assert_eq!(site_of("https://namu.wiki/w/Vampire%20Survivors").as_deref(), Some("namu.wiki"));
  assert_eq!(
    site_of("https://m.Stock.Naver.com/domestic/stock/005930").as_deref(),
    Some("stock.naver.com")
  );
  assert_eq!(site_of("https://www.example.com/a").as_deref(), Some("example.com"));
  assert_eq!(site_of("not an address"), None);
}

// Carried from packages/web/read-failures.test.ts: is read out of a tool's site line by the hosts it names
#[test]
fn a_tools_site_line_names_its_sites_by_their_hosts() {
  assert_eq!(
    sites_named_in(
      "네이버 증권 (m.stock.naver.com, api.stock.naver.com) and WiseReport (navercomp.wisereport.co.kr)"
    ),
    ["stock.naver.com", "api.stock.naver.com", "navercomp.wisereport.co.kr"]
  );
  assert!(sites_named_in("나무위키").is_empty());
  assert_eq!(
    sites_named_in("가사집 (gasazip.com) gasazip.com/<number> and https://www.gasazip.com/search.html?q=…"),
    ["gasazip.com"]
  );
}

// Carried from packages/web/read-failures.test.ts: is read off the error Chrome failed with after the direct read
#[test]
fn the_shape_is_read_off_the_error_chrome_failed_with() {
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached JavaScript application shell has no readable body at https://namu.wiki/w/x; no CAPTCHA or challenge was attempted",
      Failure::Blocked,
      Some(200)
    ),
    Some("script-only")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached empty HTML page at https://a.example/",
      Failure::Blocked,
      Some(200)
    ),
    Some("script-only")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached CAPTCHA or browser challenge at https://a.example/",
      Failure::Blocked,
      Some(200)
    ),
    Some("challenge")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached redirected to a login page at https://a.example/login",
      Failure::Blocked,
      Some(200)
    ),
    Some("login")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached HTTP 418 at https://a.example/",
      Failure::Blocked,
      Some(418)
    ),
    Some("refused")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached HTTP 429 at https://a.example/",
      Failure::Blocked,
      Some(429)
    ),
    Some("rate-limited")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached HTTP 404 at https://a.example/",
      Failure::Blocked,
      Some(404)
    ),
    Some("gone")
  );
  assert_eq!(
    after_chrome(
      "Standard Chrome still reached HTTP 503 at https://a.example/",
      Failure::Blocked,
      Some(503)
    ),
    Some("server-error")
  );
  assert_eq!(after_chrome("Web request was cancelled", Failure::Deadline, None), Some("timeout"));
}

// Carried from packages/web/read-failures.test.ts: is nothing where Chrome was not tried after the direct read
#[test]
fn nothing_is_shaped_where_chrome_was_not_tried() {
  assert_eq!(
    fetch_failure_shape(
      &WebError::new("JavaScript application shell for https://a.example/", Failure::Blocked)
        .at(Some(200), None)
    ),
    None
  );
  assert_eq!(
    fetch_failure_shape(
      &WebError::new("HTTP 404 Not Found for https://a.example/", Failure::Http).at(Some(404), None)
    ),
    None
  );
}

// Carried from packages/web/read-failures.test.ts: is nothing where the failure is this machine's own
#[test]
fn nothing_is_shaped_where_the_failure_is_this_machines_own() {
  assert_eq!(
    after_chrome(
      "Host pacing for page:a.example would start after the web deadline",
      Failure::Deadline,
      None
    ),
    None
  );
  assert_eq!(
    after_chrome("Standard Chrome exited during startup (1)", Failure::BrowserUnavailable, None),
    None
  );
  assert_eq!(
    after_chrome("Private web address is not allowed: 10.0.0.1", Failure::UnsafeAddress, None),
    None
  );
}

// Carried from packages/web/read-failures.test.ts: names an address refused by this machine's own policy as the machine's in the error text
#[test]
fn an_address_this_machine_refused_is_named_as_the_machines() {
  let own = fetch_error_text(&WebError::new(
    "Private or non-routable web address is not allowed: a.example",
    Failure::UnsafeAddress,
  ));

  assert!(own.starts_with("ERROR: Private or non-routable web address is not allowed: a.example"));
  assert!(own.contains("not a page to name as unread"));
  assert_eq!(
    fetch_error_text(&WebError::new("HTTP 403 for https://a.example/", Failure::Http).at(Some(403), None)),
    "ERROR: HTTP 403 for https://a.example/"
  );
}
