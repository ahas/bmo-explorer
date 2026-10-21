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

/// The repository's version, which every contract it serves carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
