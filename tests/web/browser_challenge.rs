//! The one attempt a fetch road browser read makes (docs/web-access.md, 「When the browser meets a
//! challenge」), driven against a stand-in browser product: what it looks at, when it clicks, and how often
//! it opens the page.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use bmo_explorer::web::browser::challenge::{BOX, STILL};
use bmo_explorer::web::browser::*;
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::error::{Failure, WebError, WebResult};

// The page one opening stands the browser at, answered by every snapshot of it.
#[derive(Clone)]
struct Phase {
  status: u16,
  title: &'static str,
  text: &'static str,
  html: &'static str,
}

fn gate() -> Phase {
  Phase {
    status: 403,
    title: "Just a moment...",
    text: "Verify you are human",
    html: "<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div></body></html>",
  }
}

fn page() -> Phase {
  Phase {
    status: 200,
    title: "The page",
    text: "what the site really says",
    html: "<html><body><p>what the site really says</p></body></html>",
  }
}

// What the stand-in browser saw.
#[derive(Default)]
struct Seen {
  opens: Vec<String>,
  clicks: Vec<(f64, f64)>,
  markers: usize,
  boxes: usize,
}

// The stand-in: one opening per phase, marker and box answers from its queues — the last of them kept — and
// every snapshot of an opening the same page, so the settle comes to rest at once.
struct Scripted {
  phases: Vec<Phase>,
  opening: AtomicUsize,
  stills: Mutex<Answers>,
  points: Mutex<Answers>,
  seen: Mutex<Seen>,
  watch: PageWatch,
}

// One scripted answer queue: the last answer it holds stands for every ask after it runs out.
struct Answers(Vec<Value>);

impl Answers {
  fn next(&mut self) -> Value {
    if self.0.len() > 1 {
      self.0.remove(0)
    } else {
      self.0.first().cloned().unwrap_or(Value::Null)
    }
  }
}

impl Scripted {
  fn of(pages: Vec<Phase>, stills: Vec<Value>, points: Vec<Value>) -> Arc<Self> {
    Arc::new(Self {
      phases: pages,
      opening: AtomicUsize::new(0),
      stills: Mutex::new(Answers(stills)),
      points: Mutex::new(Answers(points)),
      seen: Mutex::new(Seen::default()),
      watch: PageWatch::default(),
    })
  }

  fn phase(&self) -> Phase {
    let at = self.opening.load(Ordering::SeqCst).saturating_sub(1);

    self.phases.get(at).or(self.phases.last()).expect("the script names a page").clone()
  }
}

impl Running for Scripted {
  fn is_open(&self) -> bool {
    true
  }

  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      self.opening.fetch_add(1, Ordering::SeqCst);
      self.seen.lock().unwrap().opens.push(url.to_owned());
      let phase = self.phase();
      *self.watch.response.lock().unwrap() = Some(DocumentResponse {
        status: phase.status,
        mime_type: "text/html".into(),
      });

      Ok(())
    })
  }

  fn evaluate<'a>(&'a self, expression: &'a str) -> Answer<'a, WebResult<Value>> {
    Box::pin(async move {
      if expression == STILL {
        self.seen.lock().unwrap().markers += 1;

        return Ok(self.stills.lock().unwrap().next());
      }

      if expression == BOX {
        self.seen.lock().unwrap().boxes += 1;

        return Ok(self.points.lock().unwrap().next());
      }

      if expression.contains("document.links") {
        let phase = self.phase();

        return Ok(json!({
          "url": "https://gate.example/asked",
          "title": phase.title,
          "contentType": "text/html",
          "text": phase.text,
          "html": phase.html,
          "links": [],
          "userAgent": "Firefox",
          "webdriver": false,
          "truncated": false,
        }));
      }

      Err(WebError::new(
        "the stand-in browser was asked to read something it does not hold",
        Failure::Transport,
      ))
    })
  }

  fn click<'a>(&'a self, x: f64, y: f64) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      self.seen.lock().unwrap().clicks.push((x, y));

      Ok(())
    })
  }

  fn watch(&self) -> &PageWatch {
    &self.watch
  }

  fn rested(&self) -> bool {
    true
  }

  fn shut_down(self: Box<Self>) -> Answer<'static, ()> {
    Box::pin(async {})
  }
}

// A started browser is the script itself, boxed; the script outlives any one start.
struct ScriptedStart(Arc<Scripted>);

impl Product for ScriptedStart {
  fn start<'a>(&'a self, _: &'a UrlGuard) -> Answer<'a, WebResult<Box<dyn Running>>> {
    let script = self.0.clone();

    Box::pin(async move { Ok(Box::new(Started(script)) as Box<dyn Running>) })
  }
}

struct Started(Arc<Scripted>);

impl std::ops::Deref for Started {
  type Target = Scripted;

  fn deref(&self) -> &Scripted {
    &self.0
  }
}

impl Running for Started {
  fn is_open(&self) -> bool {
    Scripted::is_open(self)
  }

  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>> {
    Scripted::open(self, url)
  }

  fn evaluate<'a>(&'a self, expression: &'a str) -> Answer<'a, WebResult<Value>> {
    Scripted::evaluate(self, expression)
  }

  fn click<'a>(&'a self, x: f64, y: f64) -> Answer<'a, WebResult<()>> {
    Scripted::click(self, x, y)
  }

  fn watch(&self) -> &PageWatch {
    Scripted::watch(self)
  }

  fn rested(&self) -> bool {
    Scripted::rested(self)
  }

  fn shut_down(self: Box<Self>) -> Answer<'static, ()> {
    Box::pin(async {})
  }
}

fn browser(script: &Arc<Scripted>) -> Browser {
  let guard: UrlGuard = Arc::new(|_| Box::pin(async { Ok(None) }));

  Browser::new(guard, Box::new(ScriptedStart(script.clone())))
}

async fn read(browser: &Browser, solve: bool) -> WebResult<BrowserDocument> {
  browser
    .read(
      "https://gate.example/asked",
      FetchOptions {
        solve_challenge: solve,
        clock: StandingClock::at(1_000_000),
        ..FetchOptions::default()
      },
    )
    .await
}

fn seen(script: &Arc<Scripted>) -> Seen {
  std::mem::take(&mut *script.seen.lock().unwrap())
}

// The read asks, the site's own check passes before anything is clicked, and the page is opened exactly once
// more.
#[tokio::test(start_paused = true)]
async fn a_challenge_that_passes_its_own_check_is_answered_with_the_page_behind_it() {
  let script = Scripted::of(vec![gate(), page()], vec![true.into(), false.into()], vec![]);
  let document = read(&browser(&script), true).await.unwrap();

  assert_eq!(document.text, "what the site really says");
  assert_eq!(document.challenge, ChallengeMark::Cleared);
  let saw = seen(&script);
  assert_eq!(saw.opens, ["https://gate.example/asked", "https://gate.example/asked"]);
  assert!(saw.clicks.is_empty(), "a check that passes on its own is never clicked");
  assert_eq!(saw.boxes, 0);
}

// The check does not pass alone: the box is clicked once, at the point the page itself named, and the page is
// then opened once more.
#[tokio::test(start_paused = true)]
async fn a_challenge_that_stands_until_clicked_is_clicked_once_at_its_own_box() {
  // The first wait is spent at its bound — twenty-half-second looks and the last one that finds the bound
  // spent, all standing — so the click, not the wait, is what clears it.
  let mut stills = vec![Value::from(true); 21];
  stills.push(Value::from(false));
  let script = Scripted::of(vec![gate(), page()], stills, vec![json!({ "x": 131, "y": 320 })]);
  let document = read(&browser(&script), true).await.unwrap();

  assert_eq!(document.text, "what the site really says");
  assert_eq!(document.challenge, ChallengeMark::Cleared);
  let saw = seen(&script);
  assert_eq!(saw.opens.len(), 2);
  assert_eq!(saw.clicks, [(131.0, 320.0)]);
}

// Nothing moves the page: the waits are spent at their bound — twenty looks of half a second each — the
// retry is opened once, and the gate answers again.
#[tokio::test(start_paused = true)]
async fn a_challenge_that_stands_is_attempted_once_and_marked_as_it_stands() {
  let script = Scripted::of(vec![gate()], vec![true.into()], vec![Value::Null]);
  let document = read(&browser(&script), true).await.unwrap();

  assert_eq!(document.title, "Just a moment...");
  assert_eq!(document.challenge, ChallengeMark::Remains);
  let saw = seen(&script);
  assert_eq!(saw.opens.len(), 2, "the read retried once and no more");
  assert!(saw.clicks.is_empty(), "the page named no box to click");
  assert_eq!(
    saw.markers, 21,
    "the bound of ten seconds was looked at once per half second, and once more when it was spent"
  );
}

// A read that does not ask: the gate is read as it stands, with no marker asked and no second opening.
#[tokio::test(start_paused = true)]
async fn a_read_that_does_not_ask_never_probes_and_never_retries() {
  let script = Scripted::of(vec![gate()], vec![false.into()], vec![]);
  let document = read(&browser(&script), false).await.unwrap();

  assert_eq!(document.title, "Just a moment...");
  assert_eq!(document.challenge, ChallengeMark::NotAttempted);
  let saw = seen(&script);
  assert_eq!(saw.opens.len(), 1);
  assert_eq!(saw.markers, 0);
  assert_eq!(saw.boxes, 0);
}
