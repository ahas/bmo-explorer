//! The session record a call is made under, issued to the tool host with its issuer token and handed to every
//! call without the token (the tool host's docs/api.md, 「The session record」).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The variable naming the data root the record's tools read.
pub const DATA_ROOT_VARIABLE: &str = "BMO_DATA_DIR";

/// One session's record. Its field names are camel case on the wire. A field the record does not know is
/// refused rather than dropped, so an issuer that hands something new is told rather than ignored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
  /// Who holds the session, a name a tool may read.
  pub holder_name: String,
  /// The namespaces the session may use.
  pub namespaces: BTreeSet<String>,
  /// What the session may be shown, by the names the manifests declare, as the issuer resolved them.
  pub capabilities: BTreeSet<String>,
  /// The resources the session may act on.
  pub grants: Vec<Grant>,
  /// The full names of the entries its namespaces list by name; every other entry the session holds stands in
  /// its toolbook.
  #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
  pub shown: BTreeSet<String>,
  /// The variables handed to every call's environment.
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub variables: BTreeMap<String, String>,
  /// A file of variables the host reads afresh at every call and lays over `variables`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub variables_file: Option<PathBuf>,
  /// The issuer's own metadata, which the host never reads.
  #[serde(default, skip_serializing_if = "Map::is_empty")]
  pub labels: Map<String, Value>,
  /// When the session lapses, in epoch milliseconds; none where it stands until revoked.
  #[serde(default)]
  pub expires_at: Option<u64>,
  /// When it was issued, in epoch milliseconds.
  pub created_at: u64,
  /// What the session's tools keep for it while its record stands, ended when the record goes; never written
  /// down.
  #[serde(skip)]
  pub kept: crate::tool::kept::Keeping,
}

/// One resource a session may act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
  pub resource: String,
}

impl Session {
  /// Whether the session holds every capability of these names.
  pub fn holds<S: AsRef<str>>(&self, wanted: &[S]) -> bool {
    wanted.iter().all(|one| self.capabilities.contains(one.as_ref()))
  }

  /// One variable of the call's environment: the record's own, or the process's where the record names none,
  /// which is where the host's passed variables and the record's variables file reach a call.
  pub fn variable(&self, name: &str) -> Option<String> {
    self.variables.get(name).cloned().or_else(|| std::env::var(name).ok())
  }

  /// The data root the record's tools read, or why the call was handed none.
  pub fn data_root(&self) -> Result<PathBuf, String> {
    self
      .variable(DATA_ROOT_VARIABLE)
      .filter(|root| !root.is_empty())
      .map(PathBuf::from)
      .ok_or_else(|| format!("the session names no data root: {DATA_ROOT_VARIABLE} is not set"))
  }

  /// The issuer's label of this name, where the record carries one.
  pub fn label(&self, name: &str) -> Option<&Value> {
    self.labels.get(name)
  }
}
