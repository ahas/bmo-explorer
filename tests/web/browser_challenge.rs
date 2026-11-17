//! The reasoning loop a fetch road browser read makes at a challenge (docs/web-access.md, 「When the browser
//! meets a challenge」), driven against a stand-in product that records every trusted gesture and key the loop
//! sends: what it waits through, what it clicks and pulls and types, the shape of the walk it walks, where it
//! stops, and whether a widget was let to move its piece at all. The clock is a standing one throughout, so a
//! round's waits cost the test nothing.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};

use bmo_explorer::web::browser::challenge::{ASK, Bounds, Outcome, STAGE, STILL};
use bmo_explorer::web::browser::*;
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::error::{Failure, WebError, WebResult};
use bmo_explorer::web::vision::Vision;

// The page one opening stands the browser at.
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

// The gate as the site itself closes it, in its own words.
fn refusing() -> Phase {
  Phase {
    status: 403,
    title: "Just a moment...",
    text: "Too many failed attempts, try again later",
    html: "<html><head><title>Just a moment...</title></head><body><p>Too many failed attempts</p></body></html>",
  }
}

// What the stand-in browser saw of one read.
#[derive(Default)]
struct Seen {
  opens: usize,
  markers: usize,
  stages: usize,
  shots: usize,
  moves: usize,
  downs: usize,
  ups: usize,
  down_at: Option<(f64, f64)>,
  up_at: Option<(f64, f64)>,
  farthest_x: f64,
  /// Where the scripted slider's piece stands, as far as the widget let it move.
  piece_at: Option<f64>,
  /// How often a stage probe was handed to this widget asking the handle's native drag off.
  disarms: usize,
  keys: Vec<(String, bool)>,
}

// How the scripted gate comes off: under its own check, under a release, under a release whose handle had its
// native drag taken off, under an answer, or by the site's own refusal — or never.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clear {
  OnWatch,
  OnRelease,
  OnDisarmedRelease,
  OnKeys,
  EndsAfterAct,
  Never,
}

// A page feels what its own script asks of it, so the stand-in for a slider widget built from a plain image
// handle judges the probe it is handed by the two requests made of that handle: its drag ability written off,
// and its drag start cancelled. Where they are not made of the handle, the widget's picture is being moved and
// its piece never follows the pointer.
fn asks_the_drag_off(probe: &str) -> bool {
  probe.contains(r#"handle.setAttribute("draggable", "false")"#)
    && probe.contains(r#"handle.addEventListener("dragstart""#)
    && probe.contains("preventDefault")
}

// The stand-in: a gate that comes off as the test scripts it, answering the loop's marker probe and its stage
// probe and recording every gesture the loop sends through the browser's own input.
struct Scripted {
  phases: Vec<Phase>,
  opening: AtomicUsize,
  stage: Value,
  clear: Clear,
  downs: AtomicUsize,
  ups: AtomicUsize,
  watches: AtomicUsize,
  cleared: AtomicBool,
  /// Whether a stage probe this widget was handed asked for the handle's native drag off.
  disarmed: AtomicBool,
  seen: Mutex<Seen>,
  watch: PageWatch,
}

impl Scripted {
  fn phase(&self) -> Phase {
    let at = self.opening.load(Ordering::SeqCst).saturating_sub(1);

    self.phases.get(at).or(self.phases.last()).expect("the script names a page").clone()
  }

  // What the page now says of its challenge, as the loop's own marker probe reads it.
  fn still(&self) -> &'static str {
    match self.clear {
      // The site's check passes on its own, part way through the round's wait.
      Clear::OnWatch => {
        if self.watches.fetch_add(1, Ordering::SeqCst) == 0 {
          "standing"
        } else {
          self.cleared.store(true, Ordering::SeqCst);

          "gone"
        }
      }
      Clear::OnRelease | Clear::OnDisarmedRelease | Clear::OnKeys => {
        if self.cleared.load(Ordering::SeqCst) { "gone" } else { "standing" }
      }
      Clear::EndsAfterAct => {
        if self.downs.load(Ordering::SeqCst) == 0 {
          "standing"
        } else {
          "ended"
        }
      }
      Clear::Never => "standing",
    }
  }
}

impl Running for Scripted {
  fn is_open(&self) -> bool {
    true
  }

  fn open<'a>(&'a self, url: &'a str) -> Answer<'a, WebResult<()>> {
    let _ = url;
    Box::pin(async move {
      let at = if self.cleared.load(Ordering::SeqCst) {
        2
      } else {
        self.opening.fetch_add(1, Ordering::SeqCst) + 1
      };

      self.opening.store(at, Ordering::SeqCst);
      self.seen.lock().unwrap().opens += 1;
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

        return Ok(Value::from(self.still()));
      }

      if expression == STAGE {
        let mut seen = self.seen.lock().unwrap();

        seen.stages += 1;

        if asks_the_drag_off(expression) {
          seen.disarms += 1;
          self.disarmed.store(true, Ordering::SeqCst);
        }

        return Ok(self.stage.clone());
      }

      // The page's own words, as the loop reads them when the site ends a challenge: asked of the snapshot
      // expression too, so the whole-page read is answered by name before the plain text of the page.
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

      if expression.contains("innerText") {
        return Ok(Value::from(self.phase().text));
      }

      Err(WebError::new(
        "the stand-in browser was asked to read something it does not hold",
        Failure::Transport,
      ))
    })
  }

  fn gesture<'a>(&'a self, at: &'a Gesture) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let mut seen = self.seen.lock().unwrap();
      let mut piece_followed = false;

      match at {
        Gesture::Move { x, y: _ } => {
          seen.moves += 1;
          seen.farthest_x = seen.farthest_x.max(*x);
        }
        Gesture::Down { x, y } => {
          self.downs.fetch_add(1, Ordering::SeqCst);
          seen.downs += 1;
          seen.down_at = Some((*x, *y));
        }
        Gesture::Up { x, y } => {
          self.ups.fetch_add(1, Ordering::SeqCst);
          seen.ups += 1;
          seen.up_at = Some((*x, *y));

          // A plain-image handle lets its piece follow only where the probe took its native drag off.
          if self.clear == Clear::OnDisarmedRelease && self.disarmed.load(Ordering::SeqCst) {
            seen.piece_at = Some(*x);
            piece_followed = true;
          }
        }
      }

      drop(seen);

      let comes_off = match self.clear {
        Clear::OnRelease => self.ups.load(Ordering::SeqCst) > 0,
        Clear::OnDisarmedRelease => piece_followed,
        _ => false,
      };

      if comes_off {
        self.cleared.store(true, Ordering::SeqCst);
      }

      Ok(())
    })
  }

  fn keys<'a>(&'a self, text: &'a str, enter: bool) -> Answer<'a, WebResult<()>> {
    Box::pin(async move {
      let mut seen = self.seen.lock().unwrap();

      seen.keys.push((text.to_owned(), enter));

      if self.clear == Clear::OnKeys {
        self.cleared.store(true, Ordering::SeqCst);
      }

      Ok(())
    })
  }

  fn screenshot<'a>(&'a self) -> Answer<'a, WebResult<Vec<u8>>> {
    Box::pin(async move {
      self.seen.lock().unwrap().shots += 1;

      Ok(b"fake-png".to_vec())
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

fn script(phases: Vec<Phase>, stage: Value, clear: Clear) -> Arc<Scripted> {
  Arc::new(Scripted {
    phases,
    opening: AtomicUsize::new(0),
    stage,
    clear,
    downs: AtomicUsize::new(0),
    ups: AtomicUsize::new(0),
    watches: AtomicUsize::new(0),
    cleared: AtomicBool::new(false),
    disarmed: AtomicBool::new(false),
    seen: Mutex::new(Seen::default()),
    watch: PageWatch::default(),
  })
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

  fn gesture<'a>(&'a self, at: &'a Gesture) -> Answer<'a, WebResult<()>> {
    Scripted::gesture(self, at)
  }

  fn keys<'a>(&'a self, text: &'a str, enter: bool) -> Answer<'a, WebResult<()>> {
    Scripted::keys(self, text, enter)
  }

  fn screenshot<'a>(&'a self) -> Answer<'a, WebResult<Vec<u8>>> {
    Scripted::screenshot(self)
  }

  fn watch(&self) -> &PageWatch {
    Scripted::watch(self)
  }

  fn rested(&self) -> bool {
    true
  }

  fn shut_down(self: Box<Self>) -> Answer<'static, ()> {
    Box::pin(async {})
  }
}

// One read of the gate, asked of the stand-in with the loop's bounds, the test's own clock and, where the round
// needs a picture read, a stand-in vision road.
async fn read(
  script: &Arc<Scripted>,
  solve: bool,
  bounds: Bounds,
  vision: Option<Arc<dyn Vision>>,
) -> WebResult<BrowserDocument> {
  let guard: UrlGuard = Arc::new(|_| Box::pin(async { Ok(None) }));
  let browser = Browser::new(guard, Box::new(ScriptedStart(script.clone())));

  browser
    .read(
      "https://gate.example/asked",
      FetchOptions {
        solve_challenge: solve,
        bounds,
        vision,
        clock: StandingClock::at(1_000_000),
        ..FetchOptions::default()
      },
    )
    .await
}

fn seen(script: &Arc<Scripted>) -> Seen {
  std::mem::take(&mut *script.seen.lock().unwrap())
}

fn checkbox(x: f64, y: f64) -> Value {
  json!({ "dpr": 1, "boxPoint": { "x": x, "y": y }, "slider": null, "input": null })
}

fn nothing() -> Value {
  json!({ "dpr": 1, "boxPoint": null, "slider": null, "input": null })
}

// The vision road as a test stands in for it: it answers the loop's one question with what it was handed.
struct Reader(Value);

impl Vision for Reader {
  fn read<'a>(&'a self, png: &'a [u8], ask: &'a str) -> Answer<'a, WebResult<Value>> {
    let said = self.0.clone();
    assert_eq!(ask, ASK, "the loop asks its one question of every picture");
    assert_eq!(png, b"fake-png", "what it is shown is the screen itself");

    Box::pin(async move { Ok(said) })
  }

  fn road(&self) -> &'static str {
    "the test road"
  }
}

fn eye(answer: Value) -> Option<Arc<dyn Vision>> {
  Some(Arc::new(Reader(answer)))
}

// The site's own check ends the stage before anything is moved: the loop waits it out, the page is opened
// exactly once more, and the account names no act of its own.
#[tokio::test]
async fn a_challenge_that_passes_its_own_check_is_answered_with_the_page_behind_it() {
  let script = script(vec![gate(), page()], nothing(), Clear::OnWatch);
  let document = read(&script, true, Bounds::default(), None).await.unwrap();

  assert_eq!(document.text, "what the site really says");
  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!("the read was cleared")
  };
  assert_eq!(tried.outcome, Outcome::Cleared);
  assert_eq!(tried.rounds, 1, "the first round's wait, with nothing acted on");
  assert!(tried.log.is_empty(), "the loop did nothing: {:?}", tried.log);
  let saw = seen(&script);
  assert_eq!(
    (saw.markers, saw.stages),
    (2, 0),
    "the round ended on its watch, before any stage probe"
  );
  assert_eq!((saw.downs, saw.ups), (0, 0), "a check that passes on its own is never clicked");
  assert_eq!(saw.opens, 2, "the first read and the one retry");
  assert!(tried.said().starts_with("cleared in 1 round(s): nothing to act on"), "{}", tried.said());
}

// The check does not pass alone: the box is clicked once, at the point the page itself named, on a path the
// hand walked rather than jumped to.
#[tokio::test]
async fn a_checkbox_is_clicked_once_on_a_walked_path() {
  let script = script(vec![gate(), page()], checkbox(131.0, 320.0), Clear::OnRelease);
  let document = read(&script, true, Bounds::default(), None).await.unwrap();

  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!("the loop was to clear it")
  };
  assert_eq!(tried.log, ["clicked its box".to_owned()]);
  assert_eq!(tried.rounds, 1);
  let saw = seen(&script);
  assert_eq!((saw.downs, saw.ups), (1, 1), "one press, one release");
  assert_eq!(saw.down_at, Some((131.0, 320.0)), "pressed at the point the page named");
  assert_eq!(saw.up_at, Some((131.0, 320.0)), "released over it");
  assert!(saw.moves >= 8, "the hand walked a path of several moves, not a jump: {}", saw.moves);
  assert!(
    saw.markers >= 22,
    "the round's wait was spent before the act, and the act was watched after"
  );
}

// A slider is pulled to the gap the ported ddddocr matcher read out of the pictures the page itself carried,
// and the pull walks past its gap and settles back on it.
#[tokio::test]
async fn a_slider_is_pulled_to_the_gap_the_ported_matcher_reads() {
  let (board, piece) = slider_pictures(300);
  let stage = json!({
    "dpr": 1,
    "boxPoint": null,
    "slider": {
      "x": 40.0, "y": 200.0, "boardX": 0.0, "boardScale": 1.0,
      "piece": base64_of(&piece), "background": base64_of(&board),
    },
    "input": null,
  });
  let script = script(vec![gate(), page()], stage, Clear::OnRelease);
  let document = read(&script, true, Bounds::default(), None).await.unwrap();

  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!("the pull was to clear it: {:?}", document.challenge)
  };
  assert!(
    tried.log[0].contains("ddddocr"),
    "the matcher is named as what read the gap: {:?}",
    tried.log
  );
  let saw = seen(&script);
  assert_eq!(saw.downs, 1, "one press, on the handle");
  assert_eq!(saw.down_at.map(|(x, _)| x), Some(40.0), "pressed where the handle stands");
  let up_x = saw.up_at.expect("the pull ended in a release").0;
  assert!((296.0..=306.0).contains(&up_x), "released over the gap the matcher read: {up_x}");
  assert!(
    saw.farthest_x > up_x + 4.0,
    "the pull overshot its gap, as a hand's does: {}",
    saw.farthest_x
  );
  assert!(saw.moves >= 14, "the pull is walked: {} moves", saw.moves);
}

// Where the page carries no pictures that could be copied, the round is read off the screen by the vision
// road, and the gap it names is set onto the board the page showed.
#[tokio::test]
async fn a_slider_without_pictures_is_read_off_the_screen() {
  let stage = json!({
    "dpr": 1, "boxPoint": null,
    "slider": { "x": 40.0, "y": 200.0, "boardX": 10.0, "boardScale": 1.0, "piece": null, "background": null },
    "input": null,
  });
  let script = script(vec![gate(), page()], stage, Clear::OnRelease);
  let document = read(
    &script,
    true,
    Bounds::default(),
    eye(json!({
      "kind": "slider", "gap_x": 300, "text": null, "click": null,
    })),
  )
  .await
  .unwrap();

  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!("the vision pull was to clear it")
  };
  assert!(tried.log[0].contains("the vision road read"), "{:?}", tried.log);
  let saw = seen(&script);
  assert_eq!(saw.shots, 1, "one look at the screen for the one round that needed it read");
  assert_eq!(saw.down_at.map(|(x, _)| x), Some(40.0));
  let up_x = saw.up_at.expect("released").0;
  assert!(
    (306.0..=316.0).contains(&up_x),
    "the board's own x 10 plus the 300 the road read: {up_x}"
  );
}

// A slider widget built from a plain image handle swallows a walked press as its picture being moved, so the
// probe takes that drag off the handle before naming it: only then does the piece follow the pull at all.
#[tokio::test]
async fn a_slider_handle_stops_dragging_before_the_round_pulls_it() {
  assert!(
    !asks_the_drag_off(STILL),
    "the scripted widget judges the probe it is handed, not any script it is given"
  );
  let (board, piece) = slider_pictures(300);
  let stage = json!({
    "dpr": 1,
    "boxPoint": null,
    "slider": {
      "x": 40.0, "y": 200.0, "boardX": 0.0, "boardScale": 1.0,
      "piece": base64_of(&piece), "background": base64_of(&board),
    },
    "input": null,
  });
  let script = script(vec![gate(), page()], stage, Clear::OnDisarmedRelease);
  let document = read(&script, true, Bounds::default(), None).await.unwrap();

  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!(
      "the pull was to move the piece once the handle stopped dragging: {:?}",
      document.challenge
    )
  };
  assert_eq!(
    tried.outcome,
    Outcome::Cleared,
    "the loop's own account, not the page it happened to answer"
  );
  assert_eq!(tried.rounds, 1, "the first pull moved the piece, once its drag was off");
  let saw = seen(&script);
  assert_eq!(saw.disarms, 1, "the probe that named the handle also took its native drag off");
  assert!(saw.piece_at.is_some(), "a handle that was left draggable would have swallowed the pull");
  assert_eq!(saw.piece_at, saw.up_at.map(|(x, _)| x), "the piece came to rest where the pull let go");
}

// Words a text prompt asks for are typed through the browser's key path and answered with Enter.
#[tokio::test]
async fn a_text_prompt_is_answered_through_the_key_path() {
  let stage = json!({ "dpr": 1, "boxPoint": null, "slider": null, "input": { "x": 300.0, "y": 140.0 } });
  let script = script(vec![gate(), page()], stage, Clear::OnKeys);
  let document = read(
    &script,
    true,
    Bounds::default(),
    eye(json!({
      "kind": "text", "text": "7349", "gap_x": null, "click": null,
    })),
  )
  .await
  .unwrap();

  let ChallengeMark::Cleared(tried) = document.challenge else {
    panic!("the answer was to clear it")
  };
  assert!(tried.log[0].starts_with("answered its words"), "{:?}", tried.log);
  assert!(tried.log[0].contains("7349"));
  let saw = seen(&script);
  assert_eq!(saw.keys, [("7349".to_owned(), true)], "the words, with one Enter");
  assert_eq!(saw.down_at, Some((300.0, 140.0)), "the field is clicked before it is typed into");
}

// Nothing moves a hard gate: the loop spends its rounds and stops at its bound, and the account says both the
// rounds and the bound, and never that it solved anything.
#[tokio::test]
async fn a_hard_gate_stops_at_the_round_bound_saying_so() {
  let script = script(vec![gate()], nothing(), Clear::Never);
  let document = read(
    &script,
    true,
    Bounds {
      rounds: 3,
      ms: 90_000,
    },
    None,
  )
  .await
  .unwrap();

  assert_eq!(document.title, "Just a moment...", "the gate answers again after the retry");
  let ChallengeMark::Remains(tried) = document.challenge else {
    panic!("the gate was to stand")
  };
  assert_eq!(
    tried.outcome,
    Outcome::Stopped {
      at: "the round bound"
    }
  );
  assert_eq!(tried.rounds, 3);
  assert_eq!(tried.log.len(), 6, "every round names its act and what the page said after it");
  assert!(
    tried
      .log
      .iter()
      .all(|one| *one == "watched its check run" || *one == "the challenge still stood")
  );
  let said = tried.said();
  assert!(said.contains("attempted over 3 round(s)"), "{said}");
  assert!(said.contains("stopped at the round bound"), "{said}");
  assert!(said.contains("the challenge was not solved"), "{said}");
  let saw = seen(&script);
  assert_eq!(saw.opens, 2, "the read retried once and no more");
  assert!(
    saw.markers > 3 * 40,
    "every round's wait was looked at once per half second: {}",
    saw.markers
  );
}

// The time bound stops the loop the same honest way, naming itself.
#[tokio::test]
async fn a_hard_gate_stops_at_the_time_bound_naming_it() {
  let script = script(vec![gate()], nothing(), Clear::Never);
  let document = read(
    &script,
    true,
    Bounds {
      rounds: 50,
      ms: 21_000,
    },
    None,
  )
  .await
  .unwrap();

  let ChallengeMark::Remains(tried) = document.challenge else {
    panic!("the gate was to stand")
  };
  assert_eq!(
    tried.outcome,
    Outcome::Stopped {
      at: "the time bound"
    }
  );
  assert!(tried.said().contains("stopped at the time bound"), "{}", tried.said());
}

// The site's own wrong-answer wording closes the loop inside its bounds, and the account says who closed it.
#[tokio::test]
async fn a_challenge_the_site_ends_is_told_as_the_sites_own_end() {
  let script = script(vec![refusing()], checkbox(131.0, 320.0), Clear::EndsAfterAct);
  let document = read(&script, true, Bounds::default(), None).await.unwrap();

  let ChallengeMark::Remains(tried) = document.challenge else {
    panic!("the site was to end it")
  };
  let Outcome::Ended { why } = &tried.outcome else {
    panic!("the account names the site's own end: {:?}", tried.outcome)
  };
  assert!(why.contains("Too many failed attempts"), "said with the page's own words: {why}");
  assert_eq!(tried.rounds, 1, "the loop spent no further round on a gate the site had closed");
  assert_eq!(tried.log, ["clicked its box".to_owned()]);
  let said = tried.said();
  assert!(said.contains("ended by the site itself after 1 round(s)"), "{said}");
  assert!(!said.contains("not solved"), "what ended it is said, no solve claimed: {said}");
}

// A read that does not ask for the loop never probes it: no marker, no stage, no second opening.
#[tokio::test]
async fn a_read_that_does_not_ask_never_probes_and_never_retries() {
  let script = script(vec![gate()], checkbox(1.0, 1.0), Clear::OnRelease);
  let document = read(&script, false, Bounds::default(), None).await.unwrap();

  assert_eq!(document.challenge, ChallengeMark::NotAttempted);
  let saw = seen(&script);
  assert_eq!((saw.opens, saw.markers, saw.stages), (1, 0, 0));
}

// A board with its hole at `gap` and the piece that fits it, as the two PNG byte strings the page's probe
// would have carried.
fn slider_pictures(gap: u32) -> (Vec<u8>, Vec<u8>) {
  let board: RgbaImage = ImageBuffer::from_fn(360, 120, |x, y| {
    if (gap..gap + 44).contains(&x) && (38..82).contains(&y) {
      Rgba([30, 30, 30, 255])
    } else {
      Rgba([210, 200, 190, 255])
    }
  });
  let piece: RgbaImage = ImageBuffer::from_fn(44, 44, |_, _| Rgba([210, 200, 190, 255]));

  (png(&board), png(&piece))
}

fn png(picture: &RgbaImage) -> Vec<u8> {
  let mut out = Vec::new();

  DynamicImage::ImageRgba8(picture.clone())
    .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
    .unwrap();
  out
}

fn base64_of(bytes: &[u8]) -> String {
  use base64::Engine;

  base64::engine::general_purpose::STANDARD.encode(bytes)
}
