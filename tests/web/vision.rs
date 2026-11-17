//! The vision road's own reading: the answer format the loop asks for, read out of whatever the model wrapped
//! around it, and the reader a data root without a saved key does not stand up (docs/web-access.md, 「When the
//! browser meets a challenge」).

use std::path::Path;

use bmo_explorer::web::vision::{Gateway, json_of};

// The one JSON object of an answer is taken whole, however the model fenced or spoke around it — including an
// answer whose note holds braces.
#[test]
fn the_object_a_vision_answer_wraps_is_read_out_of_the_wrapping() {
  let said = json_of("```json\n{\"kind\":\"slider\",\"click\":null,\"note\":null}\n```").unwrap();

  assert_eq!(said["kind"], "slider");
  let spoken =
    json_of("The challenge is a slider. {\"kind\":\"slider\",\"gap_x\":312,\"note\":\"{ not a brace\"}");
  let spoken = spoken.expect("prose around the object is not a reason it cannot be read");

  assert_eq!(spoken["gap_x"], 312);
  assert_eq!(spoken["note"], "{ not a brace", "a brace inside a string does not end the object");
}

// An answer that never closes its object, or holds none, is read as nothing rather than as a guess.
#[test]
fn a_vision_answer_holding_no_whole_object_is_read_as_nothing() {
  assert!(
    json_of("{\"kind\":\"text\",\"text\":\"73").is_none(),
    "a cut-off object is not half-read"
  );
  assert!(json_of("I cannot read this picture.").is_none());
  assert!(json_of("").is_none());
}

// Where the saved key for the gateway is not there to read, no road stands up: the loop then acts on what the
// page itself showed and says so.
#[test]
fn no_vision_road_stands_up_where_the_saved_key_is_not_there() {
  let root = tempfile::tempdir().expect("a data root of the test's own");

  assert!(Gateway::open(Path::new(root.path())).is_none());
}
