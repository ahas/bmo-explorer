//! A page's text as a document viewer lays it out: pieces of text, each placed where it stands on the page, with
//! no order or line of their own. Scribd's text layer and ManualsLib's pages both serve a PDF page this way, and
//! the lines a reader sees are rebuilt from where the pieces stand.

use std::sync::LazyLock;

use regex::Regex;

use super::units;

static PIXELS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[\d.]+px$").expect("a fixed pattern"));
static SPACE_SEPARATORS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\p{Zs}").expect("a fixed pattern"));
static PRIVATE_USE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Co}").expect("a fixed pattern"));

/// The replacement character, which stands in for a glyph the text does not spell out.
pub const UNSPELLED: char = '\u{fffd}';

/// A length in pixels an element's style attribute sets, or none where it sets none. The declarations are read
/// by a CSS reader, the last one of a name standing.
pub fn pixels(style: Option<&str>, property: &str) -> Option<f64> {
  let value = simplecss::DeclarationTokenizer::from(style.unwrap_or(""))
    .filter(|declaration| declaration.name.eq_ignore_ascii_case(property))
    .last()?
    .value
    .trim();

  if !PIXELS.is_match(value) {
    return None;
  }

  leading_float(value.trim_end_matches("px"))
}

// The number a text opens with, as far as it reads as one: "1.5.2" is 1.5.
fn leading_float(text: &str) -> Option<f64> {
  (1..=text.len())
    .rev()
    .filter(|end| text.is_char_boundary(*end))
    .find_map(|end| text[..end].parse::<f64>().ok())
}

/// A piece's text with every kind of space a space, and a private-use code point — a glyph the page's font draws
/// that the text does not spell out — the replacement character.
pub fn readable(text: &str) -> String {
  let spaced = SPACE_SEPARATORS.replace_all(text, " ");

  PRIVATE_USE.replace_all(&spaced, UNSPELLED.to_string()).into_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPiece {
  pub left: f64,
  pub top: f64,
  /// The font size the piece is drawn at, which says how tall its line is.
  pub size: f64,
  pub text: String,
}

impl PlacedPiece {
  fn foot(&self) -> f64 {
    self.top + self.size
  }
}

// Whether a piece stands clear of the one before it on its line, rather than going on from it: past where the
// earlier piece's letters would end at their widest, and half a letter more.
fn stands_apart(before: &PlacedPiece, piece: &PlacedPiece, letter_width: f64) -> bool {
  piece.left > before.left + units(&before.text) as f64 * before.size * letter_width + before.size * 0.5
}

/// The widest a letter of a page's fonts runs, as a share of its size, where the viewer draws the page in the
/// PDF's own fonts and so allows for any.
pub const ANY_LETTER_WIDTH: f64 = 0.7;

/// The lines of one page: pieces whose feet stand level are one line, read from left to right, and two pieces on
/// a line are one word run on unless the second stands clear of the first. `letter_width` is the widest a letter
/// of the page's fonts runs, as a share of its size: a viewer that draws the page in its own font knows it, and
/// one drawing in the PDF's fonts allows for any.
pub fn placed_lines(pieces: &[PlacedPiece], letter_width: f64) -> Vec<String> {
  let mut ordered: Vec<&PlacedPiece> = pieces.iter().collect();

  ordered.sort_by(|a, b| a.foot().total_cmp(&b.foot()).then(a.left.total_cmp(&b.left)));

  let mut lines: Vec<Vec<&PlacedPiece>> = Vec::new();

  for piece in ordered {
    match lines.last_mut() {
      Some(line) if (piece.foot() - line[0].foot()).abs() <= piece.size.max(line[0].size) * 0.4 => {
        line.push(piece)
      }
      _ => lines.push(vec![piece]),
    }
  }

  lines
    .into_iter()
    .map(|mut line| {
      line.sort_by(|a, b| a.left.total_cmp(&b.left));

      let mut joined = String::new();

      for (index, piece) in line.iter().enumerate() {
        let run_on = match index.checked_sub(1).map(|at| line[at]) {
          None => true,
          Some(before) => {
            before.text.ends_with(char::is_whitespace)
              || piece.text.starts_with(char::is_whitespace)
              || !stands_apart(before, piece, letter_width)
          }
        };

        if !run_on {
          joined.push(' ');
        }

        joined.push_str(&piece.text);
      }

      joined.split_whitespace().collect::<Vec<_>>().join(" ")
    })
    .filter(|line| !line.is_empty())
    .collect()
}
