//! What a read of the web failed with, as every caller of this crate reads it: the kind of failure, the
//! status and the address where they are known, and whether the real browser was tried after the direct
//! read failed. The forms themselves stand in the web's forms crate, below this layer and the site
//! contract; they are said here so every caller of this layer names them as it always has.

pub use crate::forms::error::{Failure, WebError, WebResult};
