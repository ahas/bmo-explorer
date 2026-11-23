use bmo_explorer::web::browser::*;
use bmo_explorer::web::clock::{Clock, StandingClock};
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::error::WebError;
use serde_json::json;
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
  let refused = WebError::new(
    format!("Private or non-routable web address is not allowed: {}.{}.{}.1", 10, 0, 0),
    Failure::UnsafeAddress,
  );

  assert!(!fails_the_page(&unresolved, Some("Script"), Some("main"), Some("main")));
  assert!(!fails_the_page(&unresolved, Some("Document"), Some("child"), Some("main")));
  assert!(fails_the_page(&unresolved, Some("Document"), Some("main"), Some("main")));
  assert!(fails_the_page(&refused, Some("Image"), Some("child"), Some("main")));
}

// The address is watched until the page has arrived, and a page that has come to rest is read once more and
// taken as it stands
#[tokio::test]
async fn the_address_is_watched_until_the_page_has_arrived_or_has_come_to_rest() {
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

  assert!(await_arrival(read, &arrived, || false, &*clock).await);
  assert_eq!(clock.now(), 500);

  // A check that stays where it is is watched for as long as the page does not rest, however long that is.
  let never: Arrived = Arc::new(|_, _| false);
  let reads = AtomicUsize::new(0);
  let still = || {
    reads.fetch_add(1, Ordering::SeqCst);
    async { Ok(("https://site.example/check".to_owned(), "Checking".to_owned())) }
  };
  let resting_from = clock.now() + 60_000;

  assert!(!await_arrival(still, &never, || clock.now() >= resting_from, &*clock).await);
  assert!(clock.now() >= resting_from);

  // The read taken as the page came to rest is the last one, and a page that arrived just then is taken.
  let looked = reads.load(Ordering::SeqCst);
  let last_turn = AtomicUsize::new(0);
  let arriving = || {
    let at = last_turn.fetch_add(1, Ordering::SeqCst);

    async move {
      Ok(if at == 0 {
        ("https://site.example/check".to_owned(), "Checking".to_owned())
      } else {
        ("https://site.example/page".to_owned(), "Page".to_owned())
      })
    }
  };
  let rested_at = clock.now() + 250;

  assert!(looked > 1);
  assert!(await_arrival(arriving, &arrived, || clock.now() >= rested_at, &*clock).await);
}

// A dialog a page opens is answered at once in either browser, or the read would never end: an alert, a confirm
// and a prompt dismissed, a question before the page unloads accepted so the read's navigation goes on.
#[test]
fn a_dialog_a_page_opens_is_dismissed_and_a_question_before_unloading_accepted() {
  for (kind, accept) in [("alert", false), ("confirm", false), ("prompt", false), ("beforeunload", true)] {
    assert_eq!(
      chrome::dialog_answer(&json!({ "type": kind, "message": "Sold out" })),
      json!({ "accept": accept })
    );
    assert_eq!(
      firefox::prompt_answer(&json!({ "context": "tab", "type": kind, "message": "Sold out" })),
      json!({ "context": "tab", "accept": accept })
    );
  }
}

// A page that leaves its tab for no web page at all, as a sold-out page sends its tab back to an empty one after
// its alert, fails the read saying so with the dialog's words, never as an address the guard refused.
#[test]
fn a_page_that_leaves_its_tab_for_no_web_page_fails_the_read_with_its_dialogs_words() {
  let left =
    left_the_web("https://shop.example/item/1", "about:blank", Some("This item is no longer sold."));

  assert_eq!(left.kind, Failure::Http);
  assert_eq!(
    left.message,
    "The page at https://shop.example/item/1 left its tab for about:blank before it could be read; its dialog \
     said: This item is no longer sold."
  );
  assert!(
    !left_the_web("https://shop.example/item/1", "about:blank", None)
      .message
      .contains("dialog")
  );
}

// A page whose content has loaded while a request of it holds its load event back is read without that load
// once its content has stood loaded for the bound; before its content has loaded, nothing ends the wait.
#[test]
fn a_page_is_read_without_its_load_once_its_content_has_stood_loaded_long_enough() {
  let content = 50_000;

  assert!(!read_without_load(None, content + 10 * LOAD_AFTER_CONTENT_MS));
  assert!(!read_without_load(Some(content), content + LOAD_AFTER_CONTENT_MS - 1));
  assert!(read_without_load(Some(content), content + LOAD_AFTER_CONTENT_MS));
}
