//! The ported slider readings of ddddocr (docs/web-access.md, 「When the browser meets a challenge」), judged on
//! synthetic boards: the edge-matching road, and the refusals both roads owe a board that holds no hole. The
//! difference road's answer is checked where the loop uses it, in `browser_challenge`.

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage, imageops};

use bmo_explorer::web::browser::ddddocr::{slide_comparison, slide_match};

// A board of one flat tone with its hole cut as `hole`, and the piece as a 44 by 44 crop of the board at
// `piece_at` — over the hole's own edge, as a slider's piece is cut: a piece standing wholly inside one flat
// tone, or wholly inside one hole, carries no shape for either road to read.
fn board_and_piece(hole: Option<(u32, u32, u32, u32)>, piece_at: (u32, u32)) -> (Vec<u8>, Vec<u8>) {
  let board: RgbaImage = ImageBuffer::from_fn(360, 120, |x, y| {
    let cut = hole.is_some_and(|(left, top, width, height)| {
      (left..left + width).contains(&x) && (top..top + height).contains(&y)
    });

    Rgba(if cut { [30, 30, 30, 255] } else { [210, 200, 190, 255] })
  });
  let piece = match hole {
    Some(_) => imageops::crop_imm(&board, piece_at.0, piece_at.1, 44, 44).to_image(),
    None => ImageBuffer::from_fn(44, 44, |_, _| Rgba([210, 200, 190, 255])),
  };

  (png(&board), png(&piece))
}

fn png(picture: &RgbaImage) -> Vec<u8> {
  let mut out = Vec::new();

  DynamicImage::ImageRgba8(picture.clone())
    .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
    .unwrap();
  out
}

// The edge-matching road stands the piece on the place it was cut from, in the background's own pixels.
#[test]
fn the_edge_matching_read_stands_the_piece_on_its_hole() {
  let (board, piece) = board_and_piece(Some((300, 20, 44, 80)), (290, 38));
  let (x, score) = slide_match(&piece, &board).expect("a piece cut across the board's hole matches it");

  assert_eq!(x, 290.0, "the match is read at the piece's own left edge");
  assert!(score > 0.9, "and stands clearly over the rest of the board: {score}");
}

// A board with nothing cut in it answers no gap to either road, so a round is never pulled to a hole that is
// not there.
#[test]
fn a_board_holding_no_hole_answers_no_gap_to_either_read() {
  let (board, flat_piece) = board_and_piece(None, (290, 38));

  assert_eq!(slide_comparison(&flat_piece, &board), None, "a board of one tone holds no hole");
  assert_eq!(slide_match(&flat_piece, &board), None, "a piece of one tone carries no shape to match");
}

// A picture too small to hold a slider is not read at all.
#[test]
fn a_board_too_small_for_a_slider_is_not_read() {
  let small = png(&ImageBuffer::from_fn(10, 10, |_, _| Rgba([210, 200, 190, 255])));
  let piece = png(&ImageBuffer::from_fn(44, 44, |_, _| Rgba([210, 200, 190, 255])));

  assert_eq!(slide_comparison(&piece, &small), None);
}
