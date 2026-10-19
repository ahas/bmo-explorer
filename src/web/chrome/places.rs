//! The machine's Chrome places (docs/web-access.md, 「Lifetime and cleanup」): a Chrome holds one for as long
//! as it lives, so no more than this many run on the machine at once, whichever process started them. A
//! place is an advisory lock on a file in the temporary directory — the same lock `flock(1)` takes, so
//! every process on the machine counts the same places — let go with the process holding it however it ends.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

use crate::web::clock::now_ms;

const POLL: Duration = Duration::from_millis(250);

/// The places' lock files: `BMO_CHROME_PLACES` of them, three where it names none, in
/// `BMO_CHROME_PLACES_DIR` or the temporary directory.
pub fn place_paths() -> Vec<PathBuf> {
  let count = std::env::var("BMO_CHROME_PLACES")
    .ok()
    .and_then(|named| named.parse::<usize>().ok())
    .filter(|n| *n > 0)
    .unwrap_or(3);
  let dir = std::env::var_os("BMO_CHROME_PLACES_DIR")
    .filter(|dir| !dir.is_empty())
    .map_or_else(std::env::temp_dir, PathBuf::from);

  (1..=count).map(|at| dir.join(format!("bmo-chrome-place-{at}.lock"))).collect()
}

/// A place held; given back when dropped.
pub struct Place {
  _file: File,
}

fn take(path: &PathBuf) -> Option<Place> {
  let file = OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
  // SAFETY: an advisory lock on a file this process holds open.
  let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;

  taken.then_some(Place { _file: file })
}

/// The first free place, waited for until the deadline; none where every place stayed taken.
pub async fn take_any(paths: &[PathBuf], until_ms: u64) -> Option<Place> {
  loop {
    if let Some(place) = paths.iter().find_map(take) {
      return Some(place);
    }

    if now_ms() + POLL.as_millis() as u64 > until_ms {
      return None;
    }

    tokio::time::sleep(POLL).await;
  }
}
