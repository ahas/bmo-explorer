use bmo_explorer::web::chrome::display::*;
use std::path::Path;

// Carried from packages/web/private-display.test.ts: a detached environment keeps the caller's own variables and none of a person's session
#[test]
fn a_detached_environment_keeps_the_callers_own_variables_and_none_of_a_persons_session() {
  let base = [
    ("PATH", "/usr/bin"),
    ("HOME", "/srv/user"),
    ("DISPLAY", ":0"),
    ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/bus"),
    ("SSH_AUTH_SOCK", "/tmp/agent"),
  ]
  .map(|(name, value)| (name.to_owned(), value.to_owned()));
  let environment =
    detached_environment(base, ":7", Path::new("/p/display/Xauthority"), Path::new("/p/display/runtime"));

  assert_eq!(environment["PATH"], "/usr/bin");
  assert_eq!(environment["HOME"], "/srv/user");
  assert_eq!(environment["DISPLAY"], ":7");
  assert_eq!(environment["DBUS_SESSION_BUS_ADDRESS"], "disabled:");
  assert_eq!(environment["XDG_RUNTIME_DIR"], "/p/display/runtime");
  assert!(!environment.contains_key("SSH_AUTH_SOCK"));
}

#[test]
fn the_authority_entry_names_every_address_and_display() {
  let entry = authority_entry(&[1; 16]);

  assert_eq!(&entry[..6], &[0xff, 0xff, 0, 0, 0, 0]);
  assert_eq!(entry.len(), 2 + 2 + 2 + 2 + 18 + 2 + 16);
}
