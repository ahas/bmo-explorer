//! The one attempt a fetch road browser read makes at a challenge standing in its page
//! (docs/web-access.md, 「When the browser meets a challenge」): the site's own check handed through to its
//! end, the challenge's own box clicked once where one stands, and nothing else — no reading of pictures,
//! no answering of puzzles, no solving service. The retry of the read itself is the caller's: this module
//! only works on the page as it stands and waits for it to move.

use crate::web::browser::Running;
use crate::web::clock::Clock;

/// How long each of the two waits gives the page's own check, and how often they look again. The whole
/// attempt is thus bounded at a little over twenty seconds, never loops, and never comes twice in one read.
pub const WAIT_MS: u64 = 10_000;
pub const POLL_MS: u64 = 500;

/// Whether a challenge still stands in the page as it is. An answer field a solved widget has filled counts
/// as cleared: the widget is done and the page is moving. The reading of the page that answers is
/// `classify`'s, not this marker's: the marker only says when a wait may end, so it errs towards standing.
pub const STILL: &str = r#"(() => {
  const token = Array.from(document.querySelectorAll("textarea")).some(
    (field) => /-response$/.test(field.name || "") && (field.value || "").length > 0,
  );
  if (token) return false;
  const widget = document.querySelector(
    "iframe[src*='recaptcha'], iframe[src*='hcaptcha'], iframe[src*='challenges.cloudflare.com'], " +
      "iframe[src*='turnstile'], iframe[src*='arkose'], iframe[src*='funcaptcha'], " +
      ".g-recaptcha, .h-captcha, .cf-turnstile, div.cap-widget",
  );
  if (widget) return true;
  const words = (document.title + " " + (document.body ? document.body.innerText.slice(0, 3000) : ""))
    .toLowerCase();
  if (/verify you are human|just a moment|attention required|checking your browser|security verification|client challenge|enable javascript and cookies/.test(words))
    return true;
  const markup = (document.documentElement ? document.documentElement.outerHTML : "")
    .slice(0, 300000)
    .toLowerCase();
  return /cf-chl-/.test(markup);
})()"#;

/// The click point of the challenge's own box, as `{x, y}` in view coordinates, or null where the page shows
/// no widget box to click: the highest challenge-named iframe the document holds, scrolled into view,
/// clicked at its left where a checkbox challenge stands its box, at its middle height. Boxes too small or
/// too wide to be one widget are left alone, and a point fallen out of the view is no point.
pub const BOX: &str = r#"(() => {
  const words = /captcha|challenge|recaptcha|hcaptcha|turnstile|funcaptcha|arkose/i;
  let best = null;
  for (const frame of document.querySelectorAll("iframe")) {
    if (!words.test(`${frame.src || ""} ${frame.id || ""} ${frame.title || ""} ${frame.className || ""}`))
      continue;
    frame.scrollIntoView({ block: "center" });
    const box = frame.getBoundingClientRect();
    if (box.width < 60 || box.height < 24 || box.width > 1200) continue;
    if (!best || box.top < best.top) best = box;
  }
  if (!best) return null;
  const x = best.left + Math.min(31, best.width - 10);
  const y = best.top + best.height / 2;
  if (x < 0 || y < 0 || x > window.innerWidth || y > window.innerHeight) return null;
  return { x, y };
})()"#;

// One wait: the marker looked at until it says gone, or the wait is spent. A marker that cannot be read —
// the page between documents — counts as standing, which is what a person watching would see.
async fn wait_through(running: &dyn Running, clock: &dyn Clock) -> bool {
  let until = clock.now() + WAIT_MS;

  loop {
    match running.evaluate(STILL).await {
      Ok(standing) if standing.as_bool() != Some(true) => return true,
      _ => {}
    }

    if clock.now() + POLL_MS > until {
      return false;
    }

    clock.sleep(POLL_MS).await;
  }
}

// Where the widget's box stands, or null where the page shows none or the page cannot be read.
async fn box_point(running: &dyn Running) -> Option<(f64, f64)> {
  let found = running.evaluate(BOX).await.ok()?;
  let x = found.get("x")?.as_f64()?;
  let y = found.get("y")?.as_f64()?;

  Some((x, y))
}

/// The one attempt, in its documented order: hand the check through, click its box where one stands and hand
/// through once more. Whether the page moved off its challenge by the end; whatever it says, the caller
/// retries the read once and the page's own classification tells what stood.
pub async fn attempt(running: &dyn Running, clock: &dyn Clock) -> bool {
  if wait_through(running, clock).await {
    return true;
  }

  let Some((x, y)) = box_point(running).await else {
    return false;
  };

  running.click(x, y).await.is_ok() && wait_through(running, clock).await
}

// The same point in whole pixels, for the two events of one click: a press and a release that a widget sees
// as one person's click.
pub(crate) fn click_point(x: f64, y: f64) -> (u32, u32) {
  (x.round().max(0.0) as u32, y.round().max(0.0) as u32)
}
