//! A browser's own profile folder and how it is taken away (docs/web-access.md, 「How a leftover profile is
//! reclaimed」) — Chrome's temporary profile, or Firefox's copy of the owner's profile: every process still
//! opened on it is stopped first — the browser, a helper naming a directory under it, its display, the GPU
//! process finding its cache through the profile — and it is removed only once none is left, and removed again
//! where a dying helper wrote it back.

use std::future::Future;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

pub const PREFIX: &str = "bmo-web-chrome-";
pub const OWNER_FILE: &str = "bmo-owner.pid";
const STALE_UNOWNED_MS: u64 = 60 * 60 * 1_000;

/// How the processes on a profile are found and stopped: the machine's, or a test's stand-in.
pub trait ProcessControl: Send + Sync {
  fn processes<'a>(&'a self, profile: &'a Path) -> Pin<Box<dyn Future<Output = Vec<u32>> + Send + 'a>>;
  fn kill(&self, pid: u32);
  fn sleep(&self, ms: u64) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The machine's processes, read off `/proc`.
pub struct LiveProcesses;

// Whether one argument or variable names the profile: as itself, as an option's value, or as a directory
// under it.
fn names(part: &str, profile: &str) -> bool {
  part == profile || part.contains(&format!("={profile}")) || part.contains(&format!("{profile}/"))
}

fn own_uid() -> u32 {
  // SAFETY: getuid reads this process's own user id and cannot fail.
  unsafe { libc::getuid() }
}

impl ProcessControl for LiveProcesses {
  fn processes<'a>(&'a self, profile: &'a Path) -> Pin<Box<dyn Future<Output = Vec<u32>> + Send + 'a>> {
    Box::pin(async move {
      let profile = profile.to_string_lossy().into_owned();
      let uid = own_uid();
      let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
      };
      let mut found = Vec::new();

      for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {
          continue;
        };
        let dir = entry.path();

        if std::fs::metadata(&dir).map(|meta| meta.uid()).ok() != Some(uid) {
          continue;
        }

        let read = |file: &str| {
          std::fs::read(dir.join(file)).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        };

        if read("cmdline").is_ok_and(|line| line.split('\0').any(|part| names(part, &profile)))
          || read("environ").is_ok_and(|vars| vars.split('\0').any(|part| names(part, &profile)))
        {
          found.push(pid);
        }
      }

      found
    })
  }

  fn kill(&self, pid: u32) {
    // SAFETY: a signal to a process of this user that is opened on the profile being removed.
    unsafe {
      libc::kill(pid as i32, libc::SIGKILL);
    }
  }

  fn sleep(&self, ms: u64) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(tokio::time::sleep(Duration::from_millis(ms)))
  }
}

// Whether every process opened on the profile is gone; one found later in the wait, a helper a dying
// browser forked after the first sweep, is stopped too.
async fn stop_processes(profile: &Path, control: &dyn ProcessControl) -> bool {
  for _ in 0..200 {
    let left = control.processes(profile).await;

    if left.is_empty() {
      return true;
    }

    for pid in left {
      control.kill(pid);
    }

    control.sleep(25).await;
  }

  false
}

/// Stops every process still pointing at the profile, removes it once none is left, and looks again; whether
/// it is gone.
pub async fn remove(profile: &Path, control: &dyn ProcessControl) -> bool {
  for _ in 0..3 {
    if !stop_processes(profile, control).await {
      return false;
    }

    let _ = std::fs::remove_dir_all(profile);
    control.sleep(150).await;

    if !profile.exists() {
      return true;
    }
  }

  false
}

fn alive(pid: i32) -> bool {
  // SAFETY: signal 0 asks only whether the process exists.
  let answered = unsafe { libc::kill(pid, 0) };

  answered == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Every Chrome profile in `dir` whose owner is gone, or that stands unowned past an hour, removed.
pub async fn reap_stale(dir: &Path, now_ms: u64, control: &dyn ProcessControl) {
  let chrome = |path: &Path| {
    path
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| name.starts_with(PREFIX))
  };

  reap_stale_where(dir, &chrome, now_ms, control).await;
}

/// Every folder in `dir` that `ours` takes for a browser profile of this layer's, whose owner is gone or that
/// stands unowned past an hour, removed; no other folder is looked at.
pub async fn reap_stale_where(
  dir: &Path,
  ours: &(dyn Fn(&Path) -> bool + Sync),
  now_ms: u64,
  control: &dyn ProcessControl,
) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return;
  };
  let profiles: Vec<PathBuf> =
    entries.flatten().map(|entry| entry.path()).filter(|path| ours(path)).collect();

  for profile in profiles {
    let owner = std::fs::read_to_string(profile.join(OWNER_FILE))
      .ok()
      .map(|text| text.trim().parse::<i32>().unwrap_or(0));
    let stale = match owner {
      Some(pid) => pid <= 1 || !alive(pid),
      None => std::fs::metadata(&profile)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .is_some_and(|at| now_ms.saturating_sub(at.as_millis() as u64) >= STALE_UNOWNED_MS),
    };

    if stale {
      remove(&profile, control).await;
    }
  }
}
