//! The web tools a tool host serves, the browse and remote-browser runs, and the unit command that starts them
//! (docs/api.md).

/// The repository's version, which every contract it serves carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
