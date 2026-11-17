//! One investigation's reading: slices, the session's cookies, and the fall back to the real browser.

use std::sync::{Arc, Mutex};

use super::testing::{FakeBrowser, Web, answer, check, pdf_of};
use bmo_explorer::web::browser::{Arrived, ChallengeMark, Link};
use bmo_explorer::web::browser::{BrowserDocument, BrowserFetcher, FetchOptions};
use bmo_explorer::web::clock::{Clock, StandingClock};
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::session::*;

fn rendered() -> BrowserDocument {
  BrowserDocument {
    url: "http://fixture.test/app".into(),
    status: 200,
    content_type: "text/html".into(),
    title: "App".into(),
    text: "rendered by JavaScript".into(),
    html: "<html><head><title>App</title></head><body><p>rendered by JavaScript</p><a href=\"/next\">Next</a></body></html>".into(),
    links: vec![Link {
      text: "Next".into(),
      url: "http://fixture.test/next".into(),
    }],
    user_agent: "Chrome".into(),
    webdriver: false,
    truncated: false,
    challenge: ChallengeMark::NotAttempted,
  }
}

// A session reading a stand-in web through a stand-in browser.
fn session(web: Arc<Web>, browser: Arc<FakeBrowser>, clock: Arc<StandingClock>) -> WebSession {
  let made: Arc<dyn BrowserFetcher> = browser;

  WebSession::new(SessionParts {
    store: Arc::new(MemoryPacing::default()),
    transport: web,
    check: check(clock),
    host_interval_ms: 0,
    browsers: Some(Arc::new(move |_| made.clone())),
  })
}

fn every(body: &'static str, content_type: &'static str, status: u16) -> Arc<Web> {
  let hosts = ["fixture.test", "search.shopping.naver.com", "nid.naver.com"];

  Arc::new(hosts.into_iter().fold(Web::default(), |web, host| {
    web.route(host, move |_| answer(status, &[("content-type", content_type)], body.as_bytes()))
  }))
}

// Carried from packages/web/web.test.ts: turns a GitHub file page into the raw file and leaves other URLs alone
#[test]
fn a_github_file_page_becomes_the_raw_file_and_other_urls_stand() {
  assert_eq!(
    raw_url_for("https://github.com/dolidolih/Iris/blob/main/README.md"),
    "https://raw.githubusercontent.com/dolidolih/Iris/main/README.md"
  );
  assert_eq!(
    raw_url_for("https://gist.github.com/someone/0123abcd"),
    "https://gist.githubusercontent.com/someone/0123abcd/raw"
  );
  assert_eq!(raw_url_for("https://github.com/dolidolih/Iris"), "https://github.com/dolidolih/Iris");
  assert_eq!(raw_url_for("https://example.com/a?b=c"), "https://example.com/a?b=c");
}

// Carried from packages/web/web.test.ts: keeps a host cookie between calls in one WebSession and drops it on close
#[tokio::test]
async fn a_host_cookie_is_kept_between_calls_and_dropped_on_close() {
  let web = Arc::new(Web::default().route("fixture.test", |asked| {
    if asked.path == "/first" {
      answer(
        200,
        &[("content-type", "text/plain"), ("set-cookie", "session=one; Path=/; HttpOnly")],
        b"first",
      )
    } else {
      answer(200, &[("content-type", "text/plain")], b"second")
    }
  }));
  let reading = session(web.clone(), FakeBrowser::new(rendered()), StandingClock::at(1_000_000));
  let options = FetchPage::default();

  reading.fetch_page("http://fixture.test/first", &options).await.unwrap();
  reading.fetch_page("http://fixture.test/second", &options).await.unwrap();
  assert_eq!(web.asked().last().unwrap().cookie.as_deref(), Some("session=one"));

  reading.close().await;
  reading.fetch_page("http://fixture.test/second", &options).await.unwrap();
  assert_eq!(web.asked().last().unwrap().cookie, None);
}

fn fallback() -> FetchPage {
  FetchPage {
    browser_fallback: true,
    ..Default::default()
  }
}

// Carried from packages/web/web.test.ts: uses an isolated browser only for a classified shell and preserves provenance and slicing
#[tokio::test]
async fn a_classified_shell_is_read_in_the_browser_with_its_provenance_and_slicing() {
  let browser = FakeBrowser::new(rendered());
  let web = every(
    "<html><body><div id=\"root\"></div><script>render()</script></body></html>",
    "text/html",
    200,
  );
  let reading = session(web, browser.clone(), StandingClock::at(1_000_000));
  let slice = reading
    .fetch_page(
      "http://fixture.test/app",
      &FetchPage {
        max_chars: Some(12),
        ..fallback()
      },
    )
    .await
    .unwrap();

  assert_eq!(
    (slice.provenance, slice.text.as_str(), slice.next_offset),
    ("browser", "rendered by ", Some(12))
  );
  reading.close().await;
  assert!(browser.closed());
}

// Carried from packages/web/web.test.ts: under browser-first reads the page in Chrome with no direct request at all
#[tokio::test]
async fn browser_first_reads_in_chrome_with_no_direct_request() {
  let browser = FakeBrowser::new(rendered());
  let web = every("direct", "text/plain", 200);
  let reading = session(web.clone(), browser.clone(), StandingClock::at(1_000_000));
  let slice = reading
    .fetch_page(
      "http://fixture.test/app",
      &FetchPage {
        browser_first: true,
        ..fallback()
      },
    )
    .await
    .unwrap();

  assert!(web.asked().is_empty());
  assert_eq!(browser.calls(), 1);
  assert_eq!(slice.provenance, "browser");
  assert!(slice.text.contains("rendered by JavaScript"));
}

fn pdf_web(content_type: &'static str) -> Arc<Web> {
  let pdf = pdf_of(&["MPS-35 Series"]);

  Arc::new(
    Web::default().route("fixture.test", move |_| answer(200, &[("content-type", content_type)], &pdf)),
  )
}

// Carried from packages/web/web.test.ts: answers a PDF with its text, sent as a PDF or as anything else
#[tokio::test]
async fn a_pdf_is_answered_with_its_text_whatever_it_was_sent_as() {
  let browser = FakeBrowser::new(rendered());
  let reading = session(pdf_web("application/octet-stream"), browser.clone(), StandingClock::at(1_000_000));
  let slice = reading.fetch_page("http://fixture.test/manual.pdf", &fallback()).await.unwrap();

  assert_eq!((slice.text.as_str(), slice.provenance), ("[page 1]\nMPS-35 Series", "direct"));
  assert_eq!(browser.calls(), 0);
}

// Carried from packages/web/web.test.ts: under browser-first hands a page Chrome shows as a PDF to the direct client
#[tokio::test]
async fn under_browser_first_a_page_chrome_shows_as_a_pdf_goes_to_the_direct_client() {
  let browser = FakeBrowser::new(BrowserDocument {
    content_type: "application/pdf".into(),
    text: String::new(),
    html: "<embed type=\"application/pdf\">".into(),
    ..rendered()
  });
  let reading = session(pdf_web("application/pdf"), browser, StandingClock::at(1_000_000));
  let slice = reading
    .fetch_page(
      "http://fixture.test/manual.pdf",
      &FetchPage {
        browser_first: true,
        ..fallback()
      },
    )
    .await
    .unwrap();

  assert_eq!((slice.text.as_str(), slice.provenance), ("[page 1]\nMPS-35 Series", "direct"));
}

// Carried from packages/web/web.test.ts: under direct-only a classified shell is returned as a block and Chrome is never started
#[tokio::test]
async fn under_direct_only_a_shell_is_a_block_and_chrome_never_starts() {
  let browser = FakeBrowser::new(rendered());
  let web = every(
    "<html><body><div id=\"root\"></div><script src=\"/app.js\"></script></body></html>",
    "text/html",
    200,
  );
  let reading = session(web, browser.clone(), StandingClock::at(1_000_000));
  let failed = reading
    .fetch_page(
      "http://fixture.test/app",
      &FetchPage {
        browser_first: true,
        browser_fallback: false,
        ..Default::default()
      },
    )
    .await
    .unwrap_err();

  assert!(failed.message.contains("JavaScript application shell"));
  assert_eq!(browser.calls(), 0);
}

// Carried from packages/web/web.test.ts: reports a challenge that remains in Chrome instead of treating it as content
#[tokio::test]
async fn a_challenge_that_remains_in_chrome_is_reported() {
  let browser = FakeBrowser::new(BrowserDocument {
    status: 403,
    title: "Just a moment...".into(),
    text: "Verify you are human".into(),
    html: "<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div></body></html>".into(),
    ..rendered()
  });
  let reading = session(every("denied", "text/plain", 403), browser, StandingClock::at(1_000_000));
  let failed = reading.fetch_page("https://fixture.test/", &fallback()).await.unwrap_err();

  assert_eq!((failed.kind, failed.chrome_fallback), (Failure::Blocked, true));
}

// The fetch road's browser read asks for the one attempt (docs/web-access.md, 「When the browser meets a
// challenge」): a challenge the attempt cleared is answered as the page behind it.
#[tokio::test]
async fn a_challenge_the_attempt_cleared_is_answered_as_the_page_behind_it() {
  let browser = FakeBrowser::new(BrowserDocument {
    challenge: ChallengeMark::Cleared,
    ..rendered()
  });
  let reading = session(every("denied", "text/plain", 403), browser.clone(), StandingClock::at(1_000_000));
  let slice = reading.fetch_page("https://fixture.test/", &fallback()).await.unwrap();

  assert_eq!(slice.provenance, "browser");
  assert!(slice.text.contains("rendered by JavaScript"));
  let asked = browser.asked();
  assert_eq!(asked.len(), 1, "the cleared page is the one read the fallback made");
  assert!(asked[0].solve, "the fetch road asks for the attempt");
}

// A challenge that stands after the attempt is refused with what was done said, and the read never goes back.
#[tokio::test]
async fn a_challenge_attempted_once_and_still_standing_is_refused_once_and_final() {
  let browser = FakeBrowser::new(BrowserDocument {
    status: 403,
    title: "Just a moment...".into(),
    text: "Verify you are human".into(),
    html: "<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div></body></html>".into(),
    challenge: ChallengeMark::Remains,
    ..rendered()
  });
  let reading = session(every("denied", "text/plain", 403), browser.clone(), StandingClock::at(1_000_000));
  let failed = reading.fetch_page("https://fixture.test/", &fallback()).await.unwrap_err();

  assert_eq!((failed.kind, failed.chrome_fallback), (Failure::Blocked, true));
  assert!(
    failed.message.contains("the challenge was attempted once and did not clear"),
    "the refusal says what was tried: {failed}"
  );
  assert_eq!(browser.calls(), 1, "one refusal is final for the read");
}

// A refusal whose page holds no challenge is not attempted at, and its words say so as they did before.
#[tokio::test]
async fn a_plain_refusal_in_the_browser_still_says_nothing_was_attempted() {
  let browser = FakeBrowser::new(BrowserDocument {
    status: 403,
    title: "Forbidden".into(),
    text: "Access denied".into(),
    html: "<html><body><p>Access denied</p></body></html>".into(),
    ..rendered()
  });
  let reading = session(every("denied", "text/plain", 403), browser, StandingClock::at(1_000_000));
  let failed = reading.fetch_page("https://fixture.test/", &fallback()).await.unwrap_err();

  assert!(failed.message.contains("no CAPTCHA or challenge was attempted"), "{failed}");
}

// The forcing read opens the browser at once, asks for the attempt, and carries its outcome beside the page.
#[tokio::test]
async fn the_forcing_read_sends_nothing_direct_and_names_what_it_met() {
  let web = Arc::new(
    Web::default()
      .route("fixture.test", |_| answer(200, &[("content-type", "text/html")], b"<p>plain</p>")),
  );
  let browser = FakeBrowser::new(rendered());
  let reading = session(web.clone(), browser.clone(), StandingClock::at(1_000_000));
  let read = reading
    .captcha(
      "https://fixture.test/app",
      &FetchPage {
        browser_fallback: true,
        browser_first: true,
        ..FetchPage::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(read.challenge, ChallengeMark::NotAttempted);
  assert_eq!(read.line(), "NO CHALLENGE");
  assert!(browser.asked()[0].solve);
  assert!(web.asked().is_empty(), "the forcing road asks the site only through the browser");
}

// Carried from packages/web/web.test.ts: opens the address asked for in Chrome when the direct client was sent to a login page
#[tokio::test]
async fn the_address_asked_for_is_opened_in_chrome_when_the_direct_client_was_sent_to_login() {
  let catalog = "https://search.shopping.naver.com/book/catalog/61591468197";
  let web = Arc::new(
    Web::default()
      .route("search.shopping.naver.com", move |_| {
        let login = format!(
          "https://nid.naver.com/nidlogin.login?url={}",
          url::form_urlencoded::byte_serialize(catalog.as_bytes()).collect::<String>()
        );

        answer(307, &[("location", &login)], b"")
      })
      .route("nid.naver.com", |_| {
        answer(
          200,
          &[("content-type", "text/html")],
          "<html><head><title>로그인</title></head><body><p>네이버 로그인</p></body></html>".as_bytes(),
        )
      }),
  );
  let browser = FakeBrowser::new(BrowserDocument {
    url: catalog.into(),
    title: "클린 코드".into(),
    text: "목차 1장 클린 코드".into(),
    html: "<html><body><p>목차 1장 클린 코드</p></body></html>".into(),
    links: Vec::new(),
    ..rendered()
  });
  let reading = session(web, browser.clone(), StandingClock::at(1_000_000));
  let slice = reading.fetch_page(catalog, &fallback()).await.unwrap();

  assert_eq!(
    browser.calls.lock().unwrap().iter().map(|one| one.url.as_str()).collect::<Vec<_>>(),
    [catalog]
  );
  assert_eq!(slice.provenance, "browser");
  assert!(slice.text.contains("목차"));
}

// Carried from packages/web/web.test.ts: answers a 204 with a no-content line and never starts Chrome
#[tokio::test]
async fn a_204_is_a_no_content_line_and_chrome_never_starts() {
  let browser = FakeBrowser::new(rendered());
  let reading = session(every("", "", 204), browser.clone(), StandingClock::at(1_000_000));
  let slice = reading.fetch_page("https://fixture.test/ping", &fallback()).await.unwrap();

  assert_eq!(slice.text, "NO CONTENT: the page answered HTTP 204 with no content.");
  assert_eq!(slice.status, 204);
  assert_eq!(browser.calls(), 0);
}

// A page's Retry-After is the site's own span: the read waits it out, however long, and then asks Chrome
#[tokio::test]
async fn a_page_s_retry_after_is_waited_out_before_chrome_asks_again() {
  let browser = FakeBrowser::new(rendered());
  let web = Arc::new(
    Web::default().route("fixture.test", |_| answer(429, &[("retry-after", "3600")], b"slow down")),
  );
  let clock = StandingClock::at(1_000);
  let reading = session(web, browser.clone(), clock.clone());
  let read = reading.fetch_page("https://fixture.test/", &fallback()).await.unwrap();

  assert_eq!(read.provenance, "browser");
  assert_eq!(browser.calls(), 1);
  assert!(clock.now() >= 1_000 + 3_600_000);
}

// Carried from packages/web/web.test.ts: renders a page asked for fresh in a browser of its own and ends it, leaving the run's browser alone
#[tokio::test]
async fn a_page_asked_for_fresh_is_rendered_in_a_browser_of_its_own_and_ended() {
  let started: Arc<Mutex<Vec<Arc<FakeBrowser>>>> = Arc::new(Mutex::new(Vec::new()));
  let made = started.clone();
  let reading = WebSession::new(SessionParts {
    store: Arc::new(MemoryPacing::default()),
    transport: Arc::new(Web::default()),
    check: check(StandingClock::at(1_000_000)),
    host_interval_ms: 0,
    browsers: Some(Arc::new(move |_| {
      let browser = FakeBrowser::new(rendered());

      made.lock().unwrap().push(browser.clone());
      browser
    })),
  });
  reading.render("https://fixture.test/app", true, FetchOptions::default()).await.unwrap();
  reading.render("https://fixture.test/app", true, FetchOptions::default()).await.unwrap();
  assert_eq!(
    started
      .lock()
      .unwrap()
      .iter()
      .map(|one| (one.calls(), one.closed()))
      .collect::<Vec<_>>(),
    [(1, true), (1, true)]
  );

  let arrived: Arrived = Arc::new(|url, _| !url.contains("/check"));

  reading
    .render("https://fixture.test/app", false, FetchOptions::default())
    .await
    .unwrap();
  reading
    .render(
      "https://fixture.test/app",
      false,
      FetchOptions {
        arrived: Some(arrived),
        ..FetchOptions::default()
      },
    )
    .await
    .unwrap();

  let run = started.lock().unwrap()[2].clone();

  assert_eq!(started.lock().unwrap().len(), 3);
  assert_eq!((run.calls(), run.closed()), (2, false));
  assert_eq!(
    run.calls.lock().unwrap().iter().map(|one| one.arrived).collect::<Vec<_>>(),
    [false, true]
  );
  reading.close().await;
  assert!(run.closed());
}
