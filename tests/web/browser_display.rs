use bmo_explorer::web::browser::display::*;
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

// A killed display's lock file and socket are swept before the next one starts, and a running server's are kept
// (docs/web-access.md, 「How the browser is started」).
#[test]
fn a_stale_display_lock_and_its_socket_are_swept_and_a_live_one_is_kept() {
  let root = tempfile::tempdir().expect("a scratch folder");
  let locks = root.path();
  let sockets = root.path().join(".X11-unix");

  std::fs::create_dir(&sockets).expect("the sockets folder");

  // A process that ran and ended stands for a server killed with its lock left behind.
  let mut gone = std::process::Command::new("sleep").arg("0").spawn().expect("a process");
  let gone_pid = gone.id();
  gone.wait().expect("it ends");
  let live = std::process::id();

  std::fs::write(locks.join(".X7-lock"), format!("{gone_pid:>10}\n")).unwrap();
  std::fs::write(sockets.join("X7"), "").unwrap();
  std::fs::write(locks.join(".X8-lock"), format!("{live:>10}\n")).unwrap();
  std::fs::write(sockets.join("X8"), "").unwrap();
  std::fs::write(locks.join(".X9-lock"), "not a pid\n").unwrap();
  std::fs::write(locks.join("unrelated-lock"), "").unwrap();

  let removed = sweep_stale_displays(locks, &sockets);

  assert_eq!(removed, 2, "the lock of a gone server and one naming no server");
  assert!(!locks.join(".X7-lock").exists() && !sockets.join("X7").exists());
  assert!(
    locks.join(".X8-lock").exists() && sockets.join("X8").exists(),
    "a running server's display stands"
  );
  assert!(!locks.join(".X9-lock").exists());
  assert!(locks.join("unrelated-lock").exists());
}
