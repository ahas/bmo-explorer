//! What a fetched page is, and the page's text as a reader sees it (docs/web-access.md, 「How a fetched page is
//! classified」). The reading stands in the web's forms crate, below this layer and the site contract, so a
//! placed site tool reads a page the way a fetch does without carrying this layer.

pub use crate::forms::classify::*;
