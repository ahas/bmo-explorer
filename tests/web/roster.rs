//! The settings built in beside the web layer's modules: the engine roster and the detached Chrome.

use bmo_explorer::web::browser::detached::DETACHED;
use bmo_explorer::web::search::roster::ROSTER;

#[test]
fn every_engine_is_named_once() {
  let mut names: Vec<&str> = ROSTER.engine.iter().map(|one| one.name.as_str()).collect();

  names.sort_unstable();
  names.dedup();
  assert_eq!(names.len(), ROSTER.engine.len());
}

#[test]
fn every_cooldown_line_names_a_rest() {
  assert!(ROSTER.cooldown.iter().all(|line| ROSTER.rests.named(&line.rest).is_some()));
}

#[test]
fn the_session_bus_is_among_what_never_reaches_a_chrome() {
  assert!(DETACHED.session_variables.iter().any(|name| name == "DBUS_SESSION_BUS_ADDRESS"));
  assert!(DETACHED.detached_arguments.iter().any(|arg| arg == "--password-store=basic"));
}
