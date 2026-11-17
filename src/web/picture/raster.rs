//! A raster picture read and redrawn to be looked at: the type its own first bytes say it is, and the picture
//! upright, laid on white and fitted inside an edge, or cut to a card's square, as JPEG. The `image` crate reads every format but AVIF; an
//! AVIF picture is read by `avif`, since no maintained crate decodes one to pixels without a system library.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};

use super::avif;

/// Why bytes are no picture this module reads.
#[derive(Debug)]
pub struct NotAPicture(pub String);

impl std::fmt::Display for NotAPicture {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.0)
  }
}

fn failed(why: impl std::fmt::Display) -> NotAPicture {
  NotAPicture(why.to_string())
}

/// The white a picture is laid on wherever its transparency would be lost.
const GROUND: u8 = 255;

/// The type a picture's own first bytes say it is, of those a page serves.
pub fn raster_type(bytes: &[u8]) -> Option<&'static str> {
  let starts = |values: &[u8]| bytes.starts_with(values);
  let text = |offset: usize, value: &str| bytes.get(offset..offset + value.len()) == Some(value.as_bytes());

  if starts(&[0xff, 0xd8, 0xff]) {
    Some("image/jpeg")
  } else if starts(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
    Some("image/png")
  } else if text(0, "GIF87a") || text(0, "GIF89a") {
    Some("image/gif")
  } else if text(0, "RIFF") && text(8, "WEBP") {
    Some("image/webp")
  } else if text(4, "ftyp") && (text(8, "avif") || text(8, "avis")) {
    Some("image/avif")
  } else if starts(&[0x00, 0x00, 0x01, 0x00]) && bytes.len() > 6 {
    Some("image/x-icon")
  } else {
    None
  }
}

// The picture bytes hold, turned to stand upright where they ask to be.
fn decode(bytes: &[u8]) -> Result<DynamicImage, NotAPicture> {
  match image::guess_format(bytes) {
    Ok(
      format @ (ImageFormat::Jpeg
      | ImageFormat::Png
      | ImageFormat::Gif
      | ImageFormat::WebP
      | ImageFormat::Tiff),
    ) => {
      let mut decoder =
        ImageReader::with_format(Cursor::new(bytes), format).into_decoder().map_err(failed)?;
      let orientation = decoder.orientation().map_err(failed)?;
      let mut picture = DynamicImage::from_decoder(decoder).map_err(failed)?;

      picture.apply_orientation(orientation);
      Ok(picture)
    }
    _ if bytes.get(4..8) == Some(b"ftyp") => avif::decode(bytes),
    _ => Err(failed("unsupported image format")),
  }
}

// The picture laid on white: each pixel's colour weighed by its own alpha against the ground, so an
// anti-aliased edge keeps its softness against the white.
fn on_white(picture: &DynamicImage) -> RgbImage {
  let rgba = picture.to_rgba8();

  RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
    let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
    let laid = |channel: u8| {
      ((u32::from(channel) * u32::from(alpha) + u32::from(GROUND) * (255 - u32::from(alpha)) + 127) / 255)
        as u8
    };

    image::Rgb([laid(red), laid(green), laid(blue)])
  })
}

fn jpeg(picture: &RgbImage, quality: u8) -> Result<Vec<u8>, NotAPicture> {
  let mut out = Vec::new();

  JpegEncoder::new_with_quality(&mut out, quality).encode_image(picture).map_err(failed)?;

  Ok(out)
}

/// The size a picture is brought inside a square of `limit` pixels at, never larger than it stands.
pub fn inside(width: u32, height: u32, limit: u32) -> (u32, u32) {
  if width <= limit && height <= limit {
    return (width, height);
  }

  let scale = f64::from(limit) / f64::from(width.max(height));
  let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, limit);

  (scaled(width), scaled(height))
}

/// A picture redrawn to be looked at: its JPEG bytes, the size it is shown at and the size it came in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fitted {
  pub jpeg: Vec<u8>,
  pub width: u32,
  pub height: u32,
  pub source_width: u32,
  pub source_height: u32,
}

/// The picture upright, fitted inside a square of `edge` pixels and never enlarged, laid on white and written
/// as JPEG at a quality on the encoder's scale of 1 to 100.
pub fn fitted(bytes: &[u8], edge: u32, quality: u8) -> Result<Fitted, NotAPicture> {
  redrawn(bytes, quality, |width, height| {
    Framing::whole(width, height, inside(width, height, edge))
  })
}

/// The size a picture is brought to `limit` pixels wide at, its sides in proportion, never larger than it stands.
pub fn narrowed(width: u32, height: u32, limit: u32) -> (u32, u32) {
  if width <= limit {
    return (width, height);
  }

  let scaled = ((f64::from(height) * f64::from(limit) / f64::from(width)).round() as u32).max(1);

  (limit, scaled)
}

/// The picture upright, brought to at most `width` pixels wide and never enlarged, laid on white and written as
/// JPEG at a quality on the encoder's scale of 1 to 100.
pub fn fitted_to_width(bytes: &[u8], width: u32, quality: u8) -> Result<Fitted, NotAPicture> {
  redrawn(bytes, quality, |source_width, source_height| {
    Framing::whole(source_width, source_height, narrowed(source_width, source_height, width))
  })
}

/// The part of a picture a redrawing keeps and the size it is brought to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Framing {
  /// The kept part's left and top edges and its width and height, in the picture's own pixels.
  pub cut: (u32, u32, u32, u32),
  /// The width and height the kept part is brought to.
  pub size: (u32, u32),
}

impl Framing {
  fn whole(width: u32, height: u32, size: (u32, u32)) -> Self {
    Self {
      cut: (0, 0, width, height),
      size,
    }
  }
}

/// The square cut from a picture's middle on its short side, brought to at most `limit` pixels on a side and
/// never larger than it stands.
pub fn squared(width: u32, height: u32, limit: u32) -> Framing {
  let side = width.min(height);
  let brought = side.min(limit);

  Framing {
    cut: ((width - side) / 2, (height - side) / 2, side, side),
    size: (brought, brought),
  }
}

/// The picture upright, laid on white, cut to a square from its middle and brought to at most `edge` pixels on a
/// side, never enlarged, written as JPEG at a quality on the encoder's scale of 1 to 100: a card's picture.
pub fn card(bytes: &[u8], edge: u32, quality: u8) -> Result<Fitted, NotAPicture> {
  redrawn(bytes, quality, |width, height| squared(width, height, edge))
}

fn redrawn(
  bytes: &[u8],
  quality: u8,
  framing: impl Fn(u32, u32) -> Framing,
) -> Result<Fitted, NotAPicture> {
  let picture = decode(bytes)?;
  let (source_width, source_height) = (picture.width(), picture.height());
  let laid = on_white(&picture);
  let Framing {
    cut: (left, top, cut_width, cut_height),
    size: (width, height),
  } = framing(source_width, source_height);
  let kept = if (left, top, cut_width, cut_height) == (0, 0, source_width, source_height) {
    laid
  } else {
    image::imageops::crop_imm(&laid, left, top, cut_width, cut_height).to_image()
  };
  let brought = if (width, height) == kept.dimensions() {
    kept
  } else {
    image::imageops::resize(&kept, width, height, FilterType::Lanczos3)
  };

  Ok(Fitted {
    jpeg: jpeg(&brought, quality)?,
    width,
    height,
    source_width,
    source_height,
  })
}
