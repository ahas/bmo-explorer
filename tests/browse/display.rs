use std::os::unix::fs::PermissionsExt;

use std::path::Path;
use std::time::{Duration, Instant};

use bmo_explorer::browse::display::*;

// A machine without the server or setpriv has nothing to start, as the run then says in its log.
#[tokio::test]
async fn a_private_display_picks_a_free_number_takes_only_its_own_cookie_and_stops() {
  if !program("BMO_XVFB_PATH", "/usr/bin/Xvfb").exists()
    || !program("BMO_SETPRIV_PATH", "/usr/bin/setpriv").exists()
  {
    return;
  }

  let dir = tempfile::tempdir().unwrap();
  let display = start(dir.path(), Instant::now() + Duration::from_secs(10)).await.unwrap();
  let pid = display.pid().unwrap();
  let number = display.display.trim_start_matches(':').to_owned();

  assert!(
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()),
    "{}",
    display.display
  );
  assert_ne!(display.display, std::env::var("DISPLAY").unwrap_or_default());
  assert!(Path::new(&format!("/tmp/.X11-unix/X{number}")).exists());
  assert_eq!(std::fs::metadata(&display.xauthority).unwrap().permissions().mode() & 0o777, 0o600);
  assert_eq!(std::fs::metadata(&display.runtime_dir).unwrap().permissions().mode() & 0o777, 0o700);

  let command = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap();
  let words: Vec<&str> = command.split('\0').collect();

  assert!(words.contains(&"-auth"));
  assert!(words.contains(&"tcp"));

  display.stop().await;

  assert!(!Path::new(&format!("/proc/{pid}")).exists());
}
