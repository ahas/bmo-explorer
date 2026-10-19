//! What the site tools stand on (docs/site-contract.md): the site contract a tool is written against and the reach
//! it is given, the client of the web helper's socket and the wrapper a placed tool is served by, the readings
//! more than one tool shares, and a page no road read as it is recorded. The placed tools and the linked tools'
//! crate serve the tools; the server reads the recorded pages by the roads named here.

pub mod contract;
pub mod helper;
pub mod placed;
pub mod readers;
pub mod unread;
