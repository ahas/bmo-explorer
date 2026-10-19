//! A page's markup as a tree of elements and text, for a site tool that reads a page by its structure rather
//! than by a selector alone. html5ever builds the tree, tolerant the way a browser is; every reading here passes
//! over what a reader never sees, so only elements and text are read. A member's own markup — a forum comment,
//! a post — is read out of such a tree as the blocks a reader sees, and a table whose headings span columns and
//! rows is read as a grid, each figure named by the headings over it.

use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

/// Elements whose content is never text a reader sees.
pub const SKIPPED: [&str; 7] = ["script", "style", "noscript", "template", "svg", "textarea", "button"];

/// The elements a member's markup is read in blocks by.
pub const BLOCK_TAGS: [&str; 21] = [
  "p",
  "div",
  "section",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "pre",
  "blockquote",
  "ul",
  "ol",
  "li",
  "table",
  "thead",
  "tbody",
  "tr",
  "hr",
  "figure",
  "figcaption",
];

/// The markup as a tree.
pub fn parse_html(html: &str) -> Html {
  Html::parse_document(html)
}

/// A selector written in code, which is never wrong.
pub fn selector(text: &str) -> Selector {
  Selector::parse(text).unwrap_or_else(|why| panic!("the selector {text} does not read: {why:?}"))
}

/// Every element under `scope` that `css` matches, in document order, none inside what a reader never sees.
/// The scope itself is never one of them: it is where the search is made, as a selector engine handed one
/// element takes it.
pub fn select_all<'a>(scope: ElementRef<'a>, css: &str) -> Vec<ElementRef<'a>> {
  scope
    .select(&selector(css))
    .filter(|element| element.id() != scope.id() && !hidden(**element))
    .collect()
}

/// The first element under `scope` that `css` matches.
pub fn select_one<'a>(scope: ElementRef<'a>, css: &str) -> Option<ElementRef<'a>> {
  select_all(scope, css).into_iter().next()
}

/// Every element among some nodes and under them that `css` matches, the nodes themselves included, as a
/// selector engine handed a list of nodes takes them.
pub fn select_in<'a>(nodes: &[NodeRef<'a, Node>], css: &str) -> Vec<ElementRef<'a>> {
  let selector = selector(css);

  nodes
    .iter()
    .filter_map(|node| ElementRef::wrap(*node))
    .flat_map(|element| {
      std::iter::once(element)
        .filter(|one| selector.matches(one))
        .chain(element.select(&selector).filter(|one| one.id() != element.id()))
        .collect::<Vec<_>>()
    })
    .filter(|element| !hidden(**element))
    .collect()
}

/// Whether any element among some nodes, or under them, `ends` takes.
pub fn any_element<'a>(
  nodes: impl IntoIterator<Item = NodeRef<'a, Node>>,
  ends: &dyn Fn(ElementRef<'a>) -> bool,
) -> bool {
  nodes.into_iter().any(|node| node.descendants().filter_map(ElementRef::wrap).any(ends))
}

/// Every element of the page that `css` matches.
pub fn page_all<'a>(page: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
  page.select(&selector(css)).filter(|element| !hidden(**element)).collect()
}

/// The first element of the page that `css` matches.
pub fn page_one<'a>(page: &'a Html, css: &str) -> Option<ElementRef<'a>> {
  page.select(&selector(css)).find(|element| !hidden(**element))
}

/// The element's tag name, lower-cased.
pub fn name_of(node: NodeRef<'_, Node>) -> Option<&str> {
  match node.value() {
    Node::Element(element) => Some(element.name()),
    _ => None,
  }
}

fn skipped(node: NodeRef<'_, Node>) -> bool {
  name_of(node).is_some_and(|name| SKIPPED.contains(&name))
}

// Whether a node is, or stands inside, something a reader never sees.
fn hidden(node: NodeRef<'_, Node>) -> bool {
  skipped(node) || node.ancestors().any(skipped)
}

/// The attribute of an element, where it has one.
pub fn attr<'a>(element: ElementRef<'a>, name: &str) -> Option<&'a str> {
  element.value().attr(name)
}

/// Every piece of text under a node, in order, none inside what a reader never sees.
pub fn texts(node: NodeRef<'_, Node>) -> Vec<String> {
  let mut out = Vec::new();

  fn walk(node: NodeRef<'_, Node>, out: &mut Vec<String>) {
    match node.value() {
      Node::Text(text) => out.push(text.to_string()),
      Node::Element(_) if skipped(node) => {}
      Node::Element(_) | Node::Document | Node::Fragment => {
        node.children().for_each(|child| walk(child, out))
      }
      _ => {}
    }
  }

  walk(node, &mut out);
  out
}

/// The text under a node as one run, as the tree's own text content reads it.
pub fn text_content(node: NodeRef<'_, Node>) -> String {
  texts(node).concat()
}

/// The text under a node with its pieces kept apart, so `<span>$0.15</span><span>input</span>` reads as
/// "$0.15 input".
pub fn spaced(node: NodeRef<'_, Node>) -> String {
  squeeze(&texts(node).join(" "))
}

/// Text that carries its markup's characters as character references, read as the characters they stand for.
/// The text is read the way a browser reads a text area's content, where references are decoded and nothing
/// else is markup.
pub fn decode_entities(text: &str) -> String {
  let page =
    Html::parse_fragment(&format!("<textarea>{}</textarea>", text.replace("</textarea", "&lt;/textarea")));

  page
    .select(&selector("textarea"))
    .next()
    .map(|area| area.text().collect())
    .unwrap_or_else(|| text.to_owned())
}

/// Text with its runs of whitespace made one space, trimmed.
pub fn squeeze(text: &str) -> String {
  text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The element children of a node.
pub fn element_children(node: NodeRef<'_, Node>) -> impl Iterator<Item = ElementRef<'_>> {
  node.children().filter_map(ElementRef::wrap).filter(|element| !skipped(**element))
}

/// The next element after a node among its siblings.
pub fn next_element(node: NodeRef<'_, Node>) -> Option<ElementRef<'_>> {
  node.next_siblings().filter_map(ElementRef::wrap).find(|element| !skipped(**element))
}

/// How a member's markup is read: the origin an address written without its host is read against, and
/// whether each table reads as its grid's lines rather than its rows as written.
#[derive(Clone, Copy, Debug)]
pub struct Reading<'a> {
  pub origin: &'a str,
  pub tables_as_lines: bool,
}

impl<'a> Reading<'a> {
  pub fn at(origin: &'a str) -> Self {
    Self {
      origin,
      tables_as_lines: false,
    }
  }

  pub fn tables_as_lines(origin: &'a str) -> Self {
    Self {
      origin,
      tables_as_lines: true,
    }
  }
}

// An address as written, read against the origin where it names no scheme of its own.
fn absolute(href: &str, origin: &str) -> String {
  let schemed = href.split_once(':').is_some_and(|(scheme, _)| {
    let mut chars = scheme.chars();

    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
      && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
  });

  if schemed {
    return href.to_owned();
  }

  Url::parse(&format!("{origin}/"))
    .and_then(|base| base.join(href))
    .map_or_else(|_| href.to_owned(), |url| url.to_string())
}

// Inline markup as one line of text. Words that are the address cut short, as a forum shows a long one, are
// the address.
fn inline(node: NodeRef<'_, Node>, reading: Reading<'_>) -> String {
  match node.value() {
    Node::Text(text) => squeeze_keeping_edges(text),
    Node::Element(element) if !skipped(node) => match element.name() {
      "br" => "\n".into(),
      "img" => "[image]".into(),
      "code" => format!("`{}`", text_content(node)),
      "a" => {
        let words = node
          .children()
          .map(|child| inline(child, reading))
          .collect::<String>()
          .trim()
          .to_owned();
        let href = element.attr("href").map_or_else(String::new, |href| absolute(href, reading.origin));

        if href.is_empty() || words.is_empty() {
          if words.is_empty() { href } else { words }
        } else if words == href || (words.ends_with("...") && href.starts_with(&words[..words.len() - 3])) {
          href
        } else {
          format!("{words} ({href})")
        }
      }
      "table" if reading.tables_as_lines => table_paragraph(node, reading),
      "caption" if reading.tables_as_lines => String::new(),
      _ => node.children().map(|child| inline(child, reading)).collect(),
    },
    _ => String::new(),
  }
}

// A run of text with its whitespace made single spaces, its edges kept as one space where it had any.
fn squeeze_keeping_edges(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  let mut space = false;

  for c in text.chars() {
    if c.is_whitespace() {
      space = true;
    } else {
      if space {
        out.push(' ');
        space = false;
      }
      out.push(c);
    }
  }

  if space {
    out.push(' ');
  }

  out
}

fn paragraph<'a>(nodes: impl Iterator<Item = NodeRef<'a, Node>>, reading: Reading<'_>) -> String {
  let joined: String = nodes.map(|node| inline(node, reading)).collect();
  let lines: Vec<&str> = joined.split('\n').map(|line| line.trim_matches(' ')).collect();

  lines.join("\n").trim().to_owned()
}

fn is_block(node: NodeRef<'_, Node>, reading: Reading<'_>) -> bool {
  name_of(node)
    .is_some_and(|name| BLOCK_TAGS.contains(&name) || (reading.tables_as_lines && name == "caption"))
}

// A table read as its grid's lines, standing as one paragraph headed TABLE:.
fn table_paragraph(table: NodeRef<'_, Node>, reading: Reading<'_>) -> String {
  let lines = ElementRef::wrap(table)
    .map(|table| table_lines_reading(table, reading))
    .unwrap_or_default();

  std::iter::once("TABLE:".to_owned()).chain(lines).collect::<Vec<_>>().join("\n")
}

/// A member's markup as the blocks a reader sees, in order: paragraphs, headings, code, quotes, lists and
/// tables. A link keeps its address where its words are not the address.
pub fn markup_blocks(parent: NodeRef<'_, Node>, reading: Reading<'_>) -> Vec<String> {
  nodes_blocks(parent.children(), reading)
}

/// Some nodes of a member's markup as the blocks a reader sees, as `markup_blocks` reads a parent's.
pub fn nodes_blocks<'a>(
  nodes: impl IntoIterator<Item = NodeRef<'a, Node>>,
  reading: Reading<'_>,
) -> Vec<String> {
  let mut out = Vec::new();
  let mut run: Vec<NodeRef<'_, Node>> = Vec::new();
  let flush = |run: &mut Vec<NodeRef<'_, Node>>, out: &mut Vec<String>| {
    let text = paragraph(run.drain(..), reading);

    if !text.is_empty() {
      out.push(text);
    }
  };

  for node in nodes {
    if !is_block(node, reading) || skipped(node) {
      if !skipped(node) {
        run.push(node);
      }
      continue;
    }

    flush(&mut run, &mut out);

    let name = name_of(node).unwrap_or_default();

    match name {
      "pre" => out.push(format!(
        "```\n{}\n```",
        text_content(node).strip_suffix('\n').unwrap_or(&text_content(node))
      )),
      "blockquote" => {
        let quoted = markup_blocks(node, reading).join("\n\n");

        out.push(
          quoted
            .split('\n')
            .map(|line| if line.is_empty() { ">".to_owned() } else { format!("> {line}") })
            .collect::<Vec<_>>()
            .join("\n"),
        );
      }
      "ul" | "ol" => {
        let lines: Vec<String> = element_children(node)
          .filter(|item| item.value().name() == "li")
          .enumerate()
          .map(|(index, item)| {
            let marker = if name == "ol" { format!("{}.", index + 1) } else { "-".to_owned() };

            format!("{marker} {}", markup_blocks(*item, reading).join("\n").replace('\n', "\n  "))
          })
          .collect();

        if !lines.is_empty() {
          out.push(lines.join("\n"));
        }
      }
      "table" if reading.tables_as_lines => out.push(table_paragraph(node, reading)),
      "caption" => {}
      "table" => {
        let rows: Vec<String> = ElementRef::wrap(node)
          .into_iter()
          .flat_map(|table| select_all(table, "tr"))
          .map(|row| {
            element_children(*row)
              .filter(|cell| matches!(cell.value().name(), "td" | "th"))
              .map(|cell| paragraph(cell.children(), reading))
              .collect::<Vec<_>>()
              .join(" | ")
          })
          .collect();

        if !rows.is_empty() {
          out.push(rows.join("\n"));
        }
      }
      "hr" => out.push("---".into()),
      "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
        let text = paragraph(node.children(), reading);

        if !text.is_empty() {
          out.push(text);
        }
      }
      _ => out.extend(markup_blocks(node, reading)),
    }
  }

  flush(&mut run, &mut out);
  out
}

/// The lines a node shows, broken where the page puts a line break and nowhere else, each line's spaces collapsed
/// and the empty ones dropped.
pub fn broken_lines(node: NodeRef<'_, Node>) -> String {
  fn shown(node: NodeRef<'_, Node>, out: &mut String) {
    match node.value() {
      Node::Element(element) if element.name() == "br" => out.push('\n'),
      Node::Element(_) if skipped(node) => {}
      Node::Element(_) => node.children().for_each(|child| shown(child, out)),
      Node::Text(text) => {
        // Each run of white space is one space, as the page shows it, its ends kept.
        for one in text.chars() {
          if !one.is_whitespace() {
            out.push(one);
          } else if !out.ends_with(' ') || out.is_empty() {
            out.push(' ');
          }
        }
      }
      _ => {}
    }
  }

  let mut out = String::new();

  shown(node, &mut out);
  out
    .split('\n')
    .map(squeeze)
    .filter(|line| !line.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

/// A member's markup as blocks, each heading standing at its top marked as one with `## `.
pub fn headed_blocks(html: &str, origin: &str) -> Vec<String> {
  const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];

  if html.trim().is_empty() {
    return Vec::new();
  }

  let page = Html::parse_fragment(html);

  page
    .root_element()
    .children()
    .flat_map(|node| {
      let blocks = nodes_blocks(std::iter::once(node), Reading::at(origin));

      if name_of(node).is_some_and(|name| HEADINGS.contains(&name)) {
        blocks.into_iter().map(|text| format!("## {}", squeeze(&text))).collect()
      } else {
        blocks
      }
    })
    .collect()
}

/// A member's markup as plain text, blocks apart by a blank line.
pub fn markup_text(html: &str, origin: &str) -> String {
  let page = Html::parse_fragment(html);

  markup_blocks(*page.root_element(), Reading::at(origin)).join("\n\n")
}

// ---------------------------------------------------------------------------------------------
// A table as a grid. A heading cell spanning columns or rows stands over every column and row it spans, so a
// figure is named by the chain of headings above it — a year, then arrivals or departures — and a figure spanning several
// columns by the headings those columns share.

/// An element's words on one line, a line break or a block inside it standing as a space between them.
pub fn line_text(node: NodeRef<'_, Node>) -> String {
  line_text_reading(node, Reading::at(""))
}

fn line_text_reading(node: NodeRef<'_, Node>, reading: Reading<'_>) -> String {
  fn words(part: NodeRef<'_, Node>, reading: Reading<'_>) -> String {
    match part.value() {
      Node::Text(text) => text.to_string(),
      Node::Element(_) if skipped(part) => String::new(),
      Node::Element(element) => {
        let name = element.name();

        if name == "br" {
          return " ".into();
        }

        if reading.tables_as_lines && name == "caption" {
          return String::new();
        }

        if reading.tables_as_lines && name == "table" {
          return format!(" {} ", table_paragraph(part, reading).replace('\n', " "));
        }

        let inner: String = part.children().map(|child| words(child, reading)).collect();

        if BLOCK_TAGS.contains(&name) { format!(" {inner} ") } else { inner }
      }
      _ => String::new(),
    }
  }

  squeeze(&words(node, reading))
}

#[derive(Clone, Debug)]
struct Placed {
  row: usize,
  col: usize,
  cols: usize,
  text: String,
  heading: bool,
}

fn span(cell: ElementRef<'_>, name: &str) -> usize {
  cell
    .value()
    .attr(name)
    .and_then(|value| value.trim().parse::<usize>().ok())
    .filter(|value| *value > 0)
    .unwrap_or(1)
}

fn place_cells(rows: &[ElementRef<'_>], reading: Reading<'_>) -> Vec<Vec<Placed>> {
  let mut taken: Vec<Vec<bool>> = vec![Vec::new(); rows.len()];
  let mut covering: Vec<Vec<Placed>> = vec![Vec::new(); rows.len()];

  for (r, row) in rows.iter().enumerate() {
    let mut col = 0;

    for cell in element_children(**row).filter(|cell| matches!(cell.value().name(), "td" | "th")) {
      while taken[r].get(col).copied().unwrap_or(false) {
        col += 1;
      }

      let placed = Placed {
        row: r,
        col,
        cols: span(cell, "colspan"),
        text: line_text_reading(*cell, reading),
        heading: cell.value().name() == "th",
      };

      for down in 0..span(cell, "rowspan") {
        if r + down >= rows.len() {
          break;
        }

        for across in 0..placed.cols {
          let cells = &mut taken[r + down];

          if cells.len() <= col + across {
            cells.resize(col + across + 1, false);
          }

          cells[col + across] = true;
        }

        covering[r + down].push(placed.clone());
      }

      col += placed.cols;
    }
  }

  for cells in &mut covering {
    cells.sort_by_key(|cell| cell.col);
  }

  covering
}

fn shared_chain(chains: &[Vec<String>]) -> Vec<String> {
  let Some(first) = chains.first() else { return Vec::new() };
  let mut length = first.len();

  for chain in &chains[1..] {
    let mut same = 0;

    while same < length.min(chain.len()) && chain[same] == first[same] {
      same += 1;
    }

    length = same;
  }

  first[..length].to_vec()
}

/// The table's lines: one to a body row, its heading cells naming it and each figure named by the column
/// headings above it. A table with no heading rows is its rows as written.
pub fn table_lines(table: ElementRef<'_>) -> Vec<String> {
  table_lines_reading(table, Reading::at(""))
}

// The rows of a table and of the tables inside it, as the tree finds them under it.
fn rows_of(table: ElementRef<'_>) -> Vec<ElementRef<'_>> {
  select_all(table, "tr")
}

fn table_lines_reading(table: ElementRef<'_>, reading: Reading<'_>) -> Vec<String> {
  // A table inside another is read first and stands in the outer one as its lines, so its rows are not the
  // outer table's.
  let rows: Vec<ElementRef<'_>> = rows_of(table)
    .into_iter()
    .filter(|row| {
      !reading.tables_as_lines
        || row
          .ancestors()
          .filter_map(ElementRef::wrap)
          .find(|one| one.value().name() == "table")
          .is_some_and(|nearest| nearest.id() == table.id())
    })
    .collect();
  let covering = place_cells(&rows, reading);
  // The heading rows are the table's head, or where it has none, the rows of headings it opens with.
  let in_head = |row: &ElementRef<'_>| {
    row
      .parent()
      .and_then(|parent| name_of(parent).map(|name| name == "thead"))
      .unwrap_or(false)
  };
  let mut head_rows: Vec<usize> = Vec::new();

  if rows.iter().any(in_head) {
    head_rows.extend(rows.iter().enumerate().filter(|(_, row)| in_head(row)).map(|(r, _)| r));
  } else {
    let mut r = 0;

    while r + 1 < rows.len() && covering[r].iter().all(|cell| cell.heading) {
      head_rows.push(r);
      r += 1;
    }
  }

  let mut chains: Vec<Vec<String>> = Vec::new();

  for &r in &head_rows {
    for cell in &covering[r] {
      for col in cell.col..cell.col + cell.cols {
        if chains.len() <= col {
          chains.resize(col + 1, Vec::new());
        }

        let chain = &mut chains[col];

        if !cell.text.is_empty() && chain.last() != Some(&cell.text) {
          chain.push(cell.text.clone());
        }
      }
    }
  }

  let mut lines = Vec::new();

  for (r, cells) in covering.iter().enumerate() {
    if head_rows.contains(&r) {
      continue;
    }

    if chains.is_empty() {
      lines.push(cells.iter().map(|cell| cell.text.as_str()).collect::<Vec<_>>().join(" | "));
      continue;
    }

    let label = cells
      .iter()
      .filter(|cell| cell.heading)
      .map(|cell| cell.text.as_str())
      .filter(|text| !text.is_empty())
      .collect::<Vec<_>>()
      .join(" ");
    let figures: Vec<String> = cells
      .iter()
      .filter(|cell| !cell.heading && cell.row == r)
      .map(|cell| {
        let chained: Vec<Vec<String>> = (0..cell.cols)
          .map(|across| chains.get(cell.col + across).cloned().unwrap_or_default())
          .collect();
        let named = shared_chain(&chained).join(" ");

        if named.is_empty() { cell.text.clone() } else { format!("{named} {}", cell.text) }
      })
      .collect();

    if figures.is_empty() {
      if !label.is_empty() {
        lines.push(label);
      }
      continue;
    }

    lines.push(format!(
      "{}{}",
      if label.is_empty() { String::new() } else { format!("{label}: ") },
      figures.join(" · ")
    ));
  }

  lines
}

// ── A page's tree edited before it is read ──
//
// A site whose markup says what a reader sees only once its own script has run — a link opened from a click
// handler, a definition list the page lays out as lines — is put right in the tree before the reading. New nodes are
// made by the markup reader itself, out of a fragment written for them, so every node stands as the parser builds
// one.

fn escaped(text: &str) -> String {
  text
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
}

// The first element of a fragment written for it, as the parser builds it, with the text under it.
fn made(markup: &str, name: &str) -> Option<(Node, Option<Node>)> {
  let fragment = Html::parse_fragment(markup);
  let element = fragment.select(&selector(name)).next()?;
  let text = element
    .children()
    .find(|child| child.value().is_text())
    .map(|text| text.value().clone());

  Some((Node::Element(element.value().clone()), text))
}

/// Takes elements out of a page's tree, with everything under them.
pub fn detach(page: &mut Html, ids: impl IntoIterator<Item = ego_tree::NodeId>) {
  for id in ids {
    if let Some(mut node) = page.tree.get_mut(id) {
      node.detach();
    }
  }
}

/// Makes a link of a page's tree lead to an address, or to nothing where there is none, its words kept.
pub fn set_link(page: &mut Html, id: ego_tree::NodeId, address: Option<&str>) {
  let markup = address
    .map_or_else(|| "<a></a>".to_owned(), |address| format!("<a href=\"{}\"></a>", escaped(address)));

  if let (Some((link, _)), Some(mut node)) = (made(&markup, "a"), page.tree.get_mut(id)) {
    *node.value() = link;
  }
}

/// Opens an element with a heading of its own, as a page's script shows a panel under the name of the tab that opens
/// it.
pub fn prepend_heading(page: &mut Html, id: ego_tree::NodeId, text: &str) {
  if let (Some((heading, Some(words))), Some(mut node)) =
    (made(&format!("<h3>{}</h3>", escaped(text)), "h3"), page.tree.get_mut(id))
  {
    node.prepend(heading).append(words);
  }
}

/// Sets a paragraph of words before an element, as a page's script lifts a table's one-cell head into a title above it.
pub fn insert_paragraph_before(page: &mut Html, id: ego_tree::NodeId, text: &str) {
  if let (Some((paragraph, words)), Some(mut node)) =
    (made(&format!("<p>{}</p>", escaped(text)), "p"), page.tree.get_mut(id))
  {
    let mut inserted = node.insert_before(paragraph);

    if let Some(words) = words {
      inserted.append(words);
    }
  }
}

/// Puts one paragraph of lines in an element's place, a line break between each line and the next, as a page's script
/// lays a definition list out as its terms' lines.
pub fn replace_with_broken_lines(page: &mut Html, id: ego_tree::NodeId, lines: &[String]) {
  let (Some((paragraph, _)), Some((line_break, _))) = (made("<p></p>", "p"), made("<br>", "br")) else {
    return;
  };
  let children: Vec<ego_tree::NodeId> = page
    .tree
    .get(id)
    .map(|node| node.children().map(|child| child.id()).collect())
    .unwrap_or_default();

  detach(page, children);

  let Some(mut node) = page.tree.get_mut(id) else {
    return;
  };

  *node.value() = paragraph;

  for (index, line) in lines.iter().enumerate() {
    if index > 0 {
      node.append(line_break.clone());
    }

    if let Some((_, Some(words))) = made(&format!("<p>{}</p>", escaped(line)), "p") {
      node.append(words);
    }
  }
}

/// Gives an element another name, its children kept, as a page's style shows a button that carries words as the words
/// it carries.
pub fn rename_element(page: &mut Html, id: ego_tree::NodeId, name: &str) {
  if let (Some((renamed, _)), Some(mut node)) =
    (made(&format!("<{name}></{name}>"), name), page.tree.get_mut(id))
  {
    *node.value() = renamed;
  }
}

/// Puts words in an element's place, as a page's script draws a picture's name where the picture stood.
pub fn replace_with_text(page: &mut Html, id: ego_tree::NodeId, text: &str) {
  let children: Vec<ego_tree::NodeId> = page
    .tree
    .get(id)
    .map(|node| node.children().map(|child| child.id()).collect())
    .unwrap_or_default();

  detach(page, children);

  match made(&format!("<p>{}</p>", escaped(text)), "p") {
    Some((_, Some(words))) => {
      if let Some(mut node) = page.tree.get_mut(id) {
        *node.value() = words;
      }
    }
    _ => detach(page, [id]),
  }
}

/// Breaks a piece of text where its own lines break, a line break standing between each line and the next, as a
/// reader sees text written without paragraph markup.
pub fn break_text_lines(page: &mut Html, id: ego_tree::NodeId) {
  let Some(text) = page.tree.get(id).and_then(|node| node.value().as_text().map(|text| text.to_string()))
  else {
    return;
  };
  let Some((line_break, _)) = made("<br>", "br") else {
    return;
  };

  for (index, line) in text.split('\n').enumerate() {
    let Some(mut node) = page.tree.get_mut(id) else {
      return;
    };

    if index > 0 {
      node.insert_before(line_break.clone());
    }

    if let Some((_, Some(words))) = made(&format!("<p>{}</p>", escaped(line)), "p") {
      node.insert_before(words);
    }
  }

  detach(page, [id]);
}

/// Puts a block of lines in an element's place: the element becomes a division holding one paragraph to a line.
pub fn replace_with_lines(page: &mut Html, id: ego_tree::NodeId, lines: &[String]) {
  let Some((division, _)) = made("<div></div>", "div") else {
    return;
  };
  let children: Vec<ego_tree::NodeId> = page
    .tree
    .get(id)
    .map(|node| node.children().map(|child| child.id()).collect())
    .unwrap_or_default();

  detach(page, children);

  let Some(mut node) = page.tree.get_mut(id) else {
    return;
  };

  *node.value() = division;

  for line in lines {
    if let Some((paragraph, Some(text))) = made(&format!("<p>{}</p>", escaped(line)), "p") {
      node.append(paragraph).append(text);
    }
  }
}
