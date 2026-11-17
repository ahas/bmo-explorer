//! The owner's own Firefox profile and the copy a browser runs on (docs/web-access.md, 「Which browser」). His
//! profile is found as his Firefox finds it and only ever read: each start copies it file by file into a new
//! folder of this layer's beside it, leaving out his saved passwords, his Sync sign-in, his history and his
//! saved session, and the browser runs on that copy alone. Only a folder this layer made, by its name, is ever
//! removed, and never his profile, a folder inside it or one holding it.

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use ini::Ini;

use super::profile::{self, ProcessControl};

/// The name every copy's folder begins with.
pub const PREFIX: &str = ".bmo-web-firefox-";

/// What of the owner's profile a copy never takes, by its name in the profile: the locks his Firefox holds, his
/// own user file, his saved passwords and their key store, the Firefox account his Sync signs in with, his saved
/// session, the copy's own endpoint note, his crash and telemetry reports, and caches Firefox makes again.
const LEFT_OUT: [&str; 25] = [
  "lock",
  ".parentlock",
  "parent.lock",
  "user.js",
  "logins.json",
  "logins-backup.json",
  "key4.db",
  "key3.db",
  "signedInUser.json",
  "sessionstore.jsonlz4",
  "sessionstore-backups",
  "sessionCheckpoints.json",
  "bookmarkbackups",
  "WebDriverBiDiServer.json",
  ".startup-incomplete",
  "crashes",
  "minidumps",
  "saved-telemetry-pings",
  "datareporting",
  "cache2",
  "startupCache",
  "shader-cache",
  "thumbnails",
  "OfflineCache",
  "Crash Reports",
];

/// The databases a copy never takes, with their journals: his history and bookmarks, their icons, and what he
/// typed into forms.
const DATABASES_LEFT_OUT: [&str; 3] = ["places.sqlite", "favicons.sqlite", "formhistory.sqlite"];

/// Whether a file or folder of the owner's profile, by its name, stays out of a copy. A database's shared-memory
/// index stays out wherever it stands, since SQLite builds it again from the database and its journal.
pub fn left_out(name: &str) -> bool {
  LEFT_OUT.contains(&name)
    || DATABASES_LEFT_OUT.iter().any(|database| name.starts_with(database))
    || name.ends_with("-shm")
}

/// The profile Firefox opens by default, as the `profiles.ini` in `root` names it: the default profile its
/// installs agree on, else the profile marked default, else the only profile. A path the list marks relative,
/// or an install's default no profile lists, stands under `root`.
pub fn default_profile(root: &Path) -> Result<PathBuf, String> {
  let listed = root.join("profiles.ini");
  let read =
    Ini::load_from_file(&listed).map_err(|why| format!("{} could not be read: {why}", listed.display()))?;
  let profiles: Vec<(&str, bool, bool)> = sections(&read, "Profile")
    .filter_map(|fields| {
      Some((
        fields.get("Path")?,
        fields.get("IsRelative") != Some("0"),
        fields.get("Default") == Some("1"),
      ))
    })
    .collect();
  let mut installs: Vec<&str> =
    sections(&read, "Install").filter_map(|fields| fields.get("Default")).collect();

  installs.sort_unstable();
  installs.dedup();

  let chosen = match installs.as_slice() {
    [only] => Some(
      profiles
        .iter()
        .find(|(path, ..)| path == only)
        .map_or((*only, !Path::new(only).is_absolute()), |(path, relative, _)| (*path, *relative)),
    ),
    _ => profiles
      .iter()
      .find(|(.., marked)| *marked)
      .or(if profiles.len() == 1 { profiles.first() } else { None })
      .map(|(path, relative, _)| (*path, *relative)),
  };

  match chosen {
    Some((path, true)) => Ok(root.join(path)),
    Some((path, false)) => Ok(PathBuf::from(path)),
    None => Err(format!("{} names no default profile", listed.display())),
  }
}

/// The fields of each section of the list whose name begins with `prefix`.
fn sections<'a>(read: &'a Ini, prefix: &'a str) -> impl Iterator<Item = &'a ini::Properties> {
  read
    .iter()
    .filter(move |(section, _)| section.is_some_and(|name| name.starts_with(prefix)))
    .map(|(_, fields)| fields)
}

/// Whether `path` is a copy's folder this layer may remove: a folder named with the copies' prefix, standing
/// directly in `profiles` beside the owner's profile, and neither the owner's profile, inside it nor holding it.
pub fn is_copy(path: &Path, profiles: &Path, owner: &Path) -> bool {
  let named = path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.starts_with(PREFIX));

  named && path.parent() == Some(profiles) && !path.starts_with(owner) && !owner.starts_with(path)
}

/// A copy of the owner's profile made in a new folder in `profiles`, private to this user, holding this
/// process's id as its owner; the folder, or why it could not be made. A folder whose copy failed part way is
/// removed again.
pub async fn make(owner: &Path, profiles: &Path, control: &dyn ProcessControl) -> Result<PathBuf, String> {
  let folder = profiles.join(format!("{PREFIX}{:016x}", rand::random::<u64>()));

  std::fs::DirBuilder::new()
    .mode(0o700)
    .create(&folder)
    .map_err(|why| format!("the copy's folder could not be made: {why}"))?;

  let written = std::fs::write(folder.join(profile::OWNER_FILE), format!("{}\n", std::process::id()))
    .map_err(|why| format!("the copy's owner could not be written: {why}"));
  // A profile is tens of megabytes, copied off the runtime's own threads.
  let filled = match written {
    Ok(()) => {
      let (from, into) = (owner.to_owned(), folder.clone());

      tokio::task::spawn_blocking(move || copy_folder(&from, &into, true))
        .await
        .unwrap_or_else(|why| Err(format!("the copy ended part way: {why}")))
    }
    Err(why) => Err(why),
  };

  match filled {
    Ok(()) => Ok(folder),
    Err(why) => {
      remove(&folder, profiles, owner, control).await;
      Err(why)
    }
  }
}

// Every file and folder of `from` copied into `into`, what a copy leaves out skipped at the profile's top; a
// link or a socket is never followed, and a file the owner's Firefox removed while it was copied is passed over.
fn copy_folder(from: &Path, into: &Path, top: bool) -> Result<(), String> {
  let entries =
    std::fs::read_dir(from).map_err(|why| format!("{} could not be read: {why}", from.display()))?;

  for entry in entries.flatten() {
    let name = entry.file_name();

    if top && name.to_str().is_some_and(left_out) {
      continue;
    }

    let Ok(kind) = entry.file_type() else {
      continue;
    };
    let target = into.join(&name);

    if kind.is_dir() {
      std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&target)
        .map_err(|why| format!("{} could not be made: {why}", target.display()))?;
      copy_folder(&entry.path(), &target, false)?;
    } else if kind.is_file() {
      copy_file(&entry.path(), &target)?;
    }
  }

  Ok(())
}

fn copy_file(from: &Path, into: &Path) -> Result<(), String> {
  let source = match std::fs::File::open(from) {
    Ok(source) => source,
    Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(why) => return Err(format!("{} could not be read: {why}", from.display())),
  };
  let mut target = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(into)
    .map_err(|why| format!("{} could not be made: {why}", into.display()))?;

  std::io::copy(&mut &source, &mut target)
    .map(|_| ())
    .map_err(|why| format!("{} could not be copied: {why}", from.display()))
}

/// A copy's folder removed with every process still opened on it, where `is_copy` takes it; whether it is gone.
/// Any other folder is left as it stands.
pub async fn remove(folder: &Path, profiles: &Path, owner: &Path, control: &dyn ProcessControl) -> bool {
  is_copy(folder, profiles, owner) && profile::remove(folder, control).await
}

/// Every copy in `profiles` whose owning process is gone, or that stands unowned past an hour, removed.
pub async fn reap_stale(profiles: &Path, owner: &Path, now_ms: u64, control: &dyn ProcessControl) {
  let ours = |path: &Path| is_copy(path, profiles, owner);

  profile::reap_stale_where(profiles, &ours, now_ms, control).await;
}
