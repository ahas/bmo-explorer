//! How long a page a fetch returns may be, however the fetch was asked: the ceiling an asked length is
//! clamped to, read the same way by the web reading and by a site tool slicing what the helper gave it; and
//! what a PDF with no text layer is read as.

/// The ceiling a fetch's asked length is clamped to.
pub const MAX_FETCH_CHARS: usize = 100_000;

/// What a PDF whose pages hold no text layer reads as.
pub const NO_PDF_TEXT: &str =
  "NO TEXT: this PDF holds no text layer; its pages are pictures, such as a scan.";
