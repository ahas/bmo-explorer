//! A picture on the web, handed to a research run as a picture it sees (docs/site-contract.md, 「The web
//! helper's socket」, `picture`). It is fetched by the same checked direct road the keeping of a search's pictures takes, so a
//! picture a run can see here is one the runner can keep once the run ends.

mod avif;
pub mod raster;

use crate::web::address::Check;
use crate::web::client::{Direct, Transport};
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::jar::Jar;
use crate::web::pacing::Pacing;

/// The most bytes a picture is read to.
pub const IMAGE_BYTE_CAP: usize = 5 * 1024 * 1024;

/// Between two requests to one host.
pub const PICTURE_HOST_INTERVAL_MS: u64 = 250;

/// The long side a picture is shown at: the most the model reads without scaling the picture down again
/// itself, and enough to read the prices on a photographed menu board.
pub const VIEWED_PICTURE_EDGE: u32 = 1568;
const VIEWED_PICTURE_QUALITY: u8 = 85;

/// A picture's bytes and the type its own first bytes say it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterImage {
  pub kind: &'static str,
  pub bytes: Vec<u8>,
}

/// The picture at an address, by the checked direct road: an answer that is not a success, a body past the
/// bound, or bytes that are no raster picture each fail it.
pub async fn fetch_raster(
  url: &str,
  store: &dyn Pacing,
  check: &Check,
  transport: &dyn Transport,
) -> WebResult<RasterImage> {
  let jar = tokio::sync::Mutex::new(Jar::default());
  let mut direct = Direct::new(store, &jar, check, transport);

  direct.interval_ms = PICTURE_HOST_INTERVAL_MS;
  direct.byte_cap = IMAGE_BYTE_CAP;

  let read = direct.bytes(url).await?;

  if !(200..300).contains(&read.status) {
    return Err(
      WebError::new(format!("HTTP {} for {url}", read.status), Failure::Http)
        .at(Some(read.status), Some(url)),
    );
  }

  if read.truncated {
    return Err(
      WebError::new(format!("Image larger than {IMAGE_BYTE_CAP} bytes: {url}"), Failure::TooLarge)
        .with_url(url),
    );
  }

  let kind = raster::raster_type(&read.bytes)
    .ok_or_else(|| WebError::new(format!("Not a raster image: {url}"), Failure::Blocked).with_url(url))?;

  Ok(RasterImage {
    kind,
    bytes: read.bytes,
  })
}

/// A picture as the viewer shows it: JPEG, its long side at most the viewer's edge, with the size it came in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewedPicture {
  pub kind: &'static str,
  pub jpeg: Vec<u8>,
  pub width: u32,
  pub height: u32,
  pub source_width: u32,
  pub source_height: u32,
}

/// The picture redrawn to be looked at: upright, fitted inside the edge and never enlarged, laid on white.
pub fn view(image: &RasterImage) -> WebResult<ViewedPicture> {
  let fitted = raster::fitted(&image.bytes, VIEWED_PICTURE_EDGE, VIEWED_PICTURE_QUALITY)
    .map_err(|why| WebError::new(format!("The picture could not be read: {why}"), Failure::Blocked))?;

  Ok(ViewedPicture {
    kind: "image/jpeg",
    jpeg: fitted.jpeg,
    width: fitted.width,
    height: fitted.height,
    source_width: fitted.source_width,
    source_height: fitted.source_height,
  })
}
