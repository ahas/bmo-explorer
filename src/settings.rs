//! The settings (docs/api.md, 「Settings and data」): the data root, this repository's folder in it, and the
//! address and token of each service it calls, read from `explorer/settings.toml` under the data root.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::store;

/// The variable naming the data root, as the tool host hands it to every call and bmo-agency to every run.
pub const DATA_ROOT_VARIABLE: &str = "BMO_DATA_DIR";

/// The settings' file in this repository's folder of the data root.
pub const FILE: &str = "settings.toml";

/// The address and bearer token of each service this repository calls; a service left unset is refused by
/// every call naming it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Units {
  pub agency: Option<String>,
  pub agency_token: Option<String>,
  pub receiver: Option<String>,
  pub receiver_token: Option<String>,
  pub artifact: Option<String>,
  pub artifact_token: Option<String>,
  pub gate: Option<String>,
  pub gate_token: Option<String>,
  pub node: Option<String>,
  pub node_token: Option<String>,
}

/// What the remote browser lands on where its steps name neither (docs/remote-browser.md, 「The steps」).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteBrowser {
  #[serde(default = "default_port")]
  pub port: i64,
  #[serde(default = "default_profile")]
  pub profile: String,
}

fn default_port() -> i64 {
  9222
}

fn default_profile() -> String {
  "remote-browser".to_owned()
}

impl Default for RemoteBrowser {
  fn default() -> Self {
    Self {
      port: default_port(),
      profile: default_profile(),
    }
  }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
  #[serde(default)]
  units: Units,
  #[serde(default)]
  remote_browser: RemoteBrowser,
}

/// The settings a process of this repository runs on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
  pub data_root: PathBuf,
  pub units: Units,
  pub remote_browser: RemoteBrowser,
}

impl Settings {
  /// The data root named, or the one `BMO_DATA_DIR` names; refused where neither names one.
  pub fn data_root(named: Option<PathBuf>) -> Result<PathBuf, String> {
    named
      .or_else(|| std::env::var_os(DATA_ROOT_VARIABLE).filter(|root| !root.is_empty()).map(PathBuf::from))
      .ok_or_else(|| format!("no data root is named: pass --data-root or set {DATA_ROOT_VARIABLE}"))
  }

  /// The settings under a data root: its file where one stands, nothing set where none does; a file that does
  /// not read is refused, naming it.
  pub fn read(data_root: &Path) -> Result<Self, String> {
    let path = Self::file(data_root);
    let file: File = match std::fs::read_to_string(&path) {
      Ok(text) => {
        toml::from_str(&text).map_err(|why| format!("{} does not read: {why}", path.display()))?
      }
      Err(why) if why.kind() == std::io::ErrorKind::NotFound => File::default(),
      Err(why) => return Err(format!("{} could not be read: {why}", path.display())),
    };

    Ok(Self {
      data_root: data_root.to_path_buf(),
      units: file.units,
      remote_browser: file.remote_browser,
    })
  }

  /// The settings' file under a data root.
  pub fn file(data_root: &Path) -> PathBuf {
    data_root.join(store::FOLDER).join(FILE)
  }
}
