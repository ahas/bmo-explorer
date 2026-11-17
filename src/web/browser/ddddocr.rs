//! The reading of a slider's gap, taken from the two slider algorithms of ddddocr (sml2h3/ddddocr, MIT
//! licence, copyright notice kept below) and re-expressed as plain Rust over the `image` crate this
//! repository already stands on: no Python, no ONNX runtime, no service — what is borrowed is the algorithm
//! the library publishes, and the models it also ships are not needed for it. The names of the two functions
//! keep the library's own, so a reader can set them beside its source.
//!
//! Permission notice for the ported algorithms, as the MIT licence asks: Copyright (c) 2022 sml2h3,
//! permission is granted free of charge to deal in the Software without restriction — the full text stands at
//! https://github.com/sml2h3/ddddocr/blob/master/LICENSE.

use image::GenericImageView;

/// The gap's centre x, in the background's own pixels, by the library's image-difference reading: both
/// pictures to grey, the piece stretched over the background's frame, and the column whose summed difference
/// from the background is the largest is where the hole in the picture stands.
///
/// This is the road for the shadow-gap sliders — those where the piece is only a darker shape cut into the
/// background — and nothing else; where the background holds no hole to find, no x is answered.
pub fn slide_comparison(piece: &[u8], background: &[u8]) -> Option<f64> {
  let piece = image::load_from_memory(piece).ok()?.to_luma8();
  let board = image::load_from_memory(background).ok()?;
  let (w, h) = board.dimensions();

  if w < 16 || h < 16 {
    return None;
  }

  let piece = image::imageops::resize(
    &image::DynamicImage::ImageLuma8(piece).into_luma8(),
    w,
    h,
    image::imageops::FilterType::Nearest,
  );
  let mut best_x = 0_u32;
  let mut best_sum = 0_i64;
  let mut sums = Vec::with_capacity(w as usize);

  for x in 0..w {
    let mut sum = 0_i64;

    for y in 0..h {
      sum += (board.get_pixel(x, y)[0] as i64 - piece.get_pixel(x, y)[0] as i64).abs();
    }

    if sum > best_sum {
      best_sum = sum;
      best_x = x;
    }
    sums.push(sum);
  }

  // The hole stands only where one column stands out by a margin both relative and absolute: the same grey
  // along the whole board answers no gap, and neither does a column one shade off its neighbours.
  let mean = sums.iter().sum::<i64>() as f64 / sums.len() as f64;
  let floor = h as i64 * 12;

  if best_sum < floor || (best_sum as f64) < mean * 1.5 {
    return None;
  }

  Some(best_x as f64)
}

/// The gap's left x and its score, by the library's edge-matching reading: the normalised cross-correlation
/// of the piece — its transparent surround left out of both sums — walked over the background. The best
/// position is answered when it stands clearly over the rest; a piece that fits nowhere answers nothing.
pub fn slide_match(piece: &[u8], background: &[u8]) -> Option<(f64, f64)> {
  let piece = image::load_from_memory(piece).ok()?.into_rgba8();
  let board = image::load_from_memory(background).ok()?.to_luma8();
  let (tw, th) = piece.dimensions();
  let (w, h) = board.dimensions();

  if tw < 8 || th < 8 || tw >= w || th >= h {
    return None;
  }

  let mask: Vec<bool> = piece.pixels().map(|one| one[3] > 128).collect();
  let pixels: Vec<f64> = piece
    .pixels()
    .map(|one| 0.299 * one[0] as f64 + 0.587 * one[1] as f64 + 0.114 * one[2] as f64)
    .collect();
  let count = mask.iter().filter(|keep| **keep).count();

  if count < 40 {
    return None;
  }

  let mean_t =
    pixels.iter().zip(&mask).filter(|(_, keep)| **keep).map(|(one, _)| one).sum::<f64>() / count as f64;
  let var_t = pixels
    .iter()
    .zip(&mask)
    .filter(|(_, keep)| **keep)
    .map(|(one, _)| (one - mean_t) * (one - mean_t))
    .sum::<f64>()
    .sqrt();

  if var_t < 1.0 {
    return None; // a flat piece carries no shape to match
  }

  let mut best = (0.0_f64, 0_u32);
  let step = 2_u32;

  for x in (0..=(w - tw)).step_by(step as usize) {
    for y in (0..=(h - th)).step_by(step as usize) {
      let mut sum = 0.0_f64;

      for py in 0..th {
        for px in 0..tw {
          let at = (py * tw + px) as usize;

          if mask[at] {
            sum += (board.get_pixel(x + px, y + py)[0] as f64 - mean_t) * (pixels[at] - mean_t);
          }
        }
      }

      let score = sum / (var_t * count as f64).max(1.0);

      if score > best.0 {
        best = (score, x);
      }
    }
  }

  (best.0 > 0.55).then_some((best.1 as f64, best.0))
}
