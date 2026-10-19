//! The readings more than one site tool stands on (docs/site-contract.md): a page's markup as a tree of nodes, a
//! journal article, a Discourse forum, a Tapatalk forum, and text placed on a page by its coordinates.

pub mod discourse;
pub mod journal;
pub mod market;
pub mod markup;
pub mod numbers;
pub mod ordered;
pub mod placed;
pub mod plain;
pub mod script;
pub mod tapatalk;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

// What an address component keeps as it stands, as a page's own encodeURIComponent keeps it.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'_')
  .remove(b'.')
  .remove(b'!')
  .remove(b'~')
  .remove(b'*')
  .remove(b'\'')
  .remove(b'(')
  .remove(b')');

/// A text's length as a page's own scripts count it, in UTF-16 units.
pub fn units(text: &str) -> usize {
  text.encode_utf16().count()
}

/// The start of a text, at most `count` UTF-16 units of it, never splitting a character.
pub fn take_units(text: &str, count: usize) -> &str {
  let mut used = 0;

  for (at, one) in text.char_indices() {
    used += one.len_utf16();

    if used > count {
      return &text[..at];
    }
  }

  text
}

/// Blocks of a conversation laid into one answer's room: every one that fits, the first always, a long one cut.
/// What comes back is the blocks kept and how many of the given ones they are.
pub fn fitted(blocks: &[String], room: usize) -> (Vec<String>, usize) {
  fitted_from(blocks, room, 1)
}

/// A long block cut to one answer's room left, two thousand of its characters kept however little room is left.
pub fn cut_to(block: &str, room: usize) -> String {
  if units(block) > room {
    format!(
      "{}\n… (it goes on; it is cut here at the answer's length)",
      take_units(block, room.max(2_000))
    )
  } else {
    block.to_owned()
  }
}

/// Blocks laid into one answer's room as `fitted` lays them, the first `always` of them shown whatever their length.
pub fn fitted_from(blocks: &[String], room: usize, always: usize) -> (Vec<String>, usize) {
  let mut kept = Vec::new();
  let mut used = 0;

  for (index, block) in blocks.iter().enumerate() {
    let left = room.saturating_sub(used);

    if index >= always && units(block) > left {
      return (kept, index);
    }

    let shown = cut_to(block, left);

    used += units(&shown);
    kept.push(shown);
  }

  (kept, blocks.len())
}

/// A text written into an address as one component of it, as a page's own encodeURIComponent writes it.
pub fn url_component(text: &str) -> String {
  utf8_percent_encode(text, COMPONENT).to_string()
}
