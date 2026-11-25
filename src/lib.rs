//! Web reading as an upstream (docs/api.md): the client of the six operations and the MCP server over it, the
//! web tools a tool host places, the shared web helper and the site contract.

pub mod client;
pub mod forms;
pub mod import;
pub mod mcp;
pub mod openapi;
pub mod site;
pub mod store;
pub mod tool;
pub mod tools;
pub mod web;
pub mod web_helper;

/// The version the MCP server and the command name: the set's version the build was made at, which the
/// testbed hands the build as `BMO_VERSION`, else the manifest's placeholder (bmo-chat's
/// `docs/conventions.md`, 「Versions」).
pub const VERSION: &str = match option_env!("BMO_VERSION") {
  Some(version) => version,
  None => env!("CARGO_PKG_VERSION"),
};
