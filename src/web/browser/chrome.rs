//! Standard Chrome on a fresh temporary profile (docs/web-access.md, 「How the browser is started」), the browser
//! of a machine that has no Firefox profile of the owner's: driven over its DevTools protocol, its page's
//! lifecycle read for the network-idle mark, and every request the page makes paused until the guard has
//! passed it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use tokio::process::{Child, Command};

use super::{
  Answer, Gesture, PageWatch, Product, Running, UrlGuard, display, fails_the_page, is_web, locked,
  navigation_failure, places, profile, proxy, remote,
};
use crate::web::clock::now_ms;
use crate::web::error::{Failure, WebError, WebResult};

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

  arguments.extend(super::detached::DETACHED.detached_arguments.iter().cloned());
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

// What Chrome's lifecycle events have shown of the main frame: the loader of the document last begun there,
// the one Chrome last marked network-idle, and whether a navigation is scheduled.
#[derive(Default)]
struct Lifecycle {
  page: PageWatch,
  main_frame: std::sync::Mutex<Option<String>>,
  document: std::sync::Mutex<Option<String>>,
  idle: std::sync::Mutex<Option<String>>,
  scheduled: AtomicBool,
}

impl Lifecycle {
  // Whether the main frame's document has come to rest: Chrome's network-idle mark given for the document
  // standing there, and no navigation scheduled from it.
  fn rested(&self) -> bool {
    let idle = locked(&self.idle).clone();
    let document = locked(&self.document).clone();

    idle.is_some() && (document.is_none() || idle == document) && !self.scheduled.load(Ordering::SeqCst)
  }

  // A lifecycle or navigation event of the page, recorded where it is the main frame's.
  fn observe(&self, method: &str, params: &Value) {
    let main = locked(&self.main_frame).clone();

    if params["frameId"].as_str() != main.as_deref() {
      return;
    }

    let loader = params["loaderId"].as_str().map(str::to_owned);

    match (method, params["name"].as_str()) {
      ("Page.lifecycleEvent", Some("init")) => {
        *locked(&self.document) = loader;
        *locked(&self.idle) = None;
        self.scheduled.store(false, Ordering::SeqCst);
      }
      ("Page.lifecycleEvent", Some("networkIdle")) => *locked(&self.idle) = loader,
      ("Page.frameScheduledNavigation", _) => self.scheduled.store(true, Ordering::SeqCst),
      ("Page.frameClearedScheduledNavigation", _) => self.scheduled.store(false, Ordering::SeqCst),
      _ => {}
    }
  }
}

// One Chrome as it runs, with everything started for it.
struct ChromeRunning {
  child: Child,
  display: display::PrivateDisplay,
  profile: PathBuf,
  proxy: proxy::PinnedProxy,
  connection: remote::Connection,
  watch: Arc<Lifecycle>,
  _place: places::Place,
}

// A started browser, its page's DevTools connection and what it watches on the page.
type Launched = (Child, remote::Connection, Arc<Lifecycle>);

// A launch that failed: the browser's process where it had started, to be ended, and why.
struct LaunchFailure {
  child: Option<Child>,
  error: WebError,
}

fn failure(child: Option<Child>, error: WebError) -> Box<LaunchFailure> {
  Box::new(LaunchFailure { child, error })
}

fn free_loopback_port() -> WebResult<u16> {
  std::net::TcpListener::bind(("127.0.0.1", 0))
    .and_then(|holder| holder.local_addr())
    .map(|address| address.port())
    .map_err(|_| {
      WebError::new("Could not reserve a loopback Chrome debugging port", Failure::BrowserUnavailable)
    })
}

/// Standard Chrome, by its executable.
pub struct Chrome {
  chrome: String,
}

impl Chrome {
  /// The machine's Chrome: `BMO_CHROME_PATH`, else the stable channel's own path.
  pub fn installed() -> Self {
    Self::at(std::env::var("BMO_CHROME_PATH").unwrap_or_else(|_| "/usr/bin/google-chrome".into()))
  }

  pub fn at(chrome: String) -> Self {
    Self { chrome }
  }

  async fn started(&self, guard: &UrlGuard) -> WebResult<Box<dyn Running>> {
    let setpriv = display::setpriv();

    if !Path::new(&self.chrome).exists() || !Path::new(&setpriv).exists() {
      return Err(WebError::new(
        format!("Standard Chrome or its parent-death guard is unavailable ({}, {setpriv})", self.chrome),
        Failure::BrowserUnavailable,
      ));
    }

    let place = places::take_any(&places::place_paths()).await.map_err(|why| {
      WebError::new(
        format!("No browser place on this machine can be taken: {why}"),
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
    let mut display = match display::PrivateDisplay::start(&profile_dir).await {
      Ok(display) => display,
      Err(error) => {
        profile::remove(&profile_dir, &profile::LiveProcesses).await;
        return Err(error);
      }
    };
    let proxy = match proxy::start(proxy::resolver_of(guard)).await {
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

    match self.launch(guard, &setpriv, &profile_dir, &display, &proxy).await {
      Ok((child, connection, watch)) => Ok(Box::new(ChromeRunning {
        child,
        display,
        profile: profile_dir,
        proxy,
        connection,
        watch,
        _place: place,
      })),
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
    guard: &UrlGuard,
    setpriv: &str,
    profile_dir: &Path,
    display: &display::PrivateDisplay,
    proxy: &proxy::PinnedProxy,
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
      .args(launch_arguments(&self.chrome, profile_dir, port, proxy.port))
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

    // The endpoint appears after Chrome has brought up the new profile: it is asked again until it answers,
    // or until Chrome has exited.
    loop {
      let asked = local.get(format!("http://127.0.0.1:{port}/json/version")).send();
      let answered = tokio::select! {
        exited = child.wait() => {
          let said = exited.map_or_else(|why| why.to_string(), |status| status.to_string());

          return Err(failure(
            None,
            WebError::new(format!("Standard Chrome exited during startup ({said})"), Failure::BrowserUnavailable),
          ));
        }
        asked = asked => asked,
      };

      if answered.is_ok_and(|answer| answer.status().is_success()) {
        break;
      }

      tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let listed = local.get(format!("http://127.0.0.1:{port}/json/list")).send().await;
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
    let watch = Arc::new(Lifecycle::default());
    let handler_watch = watch.clone();
    let guard = guard.clone();
    let handle: remote::EventHandler = Arc::new(move |sender, method, params| match method {
      "Network.responseReceived" => {
        let main = locked(&handler_watch.main_frame).clone();

        if params["type"] == "Document" && params["frameId"].as_str() == main.as_deref() {
          let response = &params["response"];

          if let (Some(_), Some(status)) = (response["url"].as_str(), response["status"].as_u64()) {
            *locked(&handler_watch.page.response) = Some(super::DocumentResponse {
              status: status as u16,
              mime_type: response["mimeType"].as_str().unwrap_or("").to_owned(),
            });
          }
        }
      }
      "Page.lifecycleEvent" | "Page.frameScheduledNavigation" | "Page.frameClearedScheduledNavigation" => {
        handler_watch.observe(method, params);
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
    let connection = match remote::Connection::open(&address, handle).await {
      Ok(connection) => connection,
      Err(error) => return Err(failure(Some(child), error)),
    };
    let sender = connection.sender.clone();
    let enabled = async {
      sender.command("Page.enable", json!({})).await?;
      sender.command("Page.setLifecycleEventsEnabled", json!({ "enabled": true })).await?;
      sender.command("Runtime.enable", json!({})).await?;
      sender.command("Network.enable", json!({})).await?;
      sender
        .command(
          "Fetch.enable",
          json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] }),
        )
        .await?;

      let tree = sender.command("Page.getFrameTree", json!({})).await?;

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
}

impl Product for Chrome {
  fn start<'a>(&'a self, guard: &'a UrlGuard) -> Answer<'a, WebResult<Box<dyn Running>>> {
    Box::pin(self.started(guard))
  }
}

// What a paused request is answered with: let through where the guard passes it, failed where it does not —
// the whole read with it where the refusal fails the page.
async fn answer_paused(sender: &remote::Sender, watch: &Lifecycle, guard: &UrlGuard, params: &Value) {
  let (Some(request_id), Some(url)) = (params["requestId"].as_str(), params["request"]["url"].as_str())
  else {
    return;
  };
  let checked = if is_web(url) { guard(url.to_owned()).await.map(|_| ()) } else { Ok(()) };

  match checked {
    Ok(()) => {
      let _ = sender.command("Fetch.continueRequest", json!({ "requestId": request_id })).await;
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
          )
          .await;
        return;
      }

      *locked(&watch.page.blocked) = Some(refusal);
      let _ = sender
        .command(
          "Fetch.failRequest",
          json!({ "requestId": request_id, "errorReason": "BlockedByClient" }),
        )
        .await;
      watch.page.blocked_now.notify_waiters();
    }
  }
}

impl Running for ChromeRunning {
  fn is_open(&self) -> bool {
    self.connection.is_open()
  }

  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let sender = &self.connection.sender;
      let page = &self.watch.page;
      let mut events = self.connection.events.subscribe();
      let navigated = tokio::select! {
        answer = sender.command("Page.navigate", json!({ "url": url })) => answer?,
        () = page.blocked_now.notified() => return Err(page.refusal(url)),
      };

      if let Some(error_text) = navigated.get("errorText").and_then(Value::as_str) {
        return Err(navigation_failure(url, error_text, page.blocked()));
      }

      tokio::select! {
        loaded = remote::event(&mut events, "Page.loadEventFired") => { loaded?; }
        () = page.blocked_now.notified() => {}
      }

      Ok(())
    })
  }

  fn evaluate<'a>(&'a self, expression: &'a str) -> Answer<'a, WebResult<Value>> {
    Box::pin(async move {
      let mut raw = self
        .connection
        .sender
        .command(
          "Runtime.evaluate",
          json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )
        .await?;

      Ok(raw["result"]["value"].take())
    })
  }

  // Pointer events through the input domain, so the widget takes them as a person's hand; the walk between
  // them is the caller's.
  fn gesture<'a>(&'a self, at: &'a Gesture) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let (kind, x, y) = match at {
        Gesture::Move { x, y } => ("mouseMoved", x, y),
        Gesture::Down { x, y } => ("mousePressed", x, y),
        Gesture::Up { x, y } => ("mouseReleased", x, y),
      };
      let (x, y) = super::whole(*x, *y);
      let held = kind == "mousePressed";

      self
        .connection
        .sender
        .command(
          "Input.dispatchMouseEvent",
          json!({
            "type": kind,
            "x": x,
            "y": y,
            "button": if kind == "mouseMoved" { "none" } else { "left" },
            "clickCount": if kind == "mouseMoved" { 0 } else { 1 },
            "buttons": if held { 1 } else { 0 },
          }),
        )
        .await
        .map(|_| ())
    })
  }

  // Words as one insert and Enter as its two key events, through the same trusted domain.
  fn keys<'a>(&'a self, text: &'a str, enter: bool) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let sender = &self.connection.sender;

      if !text.is_empty() {
        sender.command("Input.insertText", json!({ "text": text })).await?;
      }

      if enter {
        for kind in ["keyDown", "keyUp"] {
          sender
            .command(
              "Input.dispatchKeyEvent",
              json!({
                "type": kind,
                "key": "Enter",
                "code": "Enter",
                "windowsVirtualKeyCode": 13,
                "nativeVirtualKeyCode": 13,
              }),
            )
            .await?;
        }
      }

      Ok(())
    })
  }

  // The view as a PNG, through the page domain.
  fn screenshot<'a>(&'a self) -> Answer<'a, WebResult<Vec<u8>>> {
    Box::pin(async move {
      let shot = self
        .connection
        .sender
        .command("Page.captureScreenshot", json!({ "format": "png" }))
        .await?;
      let data = shot["data"]
        .as_str()
        .ok_or_else(|| WebError::new("Chrome answered no picture for the screen", Failure::Transport))?;
      let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| WebError::new("Chrome's picture did not read as base64", Failure::Transport))?;

      Ok(png)
    })
  }

  fn watch(&self) -> &PageWatch {
    &self.watch.page
  }

  fn rested(&self) -> bool {
    self.watch.rested() || !self.connection.sender.is_reading()
  }

  // Ends the Chrome and everything started for it, and gives its place back. Chrome is asked to close and sent
  // `SIGTERM`, on which it ends, and is waited for until it has.
  fn shut_down(self: Box<Self>) -> Answer<'static, ()> {
    Box::pin(async move {
      let mut running = *self;

      running.connection.sender.tell("Browser.close", json!({}));

      if let Some(pid) = running.child.id() {
        // SAFETY: a signal to the Chrome this browser started and still holds.
        unsafe {
          libc::kill(pid as i32, libc::SIGTERM);
        }
      }

      let _ = running.child.wait().await;

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
    })
  }
}
