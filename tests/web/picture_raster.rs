//! A raster picture read and redrawn to be looked at.

use std::io::Cursor;

use bmo_explorer::web::picture::raster::{card, fitted, inside, raster_type};
use image::{DynamicImage, ImageFormat, RgbImage};

fn png(picture: &DynamicImage) -> Vec<u8> {
  let mut out = Cursor::new(Vec::new());

  picture.write_to(&mut out, ImageFormat::Png).unwrap();
  out.into_inner()
}

#[test]
fn a_picture_is_known_by_its_first_bytes() {
  assert_eq!(raster_type(&[0xff, 0xd8, 0xff, 0]), Some("image/jpeg"));
  assert_eq!(raster_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
  assert_eq!(raster_type(b"\0\0\0\x1cftypavif"), Some("image/avif"));
  assert_eq!(raster_type(b"hello"), None);
}

#[test]
fn a_picture_is_fitted_inside_its_edge_never_enlarged_and_laid_on_white() {
  let wide = DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 100, image::Rgb([200, 10, 10])));
  let shown = fitted(&png(&wide), 150, 85).unwrap();
  let read = image::load_from_memory(&shown.jpeg).unwrap();

  assert_eq!(
    (shown.width, shown.height, shown.source_width, shown.source_height),
    (150, 50, 300, 100)
  );
  assert_eq!((read.width(), read.height()), (150, 50));

  let clear = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 2, image::Rgba([0, 0, 0, 0])));
  let small = fitted(&png(&clear), 150, 85).unwrap();
  let read = image::load_from_memory(&small.jpeg).unwrap().to_rgb8();

  assert_eq!((small.width, small.height), (4, 2));
  assert!(read.pixels().all(|pixel| pixel.0.iter().all(|channel| *channel > 240)));
  assert!(fitted(b"no picture", 150, 85).is_err());
  assert_eq!(inside(3000, 2000, 1568), (1568, 1045));
}

#[test]
fn a_card_s_picture_is_the_square_cut_from_its_middle_brought_to_the_edge_and_never_enlarged() {
  // A wide picture whose middle third is green between red sides: the square cut from its middle is all green.
  let banded = DynamicImage::ImageRgb8(RgbImage::from_fn(600, 200, |x, _| {
    if (200..400).contains(&x) {
      image::Rgb([10, 200, 10])
    } else {
      image::Rgb([200, 10, 10])
    }
  }));
  let shown = card(&png(&banded), 100, 85).unwrap();
  let read = image::load_from_memory(&shown.jpeg).unwrap().to_rgb8();

  assert_eq!(
    (shown.width, shown.height, shown.source_width, shown.source_height),
    (100, 100, 600, 200)
  );
  assert_eq!(read.dimensions(), (100, 100));
  assert!(
    read.pixels().all(|pixel| pixel.0[1] > 150 && pixel.0[0] < 90),
    "only the green middle is kept"
  );

  let tall_and_small = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 90, image::Rgb([10, 10, 200])));
  let small = card(&png(&tall_and_small), 100, 85).unwrap();

  assert_eq!((small.width, small.height), (40, 40));
}
