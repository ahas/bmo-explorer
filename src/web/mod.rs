//! The web as the agents see it (docs/web-access.md): a search that returns what the search engines
//! returned, and a fetch that returns what the page contained, each through the checks every outward request
//! of the web layer goes through — the address policy and the pacing per host and per engine — and the real
//! Chrome a read falls back to. Neither passes through a model on the way, so what an agent reads is the
//! page's own words.

pub mod address;
pub mod chrome;
pub mod classify;
pub mod client;
pub mod clock;
pub mod error;
pub mod identity;
pub mod jar;
pub mod pacing;
pub mod pdf;
pub mod picture;
pub mod search;
pub mod session;

pub use error::{Failure, WebError, WebResult};
