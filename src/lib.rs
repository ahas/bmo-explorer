//! The web tools a tool host serves, the browse and remote-browser runs, and the unit command that starts them
//! (docs/api.md).

pub mod browse;
pub mod clients;
pub mod forms;
pub mod remote_browser;
pub mod settings;
pub mod site;
pub mod store;
pub mod tool;
pub mod tools;
pub mod web;
pub mod web_helper;

/// The repository's version, which every contract it serves carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
