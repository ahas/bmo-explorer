use bmo_explorer::web::chrome::*;
use bmo_explorer::web::clock::{Clock, StandingClock};
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::error::WebError;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

// Carried from packages/web/browser-fetch.test.ts: Chrome is launched off every person's session: its own display, no session bus, no keyring
#[test]
fn chrome_is_launched_off_every_persons_session() {
  let profile = Path::new("/tmp/bmo-web-chrome-x");
  let arguments = launch_arguments("/usr/bin/google-chrome", profile, 9222, 4000);
  let environment = launch_environment(
    [("DISPLAY".to_owned(), ":0".to_owned()), ("HOME".to_owned(), "/srv/user".to_owned())],
    ":9",
    Path::new("/tmp/bmo-web-chrome-x/display/Xauthority"),
    Path::new("/tmp/bmo-web-chrome-x/display/runtime"),
    profile,
  );

  assert!(arguments.contains(&"--password-store=basic".to_owned()));
  assert!(arguments.contains(&"--ozone-platform=x11".to_owned()));
  assert!(arguments.contains(&"--user-data-dir=/tmp/bmo-web-chrome-x".to_owned()));
  assert!(arguments.contains(&"--proxy-server=http://127.0.0.1:4000".to_owned()));
  assert_eq!(arguments.last().unwrap(), "about:blank");
  assert_eq!(environment["DISPLAY"], ":9");
  assert_eq!(environment["DBUS_SESSION_BUS_ADDRESS"], "disabled:");
  assert_eq!(environment["XDG_CACHE_HOME"], "/tmp/bmo-web-chrome-x/cache");
}

// Carried from packages/web/browser-fetch.test.ts: a navigation error is the guard's refusal where one was recorded, and a transport failure where none was
#[test]
fn a_navigation_error_is_the_guards_refusal_where_one_was_recorded() {
  let refusal =
    WebError::new("Private or non-routable web address is not allowed: inner", Failure::UnsafeAddress);

  assert_eq!(
    navigation_failure("https://a.example/", "net::ERR_BLOCKED_BY_CLIENT", Some(refusal.clone())),
    refusal
  );
  assert_eq!(
    navigation_failure("https://a.example/", "net::ERR_FAILED", None).kind,
    Failure::Transport
  );
}

// Carried from packages/web/browser-fetch.test.ts: a name with no address fails only its own request, unless it is the page itself
#[test]
fn a_name_with_no_address_fails_only_its_own_request_unless_it_is_the_page_itself() {
  let unresolved = WebError::new("Could not resolve web host ads.example", Failure::Transport);
  let refused =
    WebError::new("Private or non-routable web address is not allowed: 10.0.0.1", Failure::UnsafeAddress);

  assert!(!fails_the_page(&unresolved, Some("Script"), Some("main"), Some("main")));
  assert!(!fails_the_page(&unresolved, Some("Document"), Some("child"), Some("main")));
  assert!(fails_the_page(&unresolved, Some("Document"), Some("main"), Some("main")));
  assert!(fails_the_page(&refused, Some("Image"), Some("child"), Some("main")));
}

// Carried from packages/web/browser-fetch.test.ts: the address is watched until the page has arrived, and the wait ends at its bound
#[tokio::test]
async fn the_address_is_watched_until_the_page_has_arrived_and_the_wait_ends_at_its_bound() {
  let clock = StandingClock::at(0);
  let turn = AtomicUsize::new(0);
  let arrived: Arrived = Arc::new(|url, title| url == "https://site.example/page" && title == "Page");
  let read = || {
    let at = turn.fetch_add(1, Ordering::SeqCst);

    async move {
      match at {
        0 => Err(WebError::new("between documents", Failure::Transport)),
        1 => Ok(("https://site.example/check".to_owned(), "Checking".to_owned())),
        _ => Ok(("https://site.example/page".to_owned(), "Page".to_owned())),
      }
    }
  };

  assert!(await_arrival(read, &arrived, 10_000, &*clock).await);
  assert_eq!(clock.now(), 500);

  let never: Arrived = Arc::new(|_, _| false);
  let still = || async { Ok(("https://site.example/check".to_owned(), "Checking".to_owned())) };

  assert!(!await_arrival(still, &never, 10_000, &*clock).await);
  assert!(clock.now() <= 10_000);
}
