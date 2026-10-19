//! Page text made ready to be read: its runs of spacing made one space and its blank runs one blank line.
//! Every reader that shows a page's words tidies the same way, the web layer's classification and the
//! readers several site tools share alike.

use std::sync::LazyLock;

use regex::Regex;

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\u{a0}]+").expect("a fixed pattern"));
static AROUND_BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" *\n *").expect("a fixed pattern"));
static BLANK_RUNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").expect("a fixed pattern"));

/// Text with its runs of whitespace made one space, trimmed.
pub fn squeeze(text: &str) -> String {
  text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text with its runs of three line breaks and more made one blank line.
pub fn tidy_blank_runs(text: &str) -> String {
  BLANK_RUNS.replace_all(text, "\n\n").into_owned()
}

/// Text with its runs of spacing made one space and its blank runs one blank line.
pub fn tidy(text: &str) -> String {
  let text = SPACES.replace_all(text, " ");
  let text = AROUND_BREAKS.replace_all(&text, "\n");

  BLANK_RUNS.replace_all(&text, "\n\n").trim().to_owned()
}
