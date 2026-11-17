//! The reasoning loop a fetch road browser read makes at a challenge standing in its page
//! (docs/web-access.md, 「When the browser meets a challenge」): round after round it asks what the stage
//! stands for — wait its check, click its box, pull its slider to its gap, answer its words or pick its
//! pictures — acts through the browser's own trusted input walked on a human-like path, and watches the page
//! answer, until the stage has genuinely ended, the site ends it, or the round and time bounds are spent.
//! What is never done: no paid solving service, and no claim. A round that cleared is only ever reported as
//! having cleared because the retried read answered the page, and everything the loop did stands in the log
//! the refusal names.

use std::sync::Arc;

use base64::Engine;
use serde_json::Value;

use super::ddddocr;
use super::track::{self, Step};
use crate::web::browser::Gesture;
use crate::web::browser::Running;
use crate::web::clock::Clock;
use crate::web::vision::Vision;

/// How long a round gives the page's own check before the loop acts, and how often the watch looks again.
pub const WAIT_MS: u64 = 10_000;
pub const POLL_MS: u64 = 500;
/// What one read's loop is bound to: rounds, and total milliseconds. A read asks for these by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
  pub rounds: u32,
  pub ms: u64,
}

impl Default for Bounds {
  fn default() -> Self {
    Self {
      rounds: 6,
      ms: 90_000,
    }
  }
}

/// Where the loop stood when it ended, and what it did to get there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
  pub outcome: Outcome,
  pub rounds: u32,
  /// What each acting round did, as the refusal and the answer line name it.
  pub log: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
  /// The page moved off its challenge.
  Cleared,
  /// The site itself ended the challenge — its own wrong-answer or limit wording stood in the page.
  Ended { why: String },
  /// The loop's own bound came first: which bound, and where it stopped.
  Stopped { at: &'static str },
}

/// What one probe round read of the stage, as the page shows it.
#[derive(Clone, Debug, Default)]
struct Stage {
  dpr: f64,
  checkbox: Option<(f64, f64)>,
  slider: Option<Slider>,
  input: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Default)]
struct Slider {
  handle: (f64, f64),
  board_x: f64,
  board_scale: f64,
  piece: Option<Vec<u8>>,
  background: Option<Vec<u8>>,
}

/// The stage probe: what the page shows of its challenge, once, as plain JSON. Widget boxes ride the same
/// provider names the marker reads; a slider needs a handle and a wider board both named or classed for it,
/// and its pictures only cross when they can be copied without tainting — a cross-origin picture answers
/// null and the round is read by the vision road instead.
pub const STAGE: &str = r#"(() => {
  const dpr = window.devicePixelRatio || 1;
  const show = (el) => {
    el.scrollIntoView({ block: "center" });
    const r = el.getBoundingClientRect();
    return r.width >= 20 && r.height >= 10 ? r : null;
  };
  const said = (el) => `${el.src || ""} ${el.id || ""} ${el.name || ""} ${el.title || ""} ${el.className || ""} ${el.alt || ""}`;
  let boxPoint = null;
  for (const frame of document.querySelectorAll("iframe")) {
    if (!/captcha|challenge|recaptcha|hcaptcha|turnstile|funcaptcha|arkose/i.test(said(frame))) continue;
    const r = show(frame);
    if (!r || r.width < 60 || r.width > 1200 || r.height < 24) continue;
    boxPoint = { x: r.x + Math.min(31, r.width - 10), y: r.y + r.height / 2 };
    break;
  }
  let slider = null;
  const handle = document.querySelector(
    "[class*='handler' i],[class*='handle' i],[class*='slider-btn' i],[class*='slide-btn' i],[class*='drag' i],[class*='knob' i]");
  const board = document.querySelector(
    "[class*='bg' i],[class*='background' i],[class*='gap' i],[class*='slider' i] img,img[class*='slide' i]");
  if (handle && board && /(slide|drag|handler|handle|knob|captcha|verify)/i.test(said(handle) + " " + said(board))) {
    const hr = show(handle), br = show(board);
    if (hr && br && br.width > hr.width * 2) {
      // A plain image handle is a native drag source, so the browser takes a walked press on it as its picture
      // being moved and the widget's own listeners never see the pull. The handle is asked not to drag, and a
      // drag start it still answers, its own or from a picture inside it, is cancelled where it stands.
      handle.setAttribute("draggable", "false");

      if (!handle.dataset.dragOff) {
        handle.dataset.dragOff = "1";
        handle.addEventListener("dragstart", (event) => event.preventDefault(), true);
      }

      const copy = (el) => {
        try {
          if (el.tagName !== "IMG" || !el.complete || !el.naturalWidth) return null;
          const c = document.createElement("canvas");
          c.width = el.naturalWidth; c.height = el.naturalHeight;
          c.getContext("2d").drawImage(el, 0, 0);
          return c.toDataURL("image/png").split(",")[1] || null;
        } catch { return null; }
      };
      slider = {
        x: hr.x + hr.width / 2, y: hr.y + hr.height / 2,
        boardX: br.x, boardScale: br.width / (board.naturalWidth || br.width),
        piece: copy(handle), background: copy(board),
      };
    }
  }
  let input = null;
  for (const field of document.querySelectorAll("input,textarea")) {
    if (!/captcha|verif|answer|code|human/i.test(`${field.name || ""} ${field.id || ""} ${field.placeholder || ""}`)
      && !/(captcha|verif)/i.test(said(field))) continue;
    if (/hidden|submit|button/i.test(field.type || "")) continue;
    const r = show(field);
    if (r) { input = { x: r.x + r.width / 2, y: r.y + r.height / 2 }; break; }
  }
  return { dpr, boxPoint, slider, input };
})()"#;

/// Whether the challenge still stands in the page as it is, has been ended by the site itself, or is gone.
/// A solved widget — its response field filled — counts as gone. The reading of the answering page stays
/// `classify`'s; this marker only says what the next round should be.
pub const STILL: &str = r#"(() => {
  const token = Array.from(document.querySelectorAll("textarea")).some(
    (field) => /-response$/.test(field.name || "") && (field.value || "").length > 0,
  );
  if (token) return "gone";
  const words = (document.title + " " + (document.body ? document.body.innerText.slice(0, 4000) : ""))
    .toLowerCase();
  if (/incorrect|try again|too many attempts|verification (?:failed|expired)|challenge expired|access denied after/.test(words))
    return "ended";
  const widget = document.querySelector(
    "iframe[src*='recaptcha'], iframe[src*='hcaptcha'], iframe[src*='challenges.cloudflare.com'], " +
      "iframe[src*='turnstile'], iframe[src*='arkose'], iframe[src*='funcaptcha'], " +
      ".g-recaptcha, .h-captcha, .cf-turnstile, div.cap-widget",
  );
  if (widget) return "standing";
  if (/verify you are human|just a moment|attention required|checking your browser|security verification|client challenge|enable javascript and cookies/.test(words))
    return "standing";
  const markup = (document.documentElement ? document.documentElement.outerHTML : "")
    .slice(0, 300000)
    .toLowerCase();
  return /cf-chl-/.test(markup) ? "standing" : "gone";
})()"#;

/// The one question the vision road is asked per round that needs a picture read. Coordinates come back in
/// the screenshot's own pixels; the ask names no answer, only the shapes it may take.
pub const ASK: &str = "This is a screenshot of a web page where the visitor is being challenged to prove they are human. Answer with one JSON object and nothing else: {\"kind\":\"checkbox|slider|text|image|interstitial|none|ended\",\"click\":[x,y]|null,\"gap_x\":NUMBER|null,\"text\":null,\"note\":null}. 'click' is where a box or confirm or a named picture stands; 'gap_x' is the left edge of the hole a slider piece must reach, in image pixels; 'text' is what a word prompt asks the visitor to type; 'ended' means the site itself refused and no round will pass; 'interstitial' means a check that runs on its own.";

/// The whole attempt: rounds until the stage ends, the site ends it, or the bounds are spent. Whatever it
/// says, the caller retries the read once and the page's own classification tells what stood.
pub async fn attempt(
  running: &dyn Running,
  clock: &dyn Clock,
  bounds: &Bounds,
  vision: Option<&Arc<dyn Vision>>,
) -> Attempt {
  let began = clock.now();
  let mut log: Vec<String> = Vec::new();
  let mut place = (120.0, 90.0); // where the hand came from, kept between rounds as a person's is

  for round in 1..=bounds.rounds {
    // Every round opens by giving the page's own check its way out, bounded by what the read has left.
    let give = WAIT_MS.min(began + bounds.ms.saturating_sub(clock.now()));
    match wait_through(running, clock, give).await {
      Watched::Gone => {
        return Attempt {
          outcome: Outcome::Cleared,
          rounds: round,
          log,
        };
      }
      Watched::Ended(why) => {
        return Attempt {
          outcome: Outcome::Ended { why },
          rounds: round,
          log,
        };
      }
      Watched::Standing => {}
    }

    if clock.now() + POLL_MS * 4 > began + bounds.ms {
      return Attempt {
        outcome: Outcome::Stopped {
          at: "the time bound",
        },
        rounds: round,
        log,
      };
    }

    let stage = probe(running).await;
    let said = act(running, clock, &stage, vision, &mut place).await;
    // Every act the loop makes stands in the log, the one that worked included: the reader of a cleared page
    // is told what moved it, and the reader of a refusal sees each round that did not.
    log.push(said);

    // The acting round's own answer, once: what the page says about it, not what the act hoped, and bounded by
    // what the read has left of its own time.
    let left = WAIT_MS.min(bounds.ms.saturating_sub(clock.now() - began));

    match wait_through(running, clock, left).await {
      Watched::Gone => {
        return Attempt {
          outcome: Outcome::Cleared,
          rounds: round,
          log,
        };
      }
      Watched::Ended(why) => {
        return Attempt {
          outcome: Outcome::Ended { why },
          rounds: round,
          log,
        };
      }
      Watched::Standing => log.push("the challenge still stood".into()),
    }
  }

  Attempt {
    outcome: Outcome::Stopped {
      at: "the round bound",
    },
    rounds: bounds.rounds,
    log,
  }
}

enum Watched {
  Gone,
  Standing,
  Ended(String),
}

async fn wait_through(running: &dyn Running, clock: &dyn Clock, ms: u64) -> Watched {
  let until = clock.now() + ms;

  loop {
    if let Ok(said) = running.evaluate(STILL).await {
      match said.as_str().unwrap_or("standing") {
        "gone" => return Watched::Gone,
        "ended" => {
          let why = running
            .evaluate("(() => document.body ? document.body.innerText.slice(0, 160) : \"\")()")
            .await
            .ok()
            .and_then(|page| page.as_str().map(str::to_owned))
            .unwrap_or_else(|| "its own refusal".into());

          return Watched::Ended(why);
        }
        _ => {}
      }
    }

    if clock.now() + POLL_MS > until {
      return Watched::Standing;
    }

    clock.sleep(POLL_MS).await;
  }
}

async fn probe(running: &dyn Running) -> Stage {
  let Ok(found) = running.evaluate(STAGE).await else {
    return Stage::default();
  };

  let mut stage = Stage {
    dpr: found["dpr"].as_f64().unwrap_or(1.0),
    checkbox: point(&found["boxPoint"]),
    input: point(&found["input"]),
    slider: None,
  };

  if !found["slider"].is_null() {
    let decoded =
      |b64: &Value| b64.as_str().and_then(|one| base64::engine::general_purpose::STANDARD.decode(one).ok());

    stage.slider = Some(Slider {
      handle: (
        found["slider"]["x"].as_f64().unwrap_or(0.0),
        found["slider"]["y"].as_f64().unwrap_or(0.0),
      ),
      board_x: found["slider"]["boardX"].as_f64().unwrap_or(0.0),
      board_scale: found["slider"]["boardScale"].as_f64().unwrap_or(1.0),
      piece: decoded(&found["slider"]["piece"]),
      background: decoded(&found["slider"]["background"]),
    });
  }

  stage
}

fn point(found: &Value) -> Option<(f64, f64)> {
  let (x, y) = (found["x"].as_f64()?, found["y"].as_f64()?);

  (x >= 0.0 && y >= 0.0).then_some((x, y))
}

// The round's act, named by the words the caller logs it under.
async fn act(
  running: &dyn Running,
  clock: &dyn Clock,
  stage: &Stage,
  vision: Option<&Arc<dyn Vision>>,
  place: &mut (f64, f64),
) -> String {
  // The gap first by the ported ddddocr reading, where the pictures crossed the canvas untainted.
  if let Some(slider) = &stage.slider {
    if let (Some(piece), Some(background)) = (&slider.piece, &slider.background) {
      let gap = ddddocr::slide_match(piece, background)
        .map(|(x, _)| x)
        .or_else(|| ddddocr::slide_comparison(piece, background));

      if let Some(gap) = gap {
        let at = slider.board_x + gap * slider.board_scale;
        drag(running, clock, *place, slider.handle, (at, slider.handle.1)).await;
        *place = (at, slider.handle.1);

        return "dragged its slider to the gap the ported ddddocr matcher read".into();
      }
    }

    // Where the matcher had no pictures to read, the round is read off the screen.
    if let Some(found) = look(running, vision).await
      && found["kind"].as_str() == Some("slider")
      && let Some(gap) = found["gap_x"].as_f64()
    {
      let at = slider.board_x + gap / stage.dpr.max(1.0) * slider.board_scale;
      drag(running, clock, *place, slider.handle, (at, slider.handle.1)).await;
      *place = (at, slider.handle.1);

      return "dragged its slider to the gap the vision road read".into();
    }
  }

  if let Some(box_point) = stage.checkbox {
    click(running, clock, *place, box_point).await;
    *place = box_point;

    return "clicked its box".into();
  }

  // What the page asks, read off the screen: words to type, a thing to click, or nothing to act on.
  if let Some(found) = look(running, vision).await {
    match found["kind"].as_str().unwrap_or("none") {
      "text" => {
        if let Some(prompt) = text_of(&found) {
          if let Some(field) = stage.input {
            click(running, clock, *place, field).await;
          }

          let _ = running.keys(&prompt, true).await;

          return format!("answered its words — {prompt}");
        }
      }
      "image" | "checkbox" => {
        if let Some((x, y)) = click_of(&found) {
          let at = (x / stage.dpr.max(1.0), y / stage.dpr.max(1.0));

          click(running, clock, *place, at).await;
          *place = at;

          return "clicked where the picture asked".into();
        }
      }
      _ => {}
    }
  }

  // A stage nothing can be read of: the round stood by and watched its check.
  clock.sleep(POLL_MS).await;
  "watched its check run".into()
}

fn text_of(found: &Value) -> Option<String> {
  found["text"].as_str().map(str::to_owned).filter(|one| !one.is_empty())
}

fn click_of(found: &Value) -> Option<(f64, f64)> {
  found["click"].get(0)?.as_f64().zip(found["click"].get(1)?.as_f64())
}

// One look at the screen for the round that needs it read; none where the road is not stood up or the
// screenshot fails — a round then acts on what the page itself showed.
async fn look(running: &dyn Running, vision: Option<&Arc<dyn Vision>>) -> Option<Value> {
  let png = running.screenshot().await.ok()?;

  vision?.read(&png, ASK).await.ok()
}

// The walk to a point and one press there, on the human path: the hand's arrival, the settle, the hold.
async fn click(running: &dyn Running, clock: &dyn Clock, from: (f64, f64), to: (f64, f64)) {
  let mut path = track::walk(from, to);

  track::settle(&mut path, track::PRESS_SETTLE_MS);
  walk(running, clock, &path).await;
  let _ = running.gesture(&Gesture::Down { x: to.0, y: to.1 }).await;
  clock.sleep(track::CLICK_HOLD_MS).await;
  let _ = running.gesture(&Gesture::Up { x: to.0, y: to.1 }).await;
}

// The pull of a slider: the hand reaches the handle and settles, presses, walks to the gap on the human path
// — the overshoot and its correction stand inside the pull itself — and lets go over the gap.
async fn drag(
  running: &dyn Running,
  clock: &dyn Clock,
  from: (f64, f64),
  handle: (f64, f64),
  to: (f64, f64),
) {
  let mut approach = track::walk(from, handle);

  track::settle(&mut approach, track::PRESS_SETTLE_MS);
  walk(running, clock, &approach).await;
  let _ = running
    .gesture(&Gesture::Down {
      x: handle.0,
      y: handle.1,
    })
    .await;
  clock.sleep(track::CLICK_HOLD_MS).await;

  let mut pull = track::walk(handle, to);

  pull.remove(0);
  walk(running, clock, &pull).await;
  let _ = running.gesture(&Gesture::Up { x: to.0, y: to.1 }).await;
}

async fn walk(running: &dyn Running, clock: &dyn Clock, path: &[Step]) {
  for step in path {
    let _ = running
      .gesture(&Gesture::Move {
        x: step.x,
        y: step.y,
      })
      .await;

    if step.pause_ms > 0 {
      clock.sleep(step.pause_ms).await;
    }
  }
}

/// The whole attempt in the words a refusal or an answer line carries: rounds, bound where stopped, log.
impl Attempt {
  pub fn said(&self) -> String {
    let done = if self.log.is_empty() { "nothing to act on".to_owned() } else { self.log.join("; ") };

    match &self.outcome {
      Outcome::Cleared => format!("cleared in {} round(s): {done}", self.rounds),
      Outcome::Ended { why } => {
        format!("ended by the site itself after {} round(s) ({done}); its page said: {why}", self.rounds)
      }
      Outcome::Stopped { at } => format!(
        "attempted over {} round(s) ({done}) and stopped at {at}; the challenge was not solved",
        self.rounds
      ),
    }
  }
}
