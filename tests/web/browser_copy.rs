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
  let owner = profiles.path().join("abcd1234.default");
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
    profiles.path().join("profiles.ini"),
    "[Profile0]\nName=default\nIsRelative=1\nPath=abcd1234.default\nDefault=1\n\n[General]\nStartWithLastProfile=1\n",
  )
  .unwrap();

  (profiles, owner)
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
