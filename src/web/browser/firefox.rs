//! Firefox on a copy of the owner's own profile (docs/web-access.md, 「Which browser」 and 「How the browser is
//! started」), the browser of a machine where his Firefox profile stands, by his order of 2026-11-17 that the web
//! tools browse as he does.
//!
//! Each start copies his profile (the `copy` module) and runs Firefox on the copy alone, on a private display,
//! behind the guard's proxy, told through the copy's own user file to send everything through it. A snap's
//! Firefox is started inside a user scope of the snap's name, made through the user's service manager, since its
//! launcher would otherwise ask the session bus for one; the bus is cut only inside it. The browser is driven
//! over WebDriver BiDi on a loopback port it chooses and announces: every request a page makes is held until
//! the guard has passed it, and the mark an automated browser shows its pages is hidden from them before their
//! own scripts run. The snap's privileged launcher drops the parent-death signal, so a watcher beside it kills
//! Firefox's process group once the process that started it is gone.
//!
//! No maintained Rust client of WebDriver BiDi stands to build on, so its commands are written here as the JSON
//! objects the specification defines, over the same connection the DevTools protocol uses.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{Value, json};
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::task::JoinHandle;

use super::{
  Answer, DocumentResponse, Gesture, PageWatch, Product, Running, UrlGuard, copy, display, fails_the_page,
  is_web, locked, navigation_failure, places, profile, proxy, remote,
};
use crate::web::clock::now_ms;
use crate::web::error::{Failure, WebError, WebResult};

/// The words Firefox announces its WebDriver BiDi endpoint with on its error stream, the endpoint after them.
const ANNOUNCED: &str = "WebDriver BiDi listening on ";

/// Run in every page before its own scripts: the page's `navigator.webdriver` answers false, as a person's
/// browser answers it.
const UNMARKED: &str = "() => { Object.defineProperty(Navigator.prototype, 'webdriver', { get: () => false, configurable: true }); }";

/// The events the browser is asked for: each request's start, end and failure, and each navigation's start,
/// load, failure and turn into a download.
const EVENTS: [&str; 7] = [
  "network.beforeRequestSent",
  "network.responseCompleted",
  "network.fetchError",
  "browsingContext.navigationStarted",
  "browsingContext.load",
  "browsingContext.navigationFailed",
  "browsingContext.downloadWillBegin",
];

/// What a request's failure says where another navigation took its place, which is no failure of the page.
const SUPERSEDED: &str = "NS_BINDING_ABORTED";

/// How long the page's network stands quiet before it has come to rest.
const QUIET_MS: u64 = 500;

/// The watcher beside a Firefox: it waits for its standard input to close — the starting process ending it, or
/// that process gone — then kills the process group the Firefox leads, named by its first argument.
const WATCH: &str = "read -r _; kill -KILL -- \"-$1\" 2>/dev/null";

/// The user scope a snap's launcher expects to start in, for an executable a snap places in its own bin folder:
/// `snap.<snap>.<app>`, the app taken as the snap where the name gives none; none for any other executable.
pub fn snap_scope(executable: &str) -> String {
  match executable.strip_prefix("/snap/bin/") {
    Some(name) if !name.is_empty() && !name.contains('/') => match name.split_once('.') {
      Some((snap, app)) => format!("snap.{snap}.{app}"),
      None => format!("snap.{name}.{name}"),
    },
    _ => String::new(),
  }
}

/// The preferences one start lays over the copy, as the lines of its user file: the remote agent's
/// automation preferences left off, so the copy behaves as his own Firefox does; every request through the
/// proxy on `proxy_port`, loopback included, with no direct connection to fail over to, no DNS over HTTPS, no
/// name prefetching or speculative connection, and WebRTC through the proxy only; the cache and every download
/// inside the copy at `folder`, so they go with it and nothing lands in the owner's own download folder; no
/// session restored and no default-browser check; and its window, on a screen nobody looks at, behaving as
/// focused, as a person's window does.
pub fn start_preferences(proxy_port: u16, folder: &Path) -> String {
  let inside = |name: &str| Value::from(folder.join(name).to_string_lossy().into_owned());
  let (cache, downloads) = (inside("cache"), inside("downloads"));

  [
    "user_pref(\"remote.prefs.recommended\", false);".to_owned(),
    "user_pref(\"network.proxy.type\", 1);".to_owned(),
    "user_pref(\"network.proxy.http\", \"127.0.0.1\");".to_owned(),
    format!("user_pref(\"network.proxy.http_port\", {proxy_port});"),
    "user_pref(\"network.proxy.ssl\", \"127.0.0.1\");".to_owned(),
    format!("user_pref(\"network.proxy.ssl_port\", {proxy_port});"),
    "user_pref(\"network.proxy.socks\", \"\");".to_owned(),
    "user_pref(\"network.proxy.share_proxy_settings\", false);".to_owned(),
    "user_pref(\"network.proxy.no_proxies_on\", \"\");".to_owned(),
    "user_pref(\"network.proxy.allow_hijacking_localhost\", true);".to_owned(),
    "user_pref(\"network.proxy.failover_direct\", false);".to_owned(),
    "user_pref(\"network.trr.mode\", 5);".to_owned(),
    "user_pref(\"network.dns.disablePrefetch\", true);".to_owned(),
    "user_pref(\"network.prefetch-next\", false);".to_owned(),
    "user_pref(\"network.predictor.enabled\", false);".to_owned(),
    "user_pref(\"network.http.speculative-parallel-limit\", 0);".to_owned(),
    "user_pref(\"browser.urlbar.speculativeConnect.enabled\", false);".to_owned(),
    "user_pref(\"media.peerconnection.ice.proxy_only\", true);".to_owned(),
    format!("user_pref(\"browser.cache.disk.parent_directory\", {cache});"),
    "user_pref(\"browser.download.folderList\", 2);".to_owned(),
    format!("user_pref(\"browser.download.dir\", {downloads});"),
    "user_pref(\"browser.download.useDownloadDir\", true);".to_owned(),
    "user_pref(\"browser.download.manager.addToRecentDocs\", false);".to_owned(),
    "user_pref(\"browser.sessionstore.resume_from_crash\", false);".to_owned(),
    "user_pref(\"browser.startup.page\", 0);".to_owned(),
    "user_pref(\"browser.shell.checkDefaultBrowser\", false);".to_owned(),
    "user_pref(\"focusmanager.testmode\", true);".to_owned(),
  ]
  .join("\n")
    + "\n"
}

/// The environment Firefox starts in, and nothing of the caller's beyond its `PATH`, `HOME` and `LANG`: the
/// private display with its authority file, a runtime folder of its own, a session bus address in the copy's
/// `folder` that leads nowhere, Wayland turned off, and no crash reporter.
pub fn launch_environment(
  caller: impl Fn(&str) -> Option<String>,
  display: &str,
  xauthority: &Path,
  runtime_dir: &Path,
  folder: &Path,
) -> Vec<(String, String)> {
  vec![
    ("PATH".to_owned(), caller("PATH").unwrap_or_else(|| "/usr/bin:/bin".to_owned())),
    ("HOME".to_owned(), caller("HOME").unwrap_or_default()),
    ("LANG".to_owned(), caller("LANG").unwrap_or_else(|| "C.UTF-8".to_owned())),
    ("DISPLAY".to_owned(), display.to_owned()),
    ("XAUTHORITY".to_owned(), xauthority.display().to_string()),
    ("XDG_RUNTIME_DIR".to_owned(), runtime_dir.display().to_string()),
    (
      "DBUS_SESSION_BUS_ADDRESS".to_owned(),
      format!("unix:path={}", folder.join("no-bus").display()),
    ),
    ("DISABLE_WAYLAND".to_owned(), "1".to_owned()),
    ("MOZ_ENABLE_WAYLAND".to_owned(), "0".to_owned()),
    ("GDK_BACKEND".to_owned(), "x11".to_owned()),
    ("MOZ_CRASHREPORTER_DISABLE".to_owned(), "1".to_owned()),
  ]
}

/// The command line Firefox is started with on the copy at `profile`: in a user scope named `{scope}-{unique}`
/// where a scope is named, with the environment given and no other, with no remote of its own and a loopback
/// WebDriver BiDi endpoint on a port it chooses itself.
pub fn launch_line(
  firefox: &str,
  scope: &str,
  unique: &str,
  environment: &[(String, String)],
  profile: &Path,
) -> Vec<String> {
  let mut line = Vec::new();

  if !scope.is_empty() {
    line.extend(["systemd-run", "--user", "--scope", "--quiet", "--collect"].map(str::to_owned));
    line.push(format!("--unit={scope}-{unique}"));
    line.push("--".to_owned());
  }

  line.extend(["env", "-i"].map(str::to_owned));
  line.extend(environment.iter().map(|(name, value)| format!("{name}={value}")));
  line.push(firefox.to_owned());
  line.extend(["--no-remote", "--profile"].map(str::to_owned));
  line.push(profile.display().to_string());
  line.extend(["--remote-debugging-port", "0", "about:blank"].map(str::to_owned));
  line
}

/// A value as WebDriver BiDi writes one, as plain JSON: strings, numbers, booleans, arrays and objects as they
/// are, a number JSON cannot hold and anything that is no plain value — a node, a function, a window — as null.
pub fn plain(remote: &Value) -> Value {
  match remote["type"].as_str() {
    Some("string" | "boolean") => remote["value"].clone(),
    Some("number") if remote["value"].is_number() => remote["value"].clone(),
    Some("bigint") => remote["value"].clone(),
    Some("array") => Value::Array(remote["value"].as_array().into_iter().flatten().map(plain).collect()),
    Some("object") => Value::Object(
      remote["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), plain(&pair[1]))))
        .collect(),
    ),
    _ => Value::Null,
  }
}

/// The page's network as it stands: the requests under way and when one last began or ended. It has come to
/// rest once the page's document has loaded, no request is under way and none has begun or ended for half a
/// second.
#[derive(Default)]
pub struct Quiet {
  under_way: Mutex<HashSet<String>>,
  last_ms: AtomicU64,
}

impl Quiet {
  pub fn begun(&self, request: &str, now_ms: u64) {
    locked(&self.under_way).insert(request.to_owned());
    self.last_ms.store(now_ms, Ordering::SeqCst);
  }

  pub fn ended(&self, request: &str, now_ms: u64) {
    locked(&self.under_way).remove(request);
    self.last_ms.store(now_ms, Ordering::SeqCst);
  }

  pub fn rested(&self, loaded: bool, now_ms: u64) -> bool {
    loaded
      && locked(&self.under_way).is_empty()
      && now_ms.saturating_sub(self.last_ms.load(Ordering::SeqCst)) >= QUIET_MS
  }
}

/// Where the main tab's navigation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Load {
  Loading,
  Loaded,
  Failed(String),
}

// What the browser's events have shown of the main tab.
struct Traffic {
  page: PageWatch,
  context: Mutex<Option<String>>,
  // The navigation last begun in the main tab, and whether its document has arrived. A failure counts only
  // where it is this one's, and a navigation whose document arrived has not failed but been left: a site's own
  // script check moves on to the next address before the page has loaded, which ends the navigation it
  // replaces as failed.
  navigation: Mutex<Option<String>>,
  arrived: std::sync::atomic::AtomicBool,
  quiet: Quiet,
  load: tokio::sync::watch::Sender<Load>,
}

impl Traffic {
  fn new() -> Self {
    Self {
      page: PageWatch::default(),
      context: Mutex::default(),
      navigation: Mutex::default(),
      arrived: std::sync::atomic::AtomicBool::new(false),
      quiet: Quiet::default(),
      load: tokio::sync::watch::Sender::new(Load::Loaded),
    }
  }

  fn main(&self, params: &Value) -> bool {
    let main = locked(&self.context).clone();

    main.is_some() && params["context"].as_str() == main.as_deref()
  }

  // Whether an event is of the navigation standing in the main tab.
  fn current(&self, params: &Value) -> bool {
    let current = locked(&self.navigation).clone();

    self.main(params) && current.is_some() && params["navigation"].as_str() == current.as_deref()
  }

  // One event of the browser's, recorded; a held request handed to the guard.
  fn observe(self: &Arc<Self>, sender: &remote::Sender, guard: &UrlGuard, method: &str, params: &Value) {
    let request = params["request"]["request"].as_str().unwrap_or_default();
    let navigation = !params["navigation"].is_null();

    match method {
      "network.beforeRequestSent" => {
        self.quiet.begun(request, now_ms());

        if params["isBlocked"] == true {
          let (sender, traffic, guard, params) =
            (sender.clone(), self.clone(), guard.clone(), params.clone());

          tokio::spawn(async move { answer_held(&sender, &traffic, &guard, &params).await });
        }
      }
      "network.responseCompleted" => {
        self.quiet.ended(request, now_ms());

        let response = &params["response"];

        if navigation && self.current(params) {
          self.arrived.store(true, Ordering::SeqCst);
        }

        if navigation
          && self.main(params)
          && let Some(status) = response["status"].as_u64()
        {
          // Firefox names the type with its parameters, which the type the document is read as leaves out.
          let mime_type = response["mimeType"].as_str().unwrap_or_default();

          *locked(&self.page.response) = Some(DocumentResponse {
            status: status as u16,
            mime_type: mime_type.split(';').next().unwrap_or_default().trim().to_owned(),
          });
        }
      }
      "network.fetchError" => {
        self.quiet.ended(request, now_ms());

        let said = params["errorText"].as_str().unwrap_or("the request failed");

        if navigation && self.current(params) && said != SUPERSEDED {
          self.load.send_replace(Load::Failed(said.to_owned()));
        }
      }
      "browsingContext.navigationStarted" if self.main(params) => {
        *locked(&self.navigation) = params["navigation"].as_str().map(str::to_owned);
        self.arrived.store(false, Ordering::SeqCst);
        self.load.send_replace(Load::Loading);
      }
      // An error page loads after its navigation failed, and the failure stands.
      "browsingContext.load" if self.main(params) => {
        self.load.send_if_modified(|load| {
          let loading = *load == Load::Loading;

          if loading {
            *load = Load::Loaded;
          }

          loading
        });
      }
      "browsingContext.navigationFailed"
        if self.current(params) && !self.arrived.load(Ordering::SeqCst) =>
      {
        self.load.send_replace(Load::Failed("the navigation failed".to_owned()));
      }
      "browsingContext.downloadWillBegin" if self.main(params) => {
        self.load.send_replace(Load::Failed(
          "the address is a download, which a browser does not show".to_owned(),
        ));
      }
      _ => {}
    }
  }
}

// What a held request is answered with: let through where the guard passes it, failed where it does not — the
// whole read with it where the refusal fails the page.
async fn answer_held(sender: &remote::Sender, traffic: &Traffic, guard: &UrlGuard, params: &Value) {
  let (Some(request), Some(url)) =
    (params["request"]["request"].as_str(), params["request"]["url"].as_str())
  else {
    return;
  };
  let checked = if is_web(url) { guard(url.to_owned()).await.map(|_| ()) } else { Ok(()) };

  match checked {
    Ok(()) => {
      let _ = sender.command("network.continueRequest", json!({ "request": request })).await;
    }
    Err(refusal) => {
      let main = locked(&traffic.context).clone();
      let kind = if params["navigation"].is_null() { "Other" } else { "Document" };

      if fails_the_page(&refusal, Some(kind), params["context"].as_str(), main.as_deref()) {
        *locked(&traffic.page.blocked) = Some(refusal);
        let _ = sender.command("network.failRequest", json!({ "request": request })).await;
        traffic.page.blocked_now.notify_waiters();
      } else {
        let _ = sender.command("network.failRequest", json!({ "request": request })).await;
      }
    }
  }
}

/// Firefox on a copy of the owner's profile.
pub struct Firefox {
  firefox: String,
  profiles: PathBuf,
  owner: PathBuf,
  scope: String,
}

impl Firefox {
  /// The owner's Firefox where this machine has it: `BMO_FIREFOX_PATH`, else the snap's, and the profiles'
  /// folder `BMO_FIREFOX_PROFILES` names, else the snap's under the home folder, holding the profile Firefox
  /// opens by default. An empty `BMO_FIREFOX_PROFILES` names none.
  pub fn on_this_machine() -> Option<Self> {
    let firefox = std::env::var("BMO_FIREFOX_PATH").unwrap_or_else(|_| "/snap/bin/firefox".to_owned());
    let profiles = match std::env::var_os("BMO_FIREFOX_PROFILES") {
      Some(named) if named.is_empty() => return None,
      Some(named) => PathBuf::from(named),
      None => PathBuf::from(std::env::var_os("HOME")?).join("snap/firefox/common/.mozilla/firefox"),
    };

    Self::at(firefox, profiles)
  }

  /// Firefox at `firefox` on the default profile in `profiles`, where both stand.
  pub fn at(firefox: String, profiles: PathBuf) -> Option<Self> {
    if !Path::new(&firefox).exists() {
      return None;
    }

    let owner = copy::default_profile(&profiles).ok().filter(|owner| owner.is_dir())?;

    Some(Self {
      scope: snap_scope(&firefox),
      firefox,
      profiles,
      owner,
    })
  }

  async fn started(&self, guard: &UrlGuard) -> WebResult<Box<dyn Running>> {
    let unavailable = |why: String| WebError::new(why, Failure::BrowserUnavailable);
    let place = places::take_any(&places::place_paths())
      .await
      .map_err(|why| unavailable(format!("No browser place on this machine can be taken: {why}")))?;

    copy::reap_stale(&self.profiles, &self.owner, now_ms(), &profile::LiveProcesses).await;

    let folder = copy::make(&self.owner, &self.profiles, &profile::LiveProcesses)
      .await
      .map_err(|why| unavailable(format!("The owner's Firefox profile could not be copied: {why}")))?;
    let mut display = match display::PrivateDisplay::start(&folder).await {
      Ok(display) => display,
      Err(error) => {
        self.remove(&folder).await;
        return Err(error);
      }
    };
    let proxy = match proxy::start(proxy::resolver_of(guard)).await {
      Ok(proxy) => proxy,
      Err(why) => {
        display.stop().await;
        self.remove(&folder).await;
        return Err(unavailable(format!("Firefox's proxy could not start: {why}")));
      }
    };
    let preferences = std::fs::write(folder.join("user.js"), start_preferences(proxy.port, &folder))
      .map_err(|why| unavailable(format!("Firefox's preferences could not be written: {why}")));
    let launched = match preferences {
      Ok(()) => self.launch(guard, &folder, &display).await,
      Err(error) => Err(Box::new(Launch::default().failed(error))),
    };

    match launched {
      Ok(launch) => Ok(Box::new(FirefoxRunning {
        launch,
        display,
        proxy,
        folder,
        profiles: self.profiles.clone(),
        owner: self.owner.clone(),
        _place: place,
      })),
      Err(failed) => {
        let (launch, error) = *failed;

        launch.end().await;
        display.stop().await;
        proxy.stop();
        self.remove(&folder).await;
        Err(error)
      }
    }
  }

  async fn remove(&self, folder: &Path) {
    copy::remove(folder, &self.profiles, &self.owner, &profile::LiveProcesses).await;
  }

  // Firefox started on the copy in `folder` on its display, its endpoint reached and its main tab found.
  async fn launch(
    &self,
    guard: &UrlGuard,
    folder: &Path,
    display: &display::PrivateDisplay,
  ) -> Result<Launch, Box<(Launch, WebError)>> {
    let unavailable = |why: String| WebError::new(why, Failure::BrowserUnavailable);
    let mut launch = Launch::default();
    let environment = launch_environment(
      |name| std::env::var(name).ok(),
      &display.display,
      &display.xauthority,
      &display.runtime_dir,
      folder,
    );
    let line = launch_line(
      &self.firefox,
      &self.scope,
      &format!("{:016x}", rand::random::<u64>()),
      &environment,
      folder,
    );
    let spawned = Command::new(&line[0])
      .args(&line[1..])
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::piped())
      .process_group(0)
      .kill_on_drop(true)
      .spawn();
    let mut child = match spawned {
      Ok(child) => child,
      Err(why) => {
        return Err(Box::new(launch.failed(unavailable(format!("Firefox could not start: {why}")))));
      }
    };
    let group = child.id();
    let output = child.stderr.take();

    launch.child = Some(child);

    if let Some(group) = group {
      let watcher = Command::new("sh")
        .args(["-c", WATCH, "firefox-watch", &group.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();

      match watcher {
        Ok(mut watcher) => {
          launch.hold = watcher.stdin.take();
          launch.watcher = Some(watcher);
        }
        Err(why) => {
          return Err(Box::new(
            launch.failed(unavailable(format!("Firefox's watcher could not start: {why}"))),
          ));
        }
      }
    }

    // Firefox names its endpoint once it listens, after it has brought the copy up; its output is read until it
    // does, or until it ends, and the rest of it read away so it never stands on a full pipe.
    let Some(output) = output else {
      return Err(Box::new(launch.failed(unavailable("Firefox's output could not be read".to_owned()))));
    };
    let mut lines = tokio::io::BufReader::new(output).split(b'\n');
    let mut endpoint = None;

    while let Ok(Some(line)) = lines.next_segment().await {
      if let Some(announced) = String::from_utf8_lossy(&line).trim().strip_prefix(ANNOUNCED) {
        endpoint = Some(announced.to_owned());
        break;
      }
    }

    let Some(endpoint) = endpoint else {
      return Err(Box::new(launch.failed(unavailable("Firefox ended before it listened".to_owned()))));
    };

    launch.output =
      Some(tokio::spawn(async move { while let Ok(Some(_)) = lines.next_segment().await {} }));

    let traffic = Arc::new(Traffic::new());
    let handler_traffic = traffic.clone();
    let guard = guard.clone();
    let handle: remote::EventHandler =
      Arc::new(move |sender, method, params| handler_traffic.observe(sender, &guard, method, params));
    let connection = match remote::Connection::open(&format!("{endpoint}/session"), handle).await {
      Ok(connection) => connection,
      Err(error) => return Err(Box::new(launch.failed(error))),
    };
    let sender = connection.sender.clone();
    let opened = async {
      sender.command("session.new", json!({ "capabilities": {} })).await?;
      sender.command("session.subscribe", json!({ "events": EVENTS })).await?;
      sender
        .command("network.addIntercept", json!({ "phases": ["beforeRequestSent"] }))
        .await?;
      sender
        .command("script.addPreloadScript", json!({ "functionDeclaration": UNMARKED }))
        .await?;

      let tree = sender.command("browsingContext.getTree", json!({ "maxDepth": 0 })).await?;

      tree["contexts"][0]["context"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| unavailable("Firefox exposed no tab".to_owned()))
    };

    match opened.await {
      Ok(context) => {
        *locked(&traffic.context) = Some(context.clone());
        launch.connection = Some(connection);
        launch.traffic = Some(traffic);
        launch.context = context;
        Ok(launch)
      }
      Err(error) => Err(Box::new(launch.failed(error))),
    }
  }
}

impl Product for Firefox {
  fn start<'a>(&'a self, guard: &'a UrlGuard) -> Answer<'a, WebResult<Box<dyn Running>>> {
    Box::pin(self.started(guard))
  }
}

// What a launch has started so far, ended together however far it went.
#[derive(Default)]
struct Launch {
  child: Option<Child>,
  watcher: Option<Child>,
  // The watcher's standard input, held open for as long as the Firefox is to run.
  hold: Option<ChildStdin>,
  output: Option<JoinHandle<()>>,
  connection: Option<remote::Connection>,
  traffic: Option<Arc<Traffic>>,
  context: String,
}

impl Launch {
  fn failed(self, error: WebError) -> (Self, WebError) {
    (self, error)
  }

  // Firefox asked to close and sent `SIGTERM`, waited for until it has ended, and whatever is left of its process
  // group killed by the watcher as its input closes.
  async fn end(mut self) {
    if let Some(connection) = &self.connection {
      connection.sender.tell("browser.close", json!({}));
    }

    if let Some(child) = &mut self.child {
      if let Some(pid) = child.id() {
        // SAFETY: a signal to the Firefox this browser started and still holds.
        unsafe {
          libc::kill(pid as i32, libc::SIGTERM);
        }
      }

      let _ = child.wait().await;
    }

    drop(self.hold.take());

    if let Some(watcher) = &mut self.watcher {
      let _ = watcher.wait().await;
    }

    if let Some(output) = self.output.take() {
      output.abort();
    }
  }
}

// One Firefox as it runs, with everything started for it.
struct FirefoxRunning {
  launch: Launch,
  display: display::PrivateDisplay,
  proxy: proxy::PinnedProxy,
  folder: PathBuf,
  profiles: PathBuf,
  owner: PathBuf,
  _place: places::Place,
}

impl FirefoxRunning {
  fn connection(&self) -> &remote::Connection {
    self.launch.connection.as_ref().expect("a running Firefox holds its connection")
  }

  fn traffic(&self) -> &Traffic {
    self.launch.traffic.as_ref().expect("a running Firefox holds what it watches")
  }
}

impl Running for FirefoxRunning {
  fn is_open(&self) -> bool {
    self.connection().is_open()
  }

  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let sender = &self.connection().sender;
      let traffic = self.traffic();
      let page = &traffic.page;
      let mut events = self.connection().events.subscribe();

      traffic.load.send_replace(Load::Loading);

      let mut load = traffic.load.subscribe();
      let navigated = tokio::select! {
        answer = sender.command(
          "browsingContext.navigate",
          json!({ "context": self.launch.context, "url": url, "wait": "none" }),
        ) => answer,
        () = page.blocked_now.notified() => return Err(page.refusal(url)),
      };

      // A navigation the site replaced at once by another, its own redirect, is waited on through the one that
      // took its place.
      if let Err(error) = navigated
        && !error.message.contains(SUPERSEDED)
      {
        return Err(navigation_failure(url, &error.message, page.blocked()));
      }

      tokio::select! {
        settled = load.wait_for(|load| *load != Load::Loading) => match settled.map(|load| load.clone()) {
          Ok(Load::Failed(said)) => Err(navigation_failure(url, &said, page.blocked())),
          _ => Ok(()),
        },
        ended = remote::event_where(&mut events, |_, _| false) => ended.map(|_| ()),
        () = page.blocked_now.notified() => Ok(()),
      }
    })
  }

  fn evaluate<'a>(&'a self, expression: &'a str) -> Answer<'a, WebResult<Value>> {
    Box::pin(async move {
      let raw = self
        .connection()
        .sender
        .command(
          "script.evaluate",
          json!({
            "expression": expression,
            "target": { "context": self.launch.context },
            "awaitPromise": true,
            "resultOwnership": "none",
            "serializationOptions": { "maxDomDepth": 0, "maxObjectDepth": 8 },
          }),
        )
        .await?;

      if raw["type"] == "exception" {
        return Err(WebError::new("The page's script failed in Firefox", Failure::Transport));
      }

      Ok(plain(&raw["result"]))
    })
  }

  // One pointer event per call through the BiDi input domain; the walk between the events is the caller's,
  // and a widget takes the press and release as a person's because they come from here.
  fn gesture<'a>(&'a self, at: &'a Gesture) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let event = match at {
        Gesture::Move { x, y } => {
          let (x, y) = super::whole(*x, *y);

          json!({ "type": "pointerMove", "x": x, "y": y, "duration": 60 })
        }
        Gesture::Down { .. } => json!({ "type": "pointerDown", "button": 0, "duration": 10 }),
        Gesture::Up { .. } => json!({ "type": "pointerUp", "button": 0, "duration": 10 }),
      };

      self
        .connection()
        .sender
        .command(
          "input.performActions",
          json!({
            "context": self.launch.context,
            "actions": [{
              "type": "pointer",
              "id": "web-solver",
              "parameters": { "pointerType": "mouse" },
              "actions": [event],
            }],
          }),
        )
        .await
        .map(|_| ())
    })
  }

  // Every word and, when asked, Enter — the BiDi keyboard source, one key up behind every key down.
  fn keys<'a>(&'a self, text: &'a str, enter: bool) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let mut events: Vec<Value> = text
        .chars()
        .flat_map(|one| {
          [
            json!({ "type": "keyDown", "value": one.to_string() }),
            json!({ "type": "keyUp", "value": one.to_string() }),
          ]
        })
        .collect();

      if enter {
        // Enter as the WebDriver key of its own.
        events.push(json!({ "type": "keyDown", "value": "" }));
        events.push(json!({ "type": "keyUp", "value": "" }));
      }

      self
        .connection()
        .sender
        .command(
          "input.performActions",
          json!({
            "context": self.launch.context,
            "actions": [{ "type": "key", "id": "web-voice", "actions": events }],
          }),
        )
        .await
        .map(|_| ())
    })
  }

  // The view as a PNG, through the BiDi picture command the same browser answers its driver with.
  fn screenshot<'a>(&'a self) -> Answer<'a, WebResult<Vec<u8>>> {
    Box::pin(async move {
      let shot = self
        .connection()
        .sender
        .command(
          "browsingContext.captureScreenshot",
          json!({ "context": self.launch.context, "format": { "type": "image/png" } }),
        )
        .await?;
      let data = shot["data"]
        .as_str()
        .ok_or_else(|| WebError::new("Firefox answered no picture for the screen", Failure::Transport))?;
      let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| WebError::new("Firefox's picture did not read as base64", Failure::Transport))?;

      Ok(png)
    })
  }

  fn watch(&self) -> &PageWatch {
    &self.traffic().page
  }

  fn rested(&self) -> bool {
    let traffic = self.traffic();
    let loaded = *traffic.load.borrow() != Load::Loading;

    traffic.quiet.rested(loaded, now_ms()) || !self.connection().sender.is_reading()
  }

  // Ends the Firefox and everything started for it, removes its copy of the owner's profile, and gives its place
  // back.
  fn shut_down(self: Box<Self>) -> Answer<'static, ()> {
    Box::pin(async move {
      let mut running = *self;

      running.launch.end().await;
      running.display.stop().await;
      running.proxy.stop();
      copy::remove(&running.folder, &running.profiles, &running.owner, &profile::LiveProcesses).await;
    })
  }
}
