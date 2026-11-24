//! The owner's Firefox profile and the copy a browser runs on (docs/web-access.md, 「Which browser」): his default
//! profile found as Firefox finds it, a copy that leaves his passwords, history and locks out and leaves his
//! profile as it stood, and removal that never reaches his profile.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use bmo_explorer::web::browser::copy::*;
use bmo_explorer::web::browser::profile::{OWNER_FILE, ProcessControl};

// A machine on which no process stands on any folder, and every wait is over at once.
struct NoProcesses;

impl ProcessControl for NoProcesses {
  fn processes<'a>(&'a self, _: &'a Path) -> Pin<Box<dyn Future<Output = Vec<u32>> + Send + 'a>> {
    Box::pin(std::future::ready(Vec::new()))
  }

  fn kill(&self, _: u32) {}

  fn sleep(&self, _: u64) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(std::future::ready(()))
  }
}

// Every file under `root` with its bytes, and every link with where it points, by its path from `root`.
fn contents(root: &Path) -> BTreeMap<String, Vec<u8>> {
  fn walk(root: &Path, at: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(at).unwrap().flatten() {
      let path = entry.path();
      let kind = entry.file_type().unwrap();
      let named = path.strip_prefix(root).unwrap().display().to_string();

      if kind.is_dir() {
        walk(root, &path, into);
      } else if kind.is_symlink() {
        into.insert(named, std::fs::read_link(&path).unwrap().into_os_string().into_encoded_bytes());
      } else {
        into.insert(named, std::fs::read(&path).unwrap());
      }
    }
  }

  let mut found = BTreeMap::new();

  walk(root, root, &mut found);
  found
}

// A profiles' folder holding one profile with a little of everything a used profile holds.
fn profiles_with_owner() -> (tempfile::TempDir, PathBuf) {
  let profiles = tempfile::tempdir().unwrap();
  let owner = owner_in(profiles.path());

  (profiles, owner)
}

// A home folder whose Firefox keeps its profiles in `.mozilla/firefox`, holding the owner's profile as
// `profiles_with_owner` does, and its cache root `.cache/mozilla/firefox` the owner's profile's own cache: the
// home, the profiles' folder, the owner's profile and the cache root.
fn home_with_owner() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
  let home = tempfile::tempdir().unwrap();
  let profiles = home.path().join(".mozilla/firefox");
  let cache = home.path().join(".cache/mozilla/firefox");

  std::fs::create_dir_all(&profiles).unwrap();
  std::fs::create_dir_all(cache.join("abcd1234.default/startupCache")).unwrap();
  std::fs::write(cache.join("abcd1234.default/startupCache/startupCache.8.little"), "his").unwrap();

  let owner = owner_in(&profiles);

  (home, profiles, owner, cache)
}

// The owner's profile, with a little of everything a used profile holds, made in `profiles` with the
// `profiles.ini` naming it.
fn owner_in(profiles: &Path) -> PathBuf {
  let owner = profiles.join("abcd1234.default");
  let files = [
    "cookies.sqlite",
    "cookies.sqlite-wal",
    "cookies.sqlite-shm",
    "prefs.js",
    "permissions.sqlite",
    "storage/default/https+++site.example/ls/data.sqlite",
    "user.js",
    "logins.json",
    "key4.db",
    "signedInUser.json",
    "places.sqlite",
    "places.sqlite-wal",
    "formhistory.sqlite",
    "sessionstore.jsonlz4",
    "sessionstore-backups/recovery.jsonlz4",
    ".parentlock",
  ];

  for file in files {
    let path = owner.join(file);

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("bytes of {file}")).unwrap();
  }

  std::os::unix::fs::symlink("127.0.0.1:+4242", owner.join("lock")).unwrap();
  std::fs::write(
    profiles.join("profiles.ini"),
    "[Profile0]\nName=default\nIsRelative=1\nPath=abcd1234.default\nDefault=1\n\n[General]\nStartWithLastProfile=1\n",
  )
  .unwrap();

  owner
}

#[test]
fn the_default_profile_is_the_one_firefox_opens() {
  let root = tempfile::tempdir().unwrap();
  let listed = |text: &str| {
    std::fs::write(root.path().join("profiles.ini"), text).unwrap();
    default_profile(root.path())
  };

  // The installs' default wins over a profile marked default.
  assert_eq!(
    listed(
      "[Install4F96D1932A9F858E]\nDefault=b.default-release\n\n[Profile1]\nIsRelative=1\nPath=a.default\nDefault=1\n\n[Profile0]\nIsRelative=1\nPath=b.default-release\n"
    ),
    Ok(root.path().join("b.default-release"))
  );
  // With no install, the profile marked default; a path the list does not mark relative stands as written.
  assert_eq!(
    listed(
      "[Profile0]\nIsRelative=1\nPath=a.default\n\n[Profile1]\nIsRelative=0\nPath=/srv/profiles/b\nDefault=1\n"
    ),
    Ok(PathBuf::from("/srv/profiles/b"))
  );
  // Two profiles, neither marked, name none.
  assert!(listed("[Profile0]\nPath=a\n\n[Profile1]\nPath=b\n").is_err());
}

#[tokio::test]
async fn a_copy_leaves_passwords_history_and_locks_out_and_the_owners_profile_as_it_stood() {
  let (profiles, owner) = profiles_with_owner();
  let before = contents(&owner);
  let copy = make(&owner, profiles.path(), &NoProcesses).await.unwrap();
  let copied = contents(&copy);
  let names: Vec<&str> = copied.keys().map(String::as_str).collect();

  assert_eq!(
    names,
    [
      "bmo-owner.pid",
      "cookies.sqlite",
      "cookies.sqlite-wal",
      "permissions.sqlite",
      "prefs.js",
      "storage/default/https+++site.example/ls/data.sqlite"
    ]
  );
  assert_eq!(copied["cookies.sqlite"], before["cookies.sqlite"]);
  assert_eq!(copied[OWNER_FILE], format!("{}\n", std::process::id()).into_bytes());
  assert!(copy.parent() == Some(profiles.path()) && is_copy(&copy, profiles.path(), &owner));
  assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o700);
  assert_eq!(contents(&owner), before);
  assert!(std::fs::symlink_metadata(owner.join("lock")).is_ok());
}

#[tokio::test]
async fn removal_and_reclaim_never_reach_the_owners_profile() {
  let (profiles, owner) = profiles_with_owner();
  let before = contents(&owner);
  let at = |name: &str| profiles.path().join(name);

  // Only a folder named as a copy, standing beside his profile, is a copy; his profile, a folder inside it and
  // the folder holding it are not, whatever their names.
  assert!(is_copy(&at(".bmo-web-firefox-1"), profiles.path(), &owner));
  assert!(!is_copy(&owner, profiles.path(), &owner));
  assert!(!is_copy(&owner.join(".bmo-web-firefox-1"), profiles.path(), &owner));
  assert!(!is_copy(profiles.path(), profiles.path(), &owner));
  assert!(!is_copy(&at("other"), profiles.path(), &owner));
  assert!(!is_copy(&at(".bmo-web-firefox-1"), profiles.path(), &at(".bmo-web-firefox-1/inner")));

  assert!(!remove(&owner, profiles.path(), &owner, &NoProcesses).await);
  assert_eq!(contents(&owner), before);

  // A copy whose owning process is gone is reclaimed; his profile, with no owner file at all and as old as
  // any, is never looked at.
  let left = at(".bmo-web-firefox-left");

  std::fs::create_dir(&left).unwrap();
  std::fs::write(left.join(OWNER_FILE), "1\n").unwrap();
  reap_stale(profiles.path(), &owner, u64::MAX, &NoProcesses).await;

  assert!(!left.exists());
  assert_eq!(contents(&owner), before);
}

#[test]
fn firefox_mirrors_a_profile_of_its_profiles_root_into_the_cache_root_beside_it() {
  let snap = Path::new("/home/someone/snap/firefox/common");

  assert_eq!(cache_root(&snap.join(".mozilla/firefox")), Some(snap.join(".cache/mozilla/firefox")));
  assert_eq!(cache_root(Path::new("/tmp/profiles")), None);
  assert_eq!(
    mirror_of(
      &snap.join(".mozilla/firefox/.bmo-web-firefox-1"),
      &snap.join(".mozilla/firefox"),
      &snap.join(".mozilla/firefox/abcd1234.default")
    ),
    Some(snap.join(".cache/mozilla/firefox/.bmo-web-firefox-1"))
  );
  assert_eq!(
    mirror_of(
      &snap.join(".mozilla/firefox/abcd1234.default"),
      &snap.join(".mozilla/firefox"),
      &snap.join(".mozilla/firefox/abcd1234.default")
    ),
    None,
    "the owner's profile is no copy, so its cache is no copy's mirror"
  );
}

#[tokio::test]
async fn a_removed_copy_takes_its_mirror_in_the_cache_root_with_it_and_leaves_the_owners_cache() {
  let (_home, profiles, owner, cache) = home_with_owner();
  let owners_cache = contents(&cache.join("abcd1234.default"));
  let copy = make(&owner, &profiles, &NoProcesses).await.unwrap();
  let mirror = cache.join(copy.file_name().unwrap());

  std::fs::create_dir_all(mirror.join("startupCache")).unwrap();
  std::fs::write(mirror.join("startupCache/startupCache.8.little"), "the copy's").unwrap();

  assert!(remove(&copy, &profiles, &owner, &NoProcesses).await);
  assert!(!copy.exists());
  assert!(!mirror.exists(), "the copy's mirror went with it");
  assert_eq!(contents(&cache.join("abcd1234.default")), owners_cache);
}

#[tokio::test]
async fn reclaim_takes_a_killed_runs_mirror_and_an_hour_old_orphan_mirror_and_nothing_else() {
  let (_home, profiles, owner, cache) = home_with_owner();
  let owners_cache = contents(&cache.join("abcd1234.default"));
  let killed = profiles.join(".bmo-web-firefox-killed");
  let orphan = cache.join(".bmo-web-firefox-orphan");
  let other = cache.join("other.default");

  std::fs::create_dir(&killed).unwrap();
  std::fs::write(killed.join(OWNER_FILE), "1\n").unwrap();

  for mirror in [&cache.join(".bmo-web-firefox-killed"), &orphan, &other] {
    std::fs::create_dir_all(mirror.join("startupCache")).unwrap();
  }

  // Just made, the orphan is not yet past the hour; the killed run's copy and its mirror go at once.
  let now_ms =
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;

  reap_stale(&profiles, &owner, now_ms, &NoProcesses).await;

  assert!(!killed.exists());
  assert!(!cache.join(".bmo-web-firefox-killed").exists());
  assert!(orphan.exists());

  reap_stale(&profiles, &owner, u64::MAX, &NoProcesses).await;

  assert!(!orphan.exists(), "an orphan mirror past the hour is reclaimed");
  assert!(other.exists(), "a folder of another name is never looked at");
  assert_eq!(contents(&cache.join("abcd1234.default")), owners_cache);
  assert!(owner.exists());
}
