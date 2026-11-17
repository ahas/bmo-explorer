//! The real browser a read falls back to (docs/web-access.md, 「Direct collection and the real browser」): Firefox
//! on a copy of the owner's own profile where this machine has one, standard Chrome on a fresh temporary profile
//! everywhere else, each non-headless on a private display of its own, every request it makes sent through a
//! proxy that connects only to an address the caller's check has just passed, and every request a page makes
//! paused and checked the same way before the browser sends it. One navigation at a time, each waited for until
//! its page has loaded or the browser has gone; a browser no read has used for three minutes ends and gives its
//! machine place back.
//!
//! What the two browsers share — the queue, the idle end, the watch for a page's arrival and the snapshot of a
//! page that holds still — stands here; each product answers how it is started and how its page is driven.

pub mod challenge;
pub mod chrome;
pub mod copy;
pub mod detached;
pub mod display;
pub mod firefox;
pub mod places;
pub mod profile;
pub mod proxy;
pub mod remote;

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::forms::classify::{Fetched, classify};
use crate::web::clock::{self, Clock, now_ms};
use crate::web::error::{Failure, WebError, WebResult};

pub use chrome::{launch_arguments, launch_environment};

/// A page as the browser rendered it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowserDocument {
  pub url: String,
  pub status: u16,
  pub content_type: String,
  pub title: String,
  pub text: String,
  pub html: String,
  pub links: Vec<Link>,
  pub user_agent: String,
  pub webdriver: bool,
  pub truncated: bool,
  /// What the read's one attempt at a met challenge came to (「When the browser meets a challenge」).
  pub challenge: ChallengeMark,
}

/// What a read that asks for the attempt (docs/web-access.md, 「When the browser meets a challenge」) did
/// about a challenge met in its page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChallengeMark {
  /// The read met no challenge, or did not ask for the attempt.
  #[default]
  NotAttempted,
  /// The attempt was made and the retried read answered a page that is no longer a challenge.
  Cleared,
  /// The attempt was made and the retried read answered a challenge again.
  Remains,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Link {
  pub text: String,
  pub url: String,
}

/// The check every request of the browser passes: the address the host may be connected at, none on a
/// trusted road that checks nothing, or a refusal.
pub type UrlGuard =
  Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = WebResult<Option<IpAddr>>> + Send>> + Send + Sync>;

/// Whether the address a page stands at, with the document's title, is the page that was asked for, for a
/// site whose own browser check runs its script and moves on to the page by itself.
pub type Arrived = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// How one page is read, where its consumer knows more of its site than this layer does.
#[derive(Clone)]
pub struct FetchOptions {
  pub arrived: Option<Arrived>,
  /// Whether a challenge met in the page is attempted once and the read retried
  /// (docs/web-access.md, 「When the browser meets a challenge」); false where the road does not ask.
  pub solve_challenge: bool,
  /// The host's turn in its pacing queue, taken before the retried read opens the address again — every
  /// top-level navigation waits, the retry included; none where the road holds no queue of its own.
  pub pace: Option<Pace>,
  /// The clock the attempt's waits stand on; the machine's own where none is handed, a test's otherwise.
  pub clock: Arc<dyn Clock>,
}

impl Default for FetchOptions {
  fn default() -> Self {
    Self {
      arrived: None,
      solve_challenge: false,
      pace: None,
      clock: clock::system(),
    }
  }
}

/// What reads a page in a real browser: the machine's browser, or a test's stand-in.
pub trait BrowserFetcher: Send + Sync {
  fn fetch(
    &self,
    url: String,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>>;
  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// What a read waits for before a browser opens a page of a host: the host's turn in its pacing queue.
pub type Pace = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = WebResult<()>> + Send>> + Send + Sync>;

/// This machine's browser (docs/web-access.md, 「Which browser」): Firefox on a copy of the owner's profile where
/// the machine has one, with Chrome on a temporary profile after a refusal of it; Chrome alone otherwise.
pub fn on_this_machine(guard: UrlGuard, pace: Pace) -> Arc<dyn BrowserFetcher> {
  let chrome = Arc::new(Browser::new(guard.clone(), Box::new(chrome::Chrome::installed())));

  match firefox::Firefox::on_this_machine() {
    Some(firefox) => {
      Arc::new(Fallback::new(Arc::new(Browser::new(guard, Box::new(firefox))), chrome, pace))
    }
    None => chrome,
  }
}

/// Whether a page's status is its site's outright refusal of the browser.
pub fn refused(status: u16) -> bool {
  matches!(status, 403 | 418 | 420 | 429)
}

/// One browser read first and another after it, where the site refused the first outright or the first could
/// not be started: the second's page is taken where its site did not refuse it too, and the first's answer
/// otherwise. The second read waits for the host's pacing first, as every top-level navigation does.
pub struct Fallback {
  first: Arc<dyn BrowserFetcher>,
  then: Arc<dyn BrowserFetcher>,
  pace: Pace,
}

impl Fallback {
  pub fn new(first: Arc<dyn BrowserFetcher>, then: Arc<dyn BrowserFetcher>, pace: Pace) -> Self {
    Self { first, then, pace }
  }
}

impl BrowserFetcher for Fallback {
  fn fetch(
    &self,
    url: String,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>> {
    Box::pin(async move {
      let read = self.first.fetch(url.clone(), options.clone()).await;
      let turned_away = match &read {
        Ok(page) => refused(page.status),
        Err(error) => error.kind == Failure::BrowserUnavailable,
      };

      if !turned_away || (self.pace)(url.clone()).await.is_err() {
        return read;
      }

      match self.then.fetch(url, options).await {
        Ok(page) if !refused(page.status) => Ok(page),
        _ => read,
      }
    })
  }

  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    Box::pin(async move {
      self.first.close().await;
      self.then.close().await;
    })
  }
}

/// A future a product's browser answers with.
pub type Answer<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A browser product: how one browser of it is started under a guard.
pub trait Product: Send + Sync {
  fn start<'a>(&'a self, guard: &'a UrlGuard) -> Answer<'a, WebResult<Box<dyn Running>>>;
}

/// One browser as it runs, with everything started for it, driven one navigation at a time.
pub trait Running: Send + Sync {
  /// Whether its connection still stands.
  fn is_open(&self) -> bool;
  /// The page sent to `url` and waited for until its load has fired, the guard has refused a request that
  /// fails the page, the navigation has failed, or the browser has gone.
  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>>;
  /// An expression's value in the page, as plain JSON.
  fn evaluate<'a>(&'a self, expression: &'a str) -> Answer<'a, WebResult<Value>>;
  /// One trusted click at a point of the page's view, through the browser's own input path: what a widget
  /// of a challenge page accepts as a person's click.
  fn click<'a>(&'a self, x: f64, y: f64) -> Answer<'a, WebResult<()>>;
  /// What the page's events have shown of the navigation under way.
  fn watch(&self) -> &PageWatch;
  /// Whether the page has come to rest (docs/web-access.md, 「Lifetime and cleanup」).
  fn rested(&self) -> bool;
  /// The browser ended with everything started for it, and its place given back.
  fn shut_down(self: Box<Self>) -> Answer<'static, ()>;
}

const SNAPSHOT_CHARS: usize = 4_000_000;
const LINK_CAP: usize = 1_000;
const LINK_FIELD_CAP: usize = 2_000;
// How often the watch for a page's arrival looks again.
const ARRIVAL_POLL_MS: u64 = 250;
// After the load, how far apart two snapshots are taken to see whether the page holds still, and when the
// snapshot of a page that never does is taken. It says when to read; it never fails the read.
const SETTLE_POLL_MS: u64 = 150;
const SETTLE_MS: u64 = 2_000;

fn idle_ms() -> u64 {
  std::env::var("BMO_CHROME_IDLE_SECONDS")
    .ok()
    .and_then(|named| named.parse::<f64>().ok())
    .filter(|seconds| *seconds > 0.0)
    .map_or(180_000, |seconds| (seconds * 1_000.0) as u64)
}

/// What a navigation that came back with an error text actually was: the guard's refusal wherever one was
/// recorded — the browser's answer to a refused request can reach the caller before the refusal is raised — and
/// a transport failure only where none was.
pub fn navigation_failure(url: &str, error_text: &str, refusal: Option<WebError>) -> WebError {
  refusal.unwrap_or_else(|| {
    WebError::new(format!("Browser navigation failed for {url}: {error_text}"), Failure::Transport)
      .with_url(url)
  })
}

/// Whether a request the guard would not let through ends the whole read: an address refused fails the page
/// wherever it stands, while a name with no address fails only its own request unless it is the main frame's
/// own document.
pub fn fails_the_page(
  refusal: &WebError,
  resource_type: Option<&str>,
  frame: Option<&str>,
  main_frame: Option<&str>,
) -> bool {
  refusal.kind != Failure::Transport
    || (resource_type == Some("Document") && frame.is_some() && frame == main_frame)
}

/// Watches what a page stands at until `arrived` takes it or `rested` says the page has come to rest; whether
/// it arrived. A page that has come to rest is read once more after it did, so a check that moved on just
/// before resting is taken. A read that fails — the page between two documents — is read again on the next
/// turn.
pub async fn await_arrival<Read, Fut>(
  mut read: Read,
  arrived: &Arrived,
  rested: impl Fn() -> bool,
  clock: &dyn crate::web::clock::Clock,
) -> bool
where
  Read: FnMut() -> Fut,
  Fut: Future<Output = Result<(String, String), WebError>>,
{
  loop {
    let at_rest = rested();

    if let Ok((url, title)) = read().await
      && arrived(&url, &title)
    {
      return true;
    }

    if at_rest {
      return false;
    }

    clock.sleep(ARRIVAL_POLL_MS).await;
  }
}

/// The answer the main frame's document came with.
#[derive(Clone, Debug)]
pub struct DocumentResponse {
  pub status: u16,
  pub mime_type: String,
}

/// What a page's events have shown of the navigation under way, whichever browser shows it: its document's
/// answer and a refusal of the guard's.
#[derive(Default)]
pub struct PageWatch {
  pub response: Mutex<Option<DocumentResponse>>,
  pub blocked: Mutex<Option<WebError>>,
  pub blocked_now: tokio::sync::Notify,
}

impl PageWatch {
  /// The guard's refusal recorded for the navigation under way.
  pub fn blocked(&self) -> Option<WebError> {
    locked(&self.blocked).clone()
  }

  /// What a navigation's wait ends with once a refusal was told: the refusal recorded.
  pub fn refusal(&self, url: &str) -> WebError {
    self.blocked().unwrap_or_else(|| {
      WebError::new(format!("Browser navigation to {url} was refused"), Failure::Blocked).with_url(url)
    })
  }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether a request's address is a web address the guard checks; every other scheme passes as the page's own.
fn is_web(url: &str) -> bool {
  let lower = url.get(..6).unwrap_or(url).to_ascii_lowercase();

  lower.starts_with("http:") || lower.starts_with("https:")
}

// Whether a browser's page reads, by the same classification every page read passes, as a challenge standing
// in place of its site's page (docs/web-access.md, 「How a fetched page is classified」).
fn stands_a_challenge(document: &BrowserDocument, requested_url: &str) -> bool {
  classify(&Fetched {
    status: document.status,
    content_type: &document.content_type,
    body: &document.html,
    title: Some(&document.title),
    visible_text: Some(&document.text),
    requested_url: Some(requested_url),
    final_url: Some(&document.url),
  })
  .is_challenge()
}

fn snapshot_expression() -> String {
  format!(
    "(() => ({{
      url: location.href,
      title: document.title || \"\",
      contentType: document.contentType || \"text/html\",
      text: (document.body ? document.body.innerText : \"\").slice(0, {SNAPSHOT_CHARS}),
      html: (document.documentElement ? document.documentElement.outerHTML : \"\").slice(0, {SNAPSHOT_CHARS}),
      links: Array.from(document.links).slice(0, {LINK_CAP}).flatMap(link => {{
        const text = (link.innerText || link.textContent || \"\").trim().slice(0, {LINK_FIELD_CAP});
        const url = (link.href || \"\").slice(0, {LINK_FIELD_CAP});
        return text && url && !url.startsWith(\"javascript:\") ? [{{ text, url }}] : [];
      }}),
      userAgent: navigator.userAgent,
      webdriver: navigator.webdriver === true,
      truncated: (document.body ? document.body.innerText.length : 0) > {SNAPSHOT_CHARS} ||
        (document.documentElement ? document.documentElement.outerHTML.length : 0) > {SNAPSHOT_CHARS} ||
        document.links.length > {LINK_CAP}
    }}))()"
  )
}

// What the page stands at once its document is complete, or null while it is still loading.
const WHERE: &str =
  "document.readyState === \"complete\" ? { url: location.href, title: document.title } : null";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
  url: String,
  #[serde(default)]
  title: String,
  #[serde(default)]
  content_type: String,
  text: String,
  html: String,
  #[serde(default)]
  links: Vec<Link>,
  #[serde(default)]
  user_agent: String,
  #[serde(default)]
  webdriver: bool,
  #[serde(default)]
  truncated: bool,
}

// A read counted as using the browser for as long as it stands, however it ends: its caller may let go of it
// part way, and the browser then becomes idle all the same.
struct Using<'a> {
  browser: &'a Browser,
}

impl Drop for Using<'_> {
  fn drop(&mut self) {
    let inner = &self.browser.inner;

    if inner.busy.fetch_sub(1, Ordering::SeqCst) == 1 && !inner.closing.load(Ordering::SeqCst) {
      self.browser.arm_idle();
    }
  }
}

type Held = Option<Box<dyn Running>>;

struct Inner {
  guard: UrlGuard,
  product: Box<dyn Product>,
  // Held while one piece of work drives the browser, so one navigation runs at a time.
  running: tokio::sync::Mutex<Held>,
  busy: AtomicUsize,
  idle: Mutex<Option<JoinHandle<()>>>,
  closing: AtomicBool,
}

/// A real browser of one product, started when first asked and ended when closed or idle.
#[derive(Clone)]
pub struct Browser {
  inner: Arc<Inner>,
}

impl Browser {
  pub fn new(guard: UrlGuard, product: Box<dyn Product>) -> Self {
    Self {
      inner: Arc::new(Inner {
        guard,
        product,
        running: tokio::sync::Mutex::new(None),
        busy: AtomicUsize::new(0),
        idle: Mutex::new(None),
        closing: AtomicBool::new(false),
      }),
    }
  }

  // The browser this work drives, started where none runs.
  async fn ensure<'a>(&self, running: &'a mut Held) -> WebResult<&'a dyn Running> {
    if self.inner.closing.load(Ordering::SeqCst) {
      return Err(WebError::new("The browser session is closing", Failure::BrowserUnavailable));
    }

    if running.as_ref().is_some_and(|held| !held.is_open())
      && let Some(gone) = running.take()
    {
      gone.shut_down().await;
    }

    if running.is_none() {
      *running = Some(self.inner.product.start(&self.inner.guard).await?);
    }

    Ok(running.as_deref().expect("a browser was just started"))
  }

  async fn snapshot(running: &dyn Running) -> WebResult<BrowserDocument> {
    let raw = running.evaluate(&snapshot_expression()).await?;
    let snapshot: Snapshot = serde_json::from_value(raw).map_err(|_| {
      WebError::new("The browser returned an invalid document snapshot", Failure::Transport)
    })?;
    let response = locked(&running.watch().response).clone();

    Ok(BrowserDocument {
      url: snapshot.url,
      status: response.as_ref().map_or(0, |response| response.status),
      content_type: response
        .as_ref()
        .map(|response| response.mime_type.clone())
        .filter(|mime| !mime.is_empty())
        .unwrap_or(if snapshot.content_type.is_empty() {
          "text/html".into()
        } else {
          snapshot.content_type
        }),
      title: snapshot.title,
      text: snapshot.text,
      html: snapshot.html,
      links: snapshot.links,
      user_agent: snapshot.user_agent,
      webdriver: snapshot.webdriver,
      truncated: snapshot.truncated,
      challenge: ChallengeMark::NotAttempted,
    })
  }

  // The page opened, watched to its arrival if one was named, and read once it holds still.
  async fn read_opened(
    &self,
    running: &dyn Running,
    url: &str,
    options: &FetchOptions,
  ) -> WebResult<BrowserDocument> {
    let watch = running.watch();

    *locked(&watch.response) = None;
    *locked(&watch.blocked) = None;

    running.open(url).await?;

    if let Some(refusal) = watch.blocked() {
      return Err(refusal);
    }

    if let Some(arrived) = &options.arrived {
      let read = || async {
        let page = running.evaluate(WHERE).await?;

        match (page["url"].as_str(), page["title"].as_str()) {
          (Some(url), Some(title)) => Ok((url.to_owned(), title.to_owned())),
          _ => Err(WebError::new("the page is still loading", Failure::Transport)),
        }
      };

      // A browser gone ends the watch too; the snapshot below then fails as the connection's end.
      await_arrival(
        read,
        arrived,
        || running.rested() || !running.is_open(),
        &crate::web::clock::SystemClock,
      )
      .await;

      if let Some(refusal) = watch.blocked() {
        return Err(refusal);
      }
    }

    // Dynamic pages often fill their root just after load. Read as soon as two snapshots agree, and take the
    // snapshot standing at the settle's end of a page whose own text never stops changing; never a
    // network-idle wait that advertisements can hold open.
    let mut prior = String::new();
    let mut document = Self::snapshot(running).await?;
    let settle_until = now_ms() + SETTLE_MS;

    while now_ms() + SETTLE_POLL_MS < settle_until {
      tokio::time::sleep(Duration::from_millis(SETTLE_POLL_MS)).await;

      let next = Self::snapshot(running).await?;
      let marker = format!("{}\0{}\0{}", next.url, next.title, next.text);

      document = next;

      if marker == prior {
        break;
      }

      prior = marker;
    }

    if let Some(refusal) = watch.blocked() {
      return Err(refusal);
    }

    Ok(document)
  }

  async fn navigate(
    &self,
    running: &dyn Running,
    url: &str,
    options: &FetchOptions,
  ) -> WebResult<BrowserDocument> {
    (self.inner.guard)(url.to_owned()).await?;
    let mut document = self.read_opened(running, url, options).await?;

    (self.inner.guard)(document.url.clone()).await?;

    // One attempt at a challenge met in the page, then the address opened once more: the read's one retry,
    // after the host's turn in its queue as every top-level navigation takes
    // (docs/web-access.md, 「When the browser meets a challenge」). A refusal whose page holds no challenge is
    // answered as it stands, and a challenge that stands after this is the caller's to refuse.
    if options.solve_challenge && stands_a_challenge(&document, url) {
      challenge::attempt(running, &*options.clock).await;

      if let Some(pace) = &options.pace {
        pace(url.to_owned()).await?;
      }

      document = self.read_opened(running, url, options).await?;
      (self.inner.guard)(document.url.clone()).await?;
      document.challenge = if stands_a_challenge(&document, url) {
        ChallengeMark::Remains
      } else {
        ChallengeMark::Cleared
      };
    }

    Ok(document)
  }

  fn arm_idle(&self) {
    let inner = Arc::downgrade(&self.inner);
    let idle = tokio::spawn(async move {
      tokio::time::sleep(Duration::from_millis(idle_ms())).await;

      let Some(inner) = inner.upgrade() else {
        return;
      };

      if inner.busy.load(Ordering::SeqCst) > 0 || inner.closing.load(Ordering::SeqCst) {
        return;
      }

      let mut running = inner.running.lock().await;

      if inner.busy.load(Ordering::SeqCst) == 0
        && let Some(gone) = running.take()
      {
        gone.shut_down().await;
      }
    });

    *locked(&self.inner.idle) = Some(idle);
  }

  /// One page read as the browser renders it, behind whatever this browser is already doing, counted so the
  /// browser ends once it has stood unused for the idle length.
  pub async fn read(&self, url: &str, options: FetchOptions) -> WebResult<BrowserDocument> {
    self.inner.busy.fetch_add(1, Ordering::SeqCst);

    let _using = Using { browser: self };

    if let Some(idle) = locked(&self.inner.idle).take() {
      idle.abort();
    }

    let mut running = self.inner.running.lock().await;
    let held = self.ensure(&mut running).await?;

    self.navigate(held, url, &options).await
  }

  /// The browser ended for good, with everything started for it.
  pub async fn shut(&self) {
    self.inner.closing.store(true, Ordering::SeqCst);

    if let Some(idle) = locked(&self.inner.idle).take() {
      idle.abort();
    }

    if let Some(gone) = self.inner.running.lock().await.take() {
      gone.shut_down().await;
    }
  }
}

impl BrowserFetcher for Browser {
  fn fetch(
    &self,
    url: String,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>> {
    Box::pin(async move { self.read(&url, options).await })
  }

  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    Box::pin(self.shut())
  }
}
