//! This repository's own store (docs/store.md): one SQLite database in its folder of the data root, which every
//! process of it opens, each call on a connection of its own. The tables are made as the store is opened, so a
//! store that does not stand yet is made by its first reader.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{Connection, Executor};

/// The folder of the data root this repository keeps its data in.
pub const FOLDER: &str = "explorer";

/// The database's file in that folder.
pub const FILE: &str = "explorer.db";

/// How long a connection waits for another process's write before it fails. The journal stays in SQLite's own
/// mode, where every lock a connection takes waits this long; changing the mode as a connection opens takes a
/// lock that does not wait, and fails two processes opening the store at once.
const BUSY_WAIT: Duration = Duration::from_secs(10);

/// Every table of the store, each made where it does not stand.
const TABLES: &str = "
CREATE TABLE IF NOT EXISTS web_engine_cookie (
  engine TEXT NOT NULL,
  host TEXT NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  value TEXT NOT NULL,
  secure INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  PRIMARY KEY (engine, host, path, name)
);
CREATE TABLE IF NOT EXISTS web_engine_health (
  engine TEXT NOT NULL PRIMARY KEY,
  state TEXT NOT NULL,
  reason TEXT,
  observed_at INTEGER,
  suspended_until INTEGER,
  elapsed_ms INTEGER,
  result_count INTEGER
);
CREATE TABLE IF NOT EXISTS web_engine_recipe (
  engine TEXT NOT NULL PRIMARY KEY,
  identity TEXT NOT NULL,
  interval_ms INTEGER NOT NULL,
  route TEXT NOT NULL,
  warm_up INTEGER NOT NULL,
  cause TEXT,
  evidence TEXT,
  set_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS web_host_pacing (
  key TEXT NOT NULL PRIMARY KEY,
  next_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
";

/// The tables `TABLES` makes; a table of the file that is not one of them is dropped as the store opens.
const KEPT: &[&str] = &["web_engine_cookie", "web_engine_health", "web_engine_recipe", "web_host_pacing"];

/// The store's file under a data root.
pub fn place(data_root: &Path) -> PathBuf {
  data_root.join(FOLDER).join(FILE)
}

/// A connection to the store at `place`, its folder and its tables made where they do not stand.
pub async fn open(place: &Path) -> Result<SqliteConnection, String> {
  if let Some(folder) = place.parent() {
    std::fs::create_dir_all(folder)
      .map_err(|why| format!("{} could not be made: {why}", folder.display()))?;
  }

  let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", place.display()))
    .map_err(|why| why.to_string())?
    .create_if_missing(true)
    .busy_timeout(BUSY_WAIT);
  let mut connection = SqliteConnection::connect_with(&options)
    .await
    .map_err(|why| format!("the store at {} could not be opened: {why}", place.display()))?;

  connection
    .execute(TABLES)
    .await
    .map_err(|why| format!("the store's tables could not be made: {why}"))?;

  let standing: Vec<String> =
    sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
      .fetch_all(&mut connection)
      .await
      .map_err(|why| format!("the store's tables could not be listed: {why}"))?;

  for table in standing.iter().filter(|table| !KEPT.contains(&table.as_str())) {
    connection
      .execute(format!("DROP TABLE IF EXISTS \"{}\"", table.replace('"', "\"\"")).as_str())
      .await
      .map_err(|why| format!("the store's table {table} could not be dropped: {why}"))?;
  }

  Ok(connection)
}
