//! A fragment's words as a reader sees them, read the way the web layer reads a page's text (web-layer's
//! classify): what a reader never sees dropped, a page's chrome a break, a block's end a line and a cell's a
//! tab, a picture by its alternative text, a link with its address after its words. A tool reading a part of
//! a page asks for a few things otherwise: a link read as its words alone, two spans standing side by side set
//! apart by a joint, and some elements left out altogether.

use ego_tree::NodeRef;
use scraper::Node;

const UNSEEN: [&str; 7] = ["script", "style", "noscript", "template", "svg", "canvas", "iframe"];
const CHROME: [&str; 4] = ["nav", "header", "footer", "aside"];
const LINE_ENDS: [&str; 23] = [
  "p",
  "div",
  "section",
  "article",
  "main",
  "li",
  "tr",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "blockquote",
  "pre",
  "table",
  "ul",
  "ol",
  "dl",
  "dt",
  "dd",
  "figure",
  "figcaption",
];
const CELL_ENDS: [&str; 2] = ["td", "th"];

/// How a fragment's words are read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain<'a> {
  /// A link read as its words alone, with no address after them.
  pub bare_links: bool,
  /// Set between two spans standing side by side with nothing between them.
  pub span_joint: Option<&'a str>,
  /// Elements by name left out with everything inside them.
  pub leave_out: &'a [&'a str],
}

fn name(node: NodeRef<'_, Node>) -> Option<&str> {
  match node.value() {
    Node::Element(element) => Some(element.name()),
    _ => None,
  }
}

fn seen_text(node: NodeRef<'_, Node>, plain: &Plain<'_>, out: &mut String) {
  for child in node.children() {
    match child.value() {
      Node::Text(text) => out.push_str(text),
      Node::Element(element)
        if !UNSEEN.contains(&element.name()) && !plain.leave_out.contains(&element.name()) =>
      {
        seen_text(child, plain, out)
      }
      _ => {}
    }
  }
}

fn reader_text(node: NodeRef<'_, Node>, plain: &Plain<'_>, out: &mut String) {
  for child in node.children() {
    one(child, plain, out);
  }
}

fn one(node: NodeRef<'_, Node>, plain: &Plain<'_>, out: &mut String) {
  match node.value() {
    Node::Text(text) => out.push_str(text),
    Node::Element(element) => element_text(node, element, plain, out),
    _ => {}
  }
}

fn element_text(
  node: NodeRef<'_, Node>,
  element: &scraper::node::Element,
  plain: &Plain<'_>,
  out: &mut String,
) {
  let tag = element.name();

  if UNSEEN.contains(&tag) || plain.leave_out.contains(&tag) {
    return;
  }

  if let Some(joint) = plain.span_joint
    && tag == "span"
    && node.prev_sibling().is_some_and(|before| name(before) == Some("span"))
  {
    out.push_str(joint);
  }

  if CHROME.contains(&tag) || tag == "br" || tag == "hr" {
    out.push('\n');
    return;
  }

  if tag == "img" {
    let alt = element.attr("alt").unwrap_or("").trim();

    if !alt.is_empty() {
      out.push_str(&format!("[image: {alt}]"));
    }

    return;
  }

  let href = if tag == "a" && !plain.bare_links { element.attr("href").unwrap_or("") } else { "" };

  if !href.is_empty() {
    let mut label = String::new();

    seen_text(node, plain, &mut label);

    let label = label.trim();
    let lower = href.to_lowercase();

    if label.is_empty() || lower.starts_with('#') || lower.starts_with("javascript:") {
      out.push_str(label);
    } else {
      out.push_str(&format!("{label} ({href})"));
    }

    return;
  }

  reader_text(node, plain, out);

  if LINE_ENDS.contains(&tag) {
    out.push('\n');
  } else if CELL_ENDS.contains(&tag) {
    out.push('\t');
  }
}

/// The words of some nodes as a reader sees them, tidied: runs of spacing one space, blank runs one line.
pub fn readable<'a>(nodes: impl IntoIterator<Item = NodeRef<'a, Node>>, plain: &Plain<'_>) -> String {
  let mut out = String::new();

  for node in nodes {
    one(node, plain, &mut out);
  }

  crate::forms::text::tidy(&out)
}

/// The words of some nodes as a reader sees them, then squeezed onto one line.
pub fn one_line<'a>(nodes: impl IntoIterator<Item = NodeRef<'a, Node>>, plain: &Plain<'_>) -> String {
  crate::site::readers::markup::squeeze(&readable(nodes, plain))
}
