//! How every Chrome the web layer starts stands apart from a person's session (docs/web-access.md, 「How the browser
//! is started」), as `detached.toml` beside this module holds it.

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detached {
  /// The arguments every Chrome it starts carries.
  pub detached_arguments: Vec<String>,
  /// The variables of a person's graphical session, none of which reaches it.
  pub session_variables: Vec<String>,
}

/// The settings as they are built into this repository.
pub static DETACHED: LazyLock<Detached> = LazyLock::new(|| {
  toml::from_str(include_str!("detached.toml")).expect("detached.toml reads as the settings")
});
