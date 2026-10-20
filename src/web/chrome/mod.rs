//! The real Chrome a read falls back to (docs/web-access.md, 「Direct collection and real Chrome」): standard
//! Chrome, non-headless, on a private display of its own, with a fresh temporary profile, every request it
//! makes sent through a proxy that connects only to an address the caller's check has just passed, and every
//! request a page makes paused and checked the same way before Chrome sends it. One navigation at a time; a
//! Chrome nothing has used for three minutes ends and gives its machine place back.

pub mod cdp;
pub mod detached;
pub mod display;
pub mod places;
pub mod profile;
pub mod proxy;

use std::future::Future;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::web::clock::now_ms;
use crate::web::error::{Failure, WebError, WebResult};

/// A page as Chrome rendered it.
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
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Link {
  pub text: String,
  pub url: String,
}

/// The check every request of this Chrome passes: the address the host may be connected at, none on a
/// trusted road that checks nothing, or a refusal.
pub type UrlGuard =
  Arc<dyn Fn(String, u64) -> Pin<Box<dyn Future<Output = WebResult<Option<IpAddr>>> + Send>> + Send + Sync>;

/// Whether the address a page stands at, with the document's title, is the page that was asked for, for a
/// site whose own browser check runs its script and moves on to the page by itself.
pub type Arrived = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// How one page is read, where its consumer knows more of its site than this layer does.
#[derive(Clone, Default)]
pub struct FetchOptions {
  pub arrived: Option<Arrived>,
}

/// What reads a page in a real browser: Chrome, or a test's stand-in.
pub trait BrowserFetcher: Send + Sync {
  fn fetch(
    &self,
    url: String,
    deadline_ms: u64,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>>;
  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

const CLOSE_WAIT: Duration = Duration::from_secs(3);
const PROXY_CHECK_MS: u64 = 10_000;
const SNAPSHOT_CHARS: usize = 4_000_000;
const LINK_CAP: usize = 1_000;
const LINK_FIELD_CAP: usize = 2_000;
const ARRIVAL_CAP_MS: u64 = 10_000;
const ARRIVAL_POLL_MS: u64 = 250;

fn idle_ms() -> u64 {
  std::env::var("BMO_CHROME_IDLE_SECONDS")
    .ok()
    .and_then(|named| named.parse::<f64>().ok())
    .filter(|seconds| *seconds > 0.0)
    .map_or(180_000, |seconds| (seconds * 1_000.0) as u64)
}

/// The arguments Chrome is started with on its private display.
pub fn launch_arguments(chrome: &str, profile: &Path, port: u16, proxy_port: u16) -> Vec<String> {
  let mut arguments = vec![
    chrome.to_owned(),
    format!("--user-data-dir={}", profile.display()),
    "--remote-debugging-address=127.0.0.1".into(),
    format!("--remote-debugging-port={port}"),
    "--no-first-run".into(),
    "--no-default-browser-check".into(),
    "--disable-background-networking".into(),
    "--disable-component-update".into(),
    "--disable-default-apps".into(),
    "--disable-sync".into(),
    "--disable-breakpad".into(),
    "--disable-dev-shm-usage".into(),
  ];

  arguments.extend(detached::DETACHED.detached_arguments.iter().cloned());
  arguments.extend(proxy::chrome_arguments(proxy_port));
  arguments.push("about:blank".into());
  arguments
}

/// The environment Chrome is started with: detached from every person's session, on its own display, with
/// its config, cache and data under the profile it is removed with.
pub fn launch_environment(
  base: impl IntoIterator<Item = (String, String)>,
  display: &str,
  xauthority: &Path,
  runtime_dir: &Path,
  profile: &Path,
) -> std::collections::BTreeMap<String, String> {
  let mut environment = display::detached_environment(base, display, xauthority, runtime_dir);

  environment.insert("XDG_CACHE_HOME".into(), profile.join("cache").to_string_lossy().into_owned());
  environment.insert("XDG_CONFIG_HOME".into(), profile.join("config").to_string_lossy().into_owned());
  environment.insert("XDG_DATA_HOME".into(), profile.join("data").to_string_lossy().into_owned());
  environment
}

/// What a navigation that came back with an error text actually was: the guard's refusal wherever one was
/// recorded — Chrome's answer to a refused request can reach the caller before the refusal is raised — and a
/// transport failure only where none was.
pub fn navigation_failure(url: &str, error_text: &str, refusal: Option<WebError>) -> WebError {
  refusal.unwrap_or_else(|| {
    WebError::new(format!("Chrome navigation failed for {url}: {error_text}"), Failure::Transport)
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

/// Watches what a page stands at until `arrived` takes it or `until_ms` comes; whether it arrived. A read that
/// fails — the page between two documents — is read again on the next turn.
pub async fn await_arrival<Read, Fut>(
  mut read: Read,
  arrived: &Arrived,
  until_ms: u64,
  clock: &dyn crate::web::clock::Clock,
) -> bool
where
  Read: FnMut() -> Fut,
  Fut: Future<Output = Result<(String, String), WebError>>,
{
  loop {
    if let Ok((url, title)) = read().await
      && arrived(&url, &title)
    {
      return true;
    }

    if clock.now() + ARRIVAL_POLL_MS >= until_ms {
      return false;
    }

    clock.sleep(ARRIVAL_POLL_MS).await;
  }
}

#[derive(Clone, Debug)]
struct DocumentResponse {
  status: u16,
  mime_type: String,
}

// What the page's events have shown of the navigation under way.
#[derive(Default)]
struct PageWatch {
  response: Mutex<Option<DocumentResponse>>,
  blocked: Mutex<Option<WebError>>,
  blocked_now: tokio::sync::Notify,
  main_frame: Mutex<Option<String>>,
  navigation_deadline: AtomicU64,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// One Chrome as it runs, with everything started for it.
struct Running {
  child: Child,
  display: display::PrivateDisplay,
  profile: PathBuf,
  proxy: proxy::PinnedProxy,
  connection: cdp::Connection,
  watch: Arc<PageWatch>,
  _place: places::Place,
}

// A started browser, its page's DevTools connection and what it watches on the page.
type Launched = (Child, cdp::Connection, Arc<PageWatch>);

// A launch that failed: the browser's process where it had started, to be ended, and why.
struct LaunchFailure {
  child: Option<Child>,
  error: WebError,
}

fn failure(child: Option<Child>, error: WebError) -> Box<LaunchFailure> {
  Box::new(LaunchFailure { child, error })
}

impl Running {
  fn sender(&self) -> &cdp::Sender {
    &self.connection.sender
  }
}

struct Inner {
  guard: UrlGuard,
  chrome: String,
  // Held while one piece of work drives the browser, so one navigation runs at a time.
  running: tokio::sync::Mutex<Option<Running>>,
  busy: AtomicUsize,
  idle: Mutex<Option<JoinHandle<()>>>,
  closing: AtomicBool,
}

/// Standard Chrome, started when first asked and ended when closed or idle.
#[derive(Clone)]
pub struct Chrome {
  inner: Arc<Inner>,
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

fn free_loopback_port() -> WebResult<u16> {
  std::net::TcpListener::bind(("127.0.0.1", 0))
    .and_then(|holder| holder.local_addr())
    .map(|address| address.port())
    .map_err(|_| {
      WebError::new("Could not reserve a loopback Chrome debugging port", Failure::BrowserUnavailable)
    })
}

impl Chrome {
  pub fn new(guard: UrlGuard) -> Self {
    Self::at(
      guard,
      std::env::var("BMO_CHROME_PATH").unwrap_or_else(|_| "/usr/bin/google-chrome".into()),
    )
  }

  pub fn at(guard: UrlGuard, chrome: String) -> Self {
    Self {
      inner: Arc::new(Inner {
        guard,
        chrome,
        running: tokio::sync::Mutex::new(None),
        busy: AtomicUsize::new(0),
        idle: Mutex::new(None),
        closing: AtomicBool::new(false),
      }),
    }
  }

  async fn start(&self, deadline_ms: u64) -> WebResult<Running> {
    let setpriv = display::setpriv();

    if !Path::new(&self.inner.chrome).exists() || !Path::new(&setpriv).exists() {
      return Err(WebError::new(
        format!(
          "Standard Chrome or its parent-death guard is unavailable ({}, {setpriv})",
          self.inner.chrome
        ),
        Failure::BrowserUnavailable,
      ));
    }

    let paths = places::place_paths();
    let place = places::take_any(&paths, deadline_ms).await.ok_or_else(|| {
      WebError::new(
        format!("All {} Chrome places on this machine stayed taken until the deadline", paths.len()),
        Failure::BrowserUnavailable,
      )
    })?;
    let temp = std::env::temp_dir();

    profile::reap_stale(&temp, now_ms(), &profile::LiveProcesses).await;

    let unavailable = |why: std::io::Error| {
      WebError::new(format!("Chrome's profile could not be made: {why}"), Failure::BrowserUnavailable)
    };
    let profile_dir = temp.join(format!("{}{:016x}", profile::PREFIX, rand::random::<u64>()));

    std::fs::create_dir(&profile_dir).map_err(unavailable)?;
    std::fs::write(profile_dir.join(profile::OWNER_FILE), format!("{}\n", std::process::id()))
      .map_err(unavailable)?;

    // What was started is taken down again where the start goes no further.
    let mut display = match display::PrivateDisplay::start(&profile_dir, deadline_ms).await {
      Ok(display) => display,
      Err(error) => {
        profile::remove(&profile_dir, &profile::LiveProcesses).await;
        return Err(error);
      }
    };
    let guard = self.inner.guard.clone();
    let resolver: proxy::Resolver = Arc::new(move |url: String| {
      let guard = guard.clone();

      Box::pin(async move { guard(url, now_ms() + PROXY_CHECK_MS).await.map_err(|error| error.message) })
    });
    let proxy = match proxy::start(resolver).await {
      Ok(proxy) => proxy,
      Err(why) => {
        display.stop().await;
        profile::remove(&profile_dir, &profile::LiveProcesses).await;
        return Err(WebError::new(
          format!("Chrome's proxy could not start: {why}"),
          Failure::BrowserUnavailable,
        ));
      }
    };
    let started = self.launch(&setpriv, &profile_dir, &display, &proxy, deadline_ms).await;

    match started {
      Ok((child, connection, watch)) => Ok(Running {
        child,
        display,
        profile: profile_dir,
        proxy,
        connection,
        watch,
        _place: place,
      }),
      Err(failed) => {
        let LaunchFailure { child, error } = *failed;

        if let Some(mut child) = child {
          let _ = child.kill().await;
        }
        display.stop().await;
        proxy.stop();
        profile::remove(&profile_dir, &profile::LiveProcesses).await;
        Err(error)
      }
    }
  }

  // The browser started on its display behind its proxy, and its page's DevTools connection opened.
  async fn launch(
    &self,
    setpriv: &str,
    profile_dir: &Path,
    display: &display::PrivateDisplay,
    proxy: &proxy::PinnedProxy,
    deadline_ms: u64,
  ) -> Result<Launched, Box<LaunchFailure>> {
    let port = free_loopback_port().map_err(|error| failure(None, error))?;
    let environment = launch_environment(
      std::env::vars(),
      &display.display,
      &display.xauthority,
      &display.runtime_dir,
      profile_dir,
    );
    let mut child = Command::new(setpriv)
      .args(["--pdeathsig", "SIGKILL"])
      .args(launch_arguments(&self.inner.chrome, profile_dir, port, proxy.port))
      .env_clear()
      .envs(environment)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      // Chrome's own diagnostics name paths and desktop details, which belong to the owning process and never
      // to an agent; they are dropped.
      .stderr(Stdio::null())
      .kill_on_drop(true)
      .spawn()
      .map_err(|why| failure(None, WebError::new(format!("Standard Chrome could not start: {why}"), Failure::BrowserUnavailable)))?;
    let local = reqwest::Client::new();
    let mut listening = false;

    while now_ms() < deadline_ms {
      if let Ok(Some(status)) = child.try_wait() {
        return Err(failure(
          None,
          WebError::new(
            format!("Standard Chrome exited during startup ({status})"),
            Failure::BrowserUnavailable,
          ),
        ));
      }

      // The endpoint appears after Chrome has brought up the new profile.
      let asked = local
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .timeout(Duration::from_millis(250))
        .send()
        .await;

      if asked.is_ok_and(|answer| answer.status().is_success()) {
        listening = true;
        break;
      }

      tokio::time::sleep(Duration::from_millis(25)).await;
    }

    if !listening {
      return Err(failure(Some(child), cdp::deadline_error("startup")));
    }

    let listed = local
      .get(format!("http://127.0.0.1:{port}/json/list"))
      .timeout(Duration::from_secs(2))
      .send()
      .await;
    let targets: Vec<Value> = match listed {
      Ok(answer) => answer.json().await.unwrap_or_default(),
      Err(_) => Vec::new(),
    };
    let Some(address) = targets
      .iter()
      .find(|target| target["type"] == "page")
      .and_then(|target| target["webSocketDebuggerUrl"].as_str())
      .map(str::to_owned)
    else {
      return Err(failure(
        Some(child),
        WebError::new("Chrome exposed no page target", Failure::BrowserUnavailable),
      ));
    };
    let watch = Arc::new(PageWatch::default());
    let handler_watch = watch.clone();
    let guard = self.inner.guard.clone();
    let handle: cdp::EventHandler = Arc::new(move |sender, method, params| match method {
      "Network.responseReceived" => {
        let main = locked(&handler_watch.main_frame).clone();

        if params["type"] == "Document" && params["frameId"].as_str() == main.as_deref() {
          let response = &params["response"];

          if let (Some(_), Some(status)) = (response["url"].as_str(), response["status"].as_u64()) {
            *locked(&handler_watch.response) = Some(DocumentResponse {
              status: status as u16,
              mime_type: response["mimeType"].as_str().unwrap_or("").to_owned(),
            });
          }
        }
      }
      "Fetch.requestPaused" => {
        let sender = sender.clone();
        let watch = handler_watch.clone();
        let guard = guard.clone();
        let params = params.clone();

        tokio::spawn(async move { answer_paused(&sender, &watch, &guard, &params).await });
      }
      _ => {}
    });
    let connection = match cdp::Connection::open(&address, deadline_ms, handle).await {
      Ok(connection) => connection,
      Err(error) => return Err(failure(Some(child), error)),
    };
    let sender = connection.sender.clone();
    let enabled = async {
      sender.command("Page.enable", json!({}), deadline_ms).await?;
      sender.command("Runtime.enable", json!({}), deadline_ms).await?;
      sender.command("Network.enable", json!({}), deadline_ms).await?;
      sender
        .command(
          "Fetch.enable",
          json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] }),
          deadline_ms,
        )
        .await?;

      let tree = sender.command("Page.getFrameTree", json!({}), deadline_ms).await?;

      tree["frameTree"]["frame"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| WebError::new("Chrome exposed no main page frame", Failure::BrowserUnavailable))
    };

    match enabled.await {
      Ok(frame) => {
        *locked(&watch.main_frame) = Some(frame);
        Ok((child, connection, watch))
      }
      Err(error) => Err(failure(Some(child), error)),
    }
  }

  // The Chrome this work drives, started where none runs.
  async fn ensure<'a>(&self, running: &'a mut Option<Running>, deadline_ms: u64) -> WebResult<&'a Running> {
    if self.inner.closing.load(Ordering::SeqCst) {
      return Err(WebError::new("Chrome session is closing", Failure::BrowserUnavailable));
    }

    if running.as_ref().is_some_and(|held| !held.connection.is_open())
      && let Some(gone) = running.take()
    {
      shut_down(gone).await;
    }

    if running.is_none() {
      *running = Some(self.start(deadline_ms).await?);
    }

    Ok(running.as_ref().expect("a Chrome was just started"))
  }

  async fn snapshot(
    sender: &cdp::Sender,
    watch: &PageWatch,
    deadline_ms: u64,
  ) -> WebResult<BrowserDocument> {
    let raw = sender
      .command(
        "Runtime.evaluate",
        json!({ "expression": snapshot_expression(), "returnByValue": true, "awaitPromise": true }),
        deadline_ms,
      )
      .await?;
    let snapshot: Snapshot = serde_json::from_value(raw["result"]["value"].clone())
      .map_err(|_| WebError::new("Chrome returned an invalid document snapshot", Failure::Transport))?;
    let response = locked(&watch.response).clone();

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
    })
  }

  async fn navigate(
    &self,
    running: &Running,
    url: &str,
    deadline_ms: u64,
    options: &FetchOptions,
  ) -> WebResult<BrowserDocument> {
    let sender = running.sender();
    let watch = &running.watch;

    (self.inner.guard)(url.to_owned(), deadline_ms).await?;
    watch.navigation_deadline.store(deadline_ms, Ordering::SeqCst);
    *locked(&watch.response) = None;
    *locked(&watch.blocked) = None;

    let mut events = running.connection.events.subscribe();
    let blocked = || locked(&watch.blocked).clone();
    let navigated = tokio::select! {
      answer = sender.command("Page.navigate", json!({ "url": url }), deadline_ms) => answer?,
      () = watch.blocked_now.notified() => return Err(blocked().unwrap_or_else(|| cdp::deadline_error("navigation"))),
    };

    if let Some(error_text) = navigated.get("errorText").and_then(Value::as_str) {
      return Err(navigation_failure(url, error_text, blocked()));
    }

    tokio::select! {
      loaded = cdp::event(&mut events, "Page.loadEventFired", deadline_ms) => { loaded?; }
      () = watch.blocked_now.notified() => {}
    }

    if let Some(refusal) = blocked() {
      return Err(refusal);
    }

    if let Some(arrived) = &options.arrived {
      let read = || async {
        let expression =
          "document.readyState === \"complete\" ? { url: location.href, title: document.title } : null";
        let raw = sender
          .command(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true }),
            deadline_ms.min(now_ms() + 2_000),
          )
          .await?;
        let page = &raw["result"]["value"];

        match (page["url"].as_str(), page["title"].as_str()) {
          (Some(url), Some(title)) => Ok((url.to_owned(), title.to_owned())),
          _ => Err(WebError::new("the page is still loading", Failure::Transport)),
        }
      };

      await_arrival(
        read,
        arrived,
        deadline_ms.min(now_ms() + ARRIVAL_CAP_MS),
        &crate::web::clock::SystemClock,
      )
      .await;

      if let Some(refusal) = blocked() {
        return Err(refusal);
      }
    }

    // Dynamic pages often fill their root just after load. Stop as soon as two bounded snapshots agree; never
    // a network-idle wait that advertisements can hold open.
    let mut prior = String::new();
    let mut document = Self::snapshot(sender, watch, deadline_ms).await?;
    let settle_until = deadline_ms.min(now_ms() + 2_000);

    while now_ms() + 150 < settle_until {
      tokio::time::sleep(Duration::from_millis(150)).await;

      let next = Self::snapshot(sender, watch, deadline_ms).await?;
      let marker = format!("{}\0{}\0{}", next.url, next.title, next.text);

      document = next;

      if marker == prior {
        break;
      }

      prior = marker;
    }

    if let Some(refusal) = blocked() {
      return Err(refusal);
    }

    (self.inner.guard)(document.url.clone(), deadline_ms).await?;
    Ok(document)
  }

  // One piece of work behind whatever this browser is already doing, counted so the Chrome ends once it has
  // stood unused for the idle length.
  async fn queued<T>(&self, work: impl AsyncFnOnce(&mut Option<Running>) -> WebResult<T>) -> WebResult<T> {
    self.inner.busy.fetch_add(1, Ordering::SeqCst);

    if let Some(idle) = locked(&self.inner.idle).take() {
      idle.abort();
    }

    let result = {
      let mut running = self.inner.running.lock().await;

      work(&mut running).await
    };

    if self.inner.busy.fetch_sub(1, Ordering::SeqCst) == 1 && !self.inner.closing.load(Ordering::SeqCst) {
      self.arm_idle();
    }

    result
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
        shut_down(gone).await;
      }
    });

    *locked(&self.inner.idle) = Some(idle);
  }

  /// One page read as Chrome renders it.
  pub async fn read(
    &self,
    url: &str,
    deadline_ms: u64,
    options: FetchOptions,
  ) -> WebResult<BrowserDocument> {
    let this = self.clone();

    self
      .queued(async move |running: &mut Option<Running>| {
        let held = this.ensure(running, deadline_ms).await?;

        this.navigate(held, url, deadline_ms, &options).await
      })
      .await
  }

  /// The processes and places this Chrome holds now: its pid, its display's pid and number, and its profile.
  pub async fn owned(&self) -> (Option<u32>, Option<u32>, Option<String>, Option<PathBuf>) {
    let running = self.inner.running.lock().await;

    running.as_ref().map_or((None, None, None, None), |held| {
      (
        held.child.id(),
        held.display.pid(),
        Some(held.display.display.clone()),
        Some(held.profile.clone()),
      )
    })
  }

  /// The Chrome ended for good, with everything started for it.
  pub async fn shut(&self) {
    self.inner.closing.store(true, Ordering::SeqCst);

    if let Some(idle) = locked(&self.inner.idle).take() {
      idle.abort();
    }

    if let Some(gone) = self.inner.running.lock().await.take() {
      shut_down(gone).await;
    }
  }
}

// What a paused request is answered with: let through where the guard passes it, failed where it does not —
// the whole read with it where the refusal fails the page.
async fn answer_paused(sender: &cdp::Sender, watch: &PageWatch, guard: &UrlGuard, params: &Value) {
  let (Some(request_id), Some(url)) = (params["requestId"].as_str(), params["request"]["url"].as_str())
  else {
    return;
  };
  let web = url.starts_with("http:")
    || url.starts_with("https:")
    || url.starts_with("HTTP:")
    || url.starts_with("HTTPS:");
  let checked = if web {
    guard(url.to_owned(), watch.navigation_deadline.load(Ordering::SeqCst))
      .await
      .map(|_| ())
  } else {
    Ok(())
  };

  match checked {
    Ok(()) => {
      let _ = sender
        .command("Fetch.continueRequest", json!({ "requestId": request_id }), now_ms() + 5_000)
        .await;
    }
    Err(refusal) => {
      let main = locked(&watch.main_frame).clone();

      if !fails_the_page(
        &refusal,
        params["resourceType"].as_str(),
        params["frameId"].as_str(),
        main.as_deref(),
      ) {
        let _ = sender
          .command(
            "Fetch.failRequest",
            json!({ "requestId": request_id, "errorReason": "NameNotResolved" }),
            now_ms() + 2_000,
          )
          .await;
        return;
      }

      *locked(&watch.blocked) = Some(refusal);
      let _ = sender
        .command(
          "Fetch.failRequest",
          json!({ "requestId": request_id, "errorReason": "BlockedByClient" }),
          now_ms() + 2_000,
        )
        .await;
      watch.blocked_now.notify_waiters();
    }
  }
}

// Ends the Chrome and everything started for it, and gives its place back.
async fn shut_down(mut running: Running) {
  running.connection.sender.tell("Browser.close", json!({}));

  if let Some(pid) = running.child.id() {
    // SAFETY: a signal to the Chrome this browser started and still holds.
    unsafe {
      libc::kill(pid as i32, libc::SIGTERM);
    }
  }

  if tokio::time::timeout(CLOSE_WAIT, running.child.wait()).await.is_err() {
    let _ = running.child.kill().await;
  }

  running.display.stop().await;
  running.proxy.stop();

  // The browser's own exit does not end its GPU and utility helpers, which keep writing under the profile;
  // what is not gone here is left for the next start's reaper.
  let temp = std::env::temp_dir();
  let ours = running.profile.parent() == Some(temp.as_path())
    && running
      .profile
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| name.starts_with(profile::PREFIX));

  if ours {
    profile::remove(&running.profile, &profile::LiveProcesses).await;
  }
}

impl BrowserFetcher for Chrome {
  fn fetch(
    &self,
    url: String,
    deadline_ms: u64,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>> {
    Box::pin(async move { self.read(&url, deadline_ms, options).await })
  }

  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    Box::pin(self.shut())
  }
}
