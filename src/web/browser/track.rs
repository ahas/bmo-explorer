//! The human-like path one trusted gesture is walked by, between two points of the page's view. The shape is
//! the one the studied roads of slider practice report as what a tracker accepts and a scripted jump does not
//! (docs/web-access.md, 「When the browser meets a challenge」): points sampled so the hand moves slowly at
//! both ends of the path and fast through its middle (ease-in-out), a short way past the target and back
//! over it on long pulls, a few pixels off the straight line, and waits that are never exactly even — a
//! pause as the path arrives, a hold under the press, uneven breaths between steps. The parameters follow the
//! shape published by itmorn/robot-mouse-track (Apache-2.0) and the Chinese slider practice write-ups; the
//! code is this repository's own, walked through the browser's trusted input path — the same write-ups report
//! synthetic in-page events as what gets detected, and none are dispatched here.

use rand::Rng;

/// One point of a walked path, and how long the walk rests at it before the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
  pub x: f64,
  pub y: f64,
  pub pause_ms: u64,
}

/// The longest path the generator ever walks.
pub const MAX_STEPS: usize = 40;
/// The pause before a press settles on its point, as a hand arriving.
pub const PRESS_SETTLE_MS: u64 = 90;
/// The hold under a press before the release, as a person's click holds.
pub const CLICK_HOLD_MS: u64 = 60;

// ease-in-out: samples cluster at both ends of the path and spread through its middle.
fn ease(at: f64) -> f64 {
  at * at * (3.0 - 2.0 * at)
}

/// The path from `from` to `to`, ending exactly on the target after its settle-back. The overshoot only
/// stands for a pull long enough to overshoot meaningfully; the short hop to a checkbox carries its wander,
/// its breaths and its arrival without an overshoot.
pub fn walk(from: (f64, f64), to: (f64, f64)) -> Vec<Step> {
  let span = to.0 - from.0;
  let rise = to.1 - from.1;
  let length = (span * span + rise * rise).sqrt();
  let mut rng = rand::rng();
  let steps = (length / 24.0).clamp(8.0, MAX_STEPS as f64) as usize;
  let wander = (length / 30.0).clamp(1.5, 6.0);
  let mut path = Vec::with_capacity(steps + 6);

  path.push(Step {
    x: from.0,
    y: from.1,
    pause_ms: rng.random_range(35..=90),
  });

  for step in 1..=steps {
    let at = ease(step as f64 / steps as f64);
    let sway = (step as f64 / steps as f64 * std::f64::consts::PI).sin() * wander;
    // The sway rides the line's normal, so the path bows a little to one side of it.
    let normal_x = if length > 0.0 { -rise / length } else { 0.0 };
    let normal_y = if length > 0.0 { span / length } else { 0.0 };

    path.push(Step {
      x: from.0 + span * at + normal_x * sway + rng.random_range(-1.0..=1.0),
      y: from.1 + rise * at + normal_y * sway + rng.random_range(-1.0..=1.0),
      pause_ms: rng.random_range(14..=42),
    });
  }

  if length > 120.0 {
    let pull_x = if span.abs() >= rise.abs() { span.signum() } else { 0.0 };
    let pull_y = if span.abs() < rise.abs() { rise.signum() } else { 0.0 };
    let over = rng.random_range(8.0..=18.0);

    path.push(Step {
      x: to.0 + pull_x * over + rng.random_range(-1.0..=1.0),
      y: to.1 + pull_y * over + rng.random_range(-1.0..=1.0),
      pause_ms: rng.random_range(70..=140),
    });
    path.push(Step {
      x: to.0 + pull_x * over * 0.4 + rng.random_range(-1.0..=1.0),
      y: to.1 + pull_y * over * 0.4 + rng.random_range(-1.0..=1.0),
      pause_ms: rng.random_range(25..=60),
    });
  }

  path.push(Step {
    x: to.0,
    y: to.1,
    pause_ms: rng.random_range(30..=80),
  });
  path
}

/// The last step's rest raised to `ms`: what stands before a press, so the arrival reads as the hand stopping
/// rather than the path ending.
pub fn settle(path: &mut [Step], ms: u64) {
  if let Some(last) = path.last_mut() {
    last.pause_ms = ms;
  }
}
