use std::io::Cursor;
use std::sync::Arc;

use super::testing::{Web, answer, check};
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::picture::*;

fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
  let picture = if alpha {
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
      width,
      height,
      image::Rgba([200, 40, 40, 0]),
    ))
  } else {
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 40])))
  };
  let mut out = Vec::new();

  picture.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
  out
}

// Carried from packages/web/picture-view.test.ts: a picture larger than the model reads is shown at the edge, with the size it came in
#[test]
fn a_picture_larger_than_the_model_reads_is_shown_at_the_edge() {
  let viewed = view(&RasterImage {
    kind: "image/png",
    bytes: png(3136, 1568, false),
  })
  .unwrap();
  let shown = image::load_from_memory(&viewed.jpeg).unwrap();

  assert_eq!(viewed.kind, "image/jpeg");
  assert_eq!(image::guess_format(&viewed.jpeg).unwrap(), image::ImageFormat::Jpeg);
  assert_eq!((viewed.width, viewed.height), (VIEWED_PICTURE_EDGE, 784));
  assert_eq!((shown.width(), shown.height()), (VIEWED_PICTURE_EDGE, 784));
  assert_eq!((viewed.source_width, viewed.source_height), (3136, 1568));
}

// Carried from packages/web/picture-view.test.ts: a small picture is never enlarged, and a transparent one is laid on white
#[test]
fn a_small_picture_is_never_enlarged_and_a_transparent_one_is_laid_on_white() {
  let viewed = view(&RasterImage {
    kind: "image/png",
    bytes: png(40, 20, true),
  })
  .unwrap();
  let pixel = image::load_from_memory(&viewed.jpeg).unwrap().to_rgb8().get_pixel(0, 0).0;

  assert_eq!((viewed.width, viewed.height), (40, 20));
  assert!(pixel.iter().all(|channel| *channel > 240));
}

// Carried from packages/web/picture-view.test.ts: an address that does not answer with a picture is the fetch's own failure
#[tokio::test]
async fn an_address_that_does_not_answer_with_a_picture_is_the_fetchs_own_failure() {
  let web = Arc::new(
    Web::default()
      .route("example.com", |_| answer(200, &[("content-type", "text/html")], b"<p>a page</p>")),
  );
  let store = MemoryPacing::default();
  let failed = fetch_raster("https://example.com/page", &store, &check(StandingClock::at(1_000)), &*web)
    .await
    .unwrap_err();

  assert!(failed.message.contains("Not a raster image"));
}
