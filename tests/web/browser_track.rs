//! The human-like path a trusted gesture is walked by (docs/web-access.md, 「When the browser meets a
//! challenge」): the shape of the sampled path, since the loop's acts are only as human as the track under them.

use bmo_explorer::web::browser::track::{MAX_STEPS, PRESS_SETTLE_MS, Step, settle, walk};

fn spans(path: &[Step]) -> Vec<(f64, f64)> {
  path.windows(2).map(|pair| (pair[1].x - pair[0].x, pair[1].y - pair[0].y)).collect()
}

// The path leaves where the hand stood, arrives exactly on the point asked for, never walks more steps than its
// bound, and rests at every step it stands on.
#[test]
fn a_walked_path_starts_where_the_hand_stands_and_ends_on_its_point() {
  for (from, to) in [
    ((120.0, 90.0), (131.0, 320.0)),
    ((0.0, 0.0), (900.0, 40.0)),
    ((40.0, 200.0), (300.0, 200.0)),
  ] {
    let path = walk(from, to);
    let last = path.last().expect("a path is never walked empty");

    assert_eq!((path[0].x, path[0].y), from, "the hand is not teleported");
    assert_eq!((last.x, last.y), to, "the path arrives where it was asked");
    assert!(path.len() <= MAX_STEPS + 6, "{} steps for {from:?} to {to:?}", path.len());
    assert!(path.iter().all(|one| one.pause_ms > 0), "no step is walked without a breath");
  }
}

// The hand moves slowly at both ends of a pull and fast through its middle, walks past its gap and back over
// it, and never along a dead straight line.
#[test]
fn a_long_pull_eases_overshoots_and_wanders() {
  let (from, to) = ((40.0, 200.0), (300.0, 200.0));
  let path = walk(from, to);
  let steps = spans(&path);
  let middle = steps.len() / 2;
  let first = steps[0].0.abs() + steps[1].0.abs();
  let through = steps[middle].0.abs() + steps[middle + 1].0.abs();
  let back = steps[steps.len() - 3].0.abs() + steps[steps.len() - 4].0.abs();

  assert!(
    through > first,
    "the pull is walked slowly as it arrives, faster through its middle: {steps:?}"
  );
  assert!(through >= back, "and slows again as it settles back: {steps:?}");
  let farthest = path.iter().map(|one| one.x).fold(f64::MIN, f64::max);

  assert!(farthest > to.0 + 4.0, "a long pull walks past its point: {farthest}");
  let past = path
    .iter()
    .rposition(|one| one.x == farthest)
    .expect("the path walked its farthest point");

  assert!(
    path.len() - 1 > past,
    "the pull does not end past its gap: it stands there and walks back over it"
  );
  let off_line = path[1..path.len() - 1].iter().filter(|one| (one.y - to.1).abs() > 0.5).count();

  assert!(off_line >= 3, "the path bows off the straight line rather than ruling it: {path:?}");
}

// The short hop to a box carries its wander and its breaths but no overshoot, and the settle before a press
// stands on the last step of the path.
#[test]
fn a_short_hop_overshoots_nothing_and_a_settle_rests_the_last_step() {
  let path = walk((300.0, 300.0), (310.0, 306.0));
  let farthest = path.iter().map(|one| one.x).fold(f64::MIN, f64::max);

  assert!(farthest < 310.0 + 4.0, "a hop is not overshot: {farthest}");
  let mut path = walk((300.0, 300.0), (310.0, 306.0));

  settle(&mut path, PRESS_SETTLE_MS);
  assert_eq!(path.last().unwrap().pause_ms, PRESS_SETTLE_MS, "the hand stops before it presses");
}
