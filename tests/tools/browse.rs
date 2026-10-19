use std::path::Path;

use bmo_explorer::tool::manifest::Manifest;
use bmo_explorer::tool::{Served, Session};
use bmo_explorer::tools::browse::{Browse, BrowseInput, TOOLS, launch, task_text, task_words};
use bmo_explorer::tools::launching::{Mode, NO_ROOM, Turn};
use serde_json::json;

fn turn(chat: Option<i64>, via: Option<&str>) -> Turn {
  let mut turn = Turn::default();

  if let Some(chat) = chat {
    turn.variables.insert("BMO_TURN_CHAT".into(), chat.to_string());
  }
  if let Some(via) = via {
    turn.variables.insert("BMO_TURN_VIA".into(), via.into());
  }
  turn
}

fn asked(input: serde_json::Value) -> BrowseInput {
  serde_json::from_value(input).unwrap()
}

#[test]
fn the_manifest_declares_the_entry_as_its_tool_does() {
  let manifest =
    Manifest::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/browse"), "browse").unwrap();

  assert_eq!(manifest.differences(TOOLS), Vec::<String>::new());
}

#[test]
fn a_browse_goes_async_by_default_and_as_asked_in_its_own_room() {
  let launch =
    launch(&turn(Some(-10), Some("telegram")), &asked(json!({ "task": " look at the page " }))).unwrap();

  assert_eq!(
    launch.args,
    ["browse", "--chat", "-10", "--by", "chat:-10", "--via", "telegram", "--mode", "async"]
  );
  assert_eq!(launch.input, "look at the page");

  let sync = launch_mode(Some(-10), json!({ "task": "t", "mode": "sync" }));

  assert_eq!(sync, Mode::Sync);
}

fn launch_mode(chat: Option<i64>, input: serde_json::Value) -> Mode {
  let launch = launch(&turn(chat, None), &asked(input)).unwrap();
  let at = launch.args.iter().position(|word| word == "--mode").unwrap();

  serde_json::from_value(json!(launch.args[at + 1])).unwrap()
}

#[test]
fn a_browse_for_another_room_reports_there_async_set_off_by_this_turn_s_room() {
  let launch =
    launch(&turn(Some(-10), None), &asked(json!({ "task": "t", "chat": -20, "mode": "sync" }))).unwrap();

  assert_eq!(launch.args, ["browse", "--chat", "-20", "--by", "chat:-10", "--mode", "async"]);
}

#[test]
fn a_browse_naming_no_room_outside_a_room_s_turn_is_refused() {
  assert_eq!(launch(&turn(None, None), &asked(json!({ "task": "t" }))), Err(NO_ROOM.to_owned()));
}

#[test]
fn an_offered_login_rides_apart_from_the_task_and_comes_off_it_again() {
  let text = task_text("sign in and read the invoice", Some(" user / pass "));

  assert_eq!(
    text,
    "sign in and read the invoice\n\nCredentials their holder offered for this task: user / pass"
  );
  assert_eq!(task_words(&text), "sign in and read the invoice");
  assert_eq!(task_text("t", Some("  ")), "t");
}

#[tokio::test]
async fn a_call_outside_a_room_s_turn_naming_no_room_is_refused_before_anything_starts() {
  let answer = Browse.call(&Session::default(), json!({ "task": "t" })).await.unwrap();

  assert!(answer.is_error);
  assert_eq!(answer.plain(), NO_ROOM);
}
