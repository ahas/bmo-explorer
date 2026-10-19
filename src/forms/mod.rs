//! The forms a read of the web takes, shared by the web layer and the site contract: what a read failed
//! with, what the search engines answered with and how that answer is written, the ceiling a page is cut
//! to, what a fetched page is and the text a page is tidied to, and the time a read is stamped with
//! (docs/web-access.md, docs/site-contract.md). This module stands below both, so a road crosses the web
//! helper's socket in these forms unchanged (docs/site-contract.md, 「Where a tool lives」).

pub mod classify;
pub mod clock;
pub mod engine;
pub mod error;
pub mod fetch;
pub mod search;
pub mod slice;
pub mod text;

pub use engine::{EngineHealth, EngineState};
pub use error::{Failure, WebError, WebResult};
pub use fetch::MAX_FETCH_CHARS;
pub use search::{SearchHit, SearchResults};
pub use slice::{DEFAULT_FETCH_CHARS, FetchSlice, render_slice, slice_content};
pub use text::{tidy, tidy_blank_runs};
