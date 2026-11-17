use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bmo_explorer::web::browser::profile::*;

// Processes that will not die for a number of looks, and a helper that writes the profile back once.
struct Stubborn {
  looks_left: AtomicUsize,
  writes_back: Mutex<Option<PathBuf>>,
  killed: AtomicUsize,
}

impl ProcessControl for Stubborn {
  fn processes<'a>(&'a self, _: &'a Path) -> Pin<Box<dyn Future<Output = Vec<u32>> + Send + 'a>> {
    let left = self.looks_left.load(Ordering::SeqCst);

    if left > 0 {
      self.looks_left.fetch_sub(1, Ordering::SeqCst);
    }

    Box::pin(async move { if left > 0 { vec![4_242] } else { Vec::new() } })
  }

  fn kill(&self, _: u32) {
    self.killed.fetch_add(1, Ordering::SeqCst);
  }

  fn sleep(&self, _: u64) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    if let Some(profile) = self.writes_back.lock().unwrap().take() {
      std::fs::create_dir_all(profile.join("cache")).unwrap();
    }

    Box::pin(std::future::ready(()))
  }
}

fn profile_in(dir: &Path, owner: &str) -> PathBuf {
  let profile = dir.join(format!("{PREFIX}test"));

  std::fs::create_dir_all(&profile).unwrap();
  std::fs::write(profile.join(OWNER_FILE), owner).unwrap();
  profile
}

// Carried from packages/web/browser-fetch.test.ts: a stale profile is kept while a process opened on it will not die, and reaped once none is left
#[tokio::test]
async fn a_stale_profile_is_kept_while_a_process_on_it_will_not_die_and_reaped_once_none_is_left() {
  let dir = tempfile::tempdir().unwrap();
  let profile = profile_in(dir.path(), "999999999");
  let stubborn = Stubborn {
    looks_left: AtomicUsize::new(usize::MAX),
    writes_back: Mutex::new(None),
    killed: AtomicUsize::new(0),
  };

  reap_stale(dir.path(), 0, &stubborn).await;
  assert!(profile.exists());
  assert!(stubborn.killed.load(Ordering::SeqCst) > 0);

  stubborn.looks_left.store(2, Ordering::SeqCst);
  reap_stale(dir.path(), 0, &stubborn).await;
  assert!(!profile.exists());
}

// Carried from packages/web/browser-fetch.test.ts: a profile a dying helper writes again after its removal is removed again
#[tokio::test]
async fn a_profile_a_dying_helper_writes_again_is_removed_again() {
  let dir = tempfile::tempdir().unwrap();
  let profile = profile_in(dir.path(), "999999999");
  let helper = Stubborn {
    looks_left: AtomicUsize::new(0),
    writes_back: Mutex::new(Some(profile.clone())),
    killed: AtomicUsize::new(0),
  };

  assert!(remove(&profile, &helper).await);
  assert!(!profile.exists());
}

#[tokio::test]
async fn a_profile_whose_owner_lives_is_left_alone() {
  let dir = tempfile::tempdir().unwrap();
  let profile = profile_in(dir.path(), &std::process::id().to_string());

  reap_stale(dir.path(), 0, &LiveProcesses).await;
  assert!(profile.exists());
}
