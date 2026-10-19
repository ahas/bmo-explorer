//! What a credential looks like, and a text written down without it: each shape's value masked, and the flag,
//! the name and the shape of the line kept. A browse's task may carry a login, so what is asked about it — the
//! knowledge its words call for — is asked with every credential shape masked.
//!
//! Some shapes look before or after a match, which the pattern reader does not, so each such shape is a plain
//! pattern with what it looks at checked beside it.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// What stands where a credential stood.
pub const MASKED_VALUE: &str = "«masked»";

// What a shape checks around a match before it is taken.
type Check = fn(&str, &Captures) -> bool;

struct Shape {
  pattern: Regex,
  check: Option<Check>,
  stand_in: fn(&Captures) -> String,
}

fn pattern(text: &str) -> Regex {
  Regex::new(text).expect("a credential shape is a pattern")
}

// The character before a byte position, where there is one.
fn before(text: &str, at: usize) -> Option<char> {
  text[..at].chars().next_back()
}

fn word_char(ch: char) -> bool {
  ch.is_ascii_alphanumeric() || ch == '_'
}

// Whether a match begins a word, as a word is counted in ASCII letters, digits and the underscore.
fn word_starts(text: &str, found: &Captures) -> bool {
  before(text, found.get(0).map_or(0, |whole| whole.start())).is_none_or(|ch| !word_char(ch))
}

fn not_masked(value: &str) -> bool {
  !value.starts_with('«')
}

static SHAPES: LazyLock<Vec<Shape>> = LazyLock::new(|| {
  vec![
    // An authorization header, whichever scheme it carries, the scheme a word of its own.
    Shape {
      pattern: pattern(r#"(?i)(Bearer|Basic|Token)\s+([^\s"'`,;]{8,})"#),
      check: Some(|text, found| {
        let start = found.get(0).map_or(0, |whole| whole.start());

        before(text, start).is_none_or(|ch| !word_char(ch) && ch != '-') && not_masked(&found[2])
      }),
      stand_in: |found| format!("{} {MASKED_VALUE}", &found[1]),
    },
    // A key of the shape the model providers issue.
    Shape {
      pattern: pattern(r"(?i)sk-([a-z]{3,8})-[A-Za-z0-9_-]{6,}"),
      check: Some(word_starts),
      stand_in: |found| format!("sk-{}-{MASKED_VALUE}", &found[1]),
    },
    Shape {
      pattern: pattern(r"(?i)sk-[A-Za-z0-9]{20,}"),
      check: Some(word_starts),
      stand_in: |_| format!("sk-{MASKED_VALUE}"),
    },
    // A password written into an address: scheme://name:password@host.
    Shape {
      pattern: pattern(r#"(?i)([a-z][a-z0-9+.-]{0,31}://[^\s:/@"'`]+:)([^\s@"'`]+)@"#),
      check: Some(|_, found| not_masked(&found[2])),
      stand_in: |found| format!("{}{MASKED_VALUE}@", &found[1]),
    },
    // A flag given its value on the command line, the flag opening a word.
    Shape {
      pattern: pattern(
        r#"(?i)(--?(?:password|passwd|pwd|token|secret|api-?key|apikey|auth|credential)s?[= ])([^\s"'`;&|]+)"#,
      ),
      check: Some(|text, found| {
        let start = found.get(0).map_or(0, |whole| whole.start());
        let value = &found[2];

        before(text, start).is_none_or(char::is_whitespace) && !value.starts_with('-') && not_masked(value)
      }),
      stand_in: |found| format!("{}{MASKED_VALUE}", &found[1]),
    },
    // An assignment: a shell variable, a line of an env file, a query string, a field of a config.
    Shape {
      pattern: pattern(
        r#"(?i)([A-Za-z0-9_.-]*(?:password|passwd|pwd|secret|token|api[_-]?key|apikey|access[_-]?key|credential)s?)(\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s"'`;&|,)}]+)"#,
      ),
      check: Some(|text, found| {
        let start = found.get(0).map_or(0, |whole| whole.start());

        before(text, start).is_none_or(|ch| !(word_char(ch) || ch == '.' || ch == '-'))
          && not_masked(&found[3])
      }),
      stand_in: |found| format!("{}{}{MASKED_VALUE}", &found[1], &found[2]),
    },
    // The console's own account script, whose second argument is the password.
    Shape {
      pattern: pattern(r#"(user\.ts\s+set\s+\S+\s+)([^\s"'`;&|]+)"#),
      check: Some(|_, found| not_masked(&found[2])),
      stand_in: |found| format!("{}{MASKED_VALUE}", &found[1]),
    },
  ]
});

// One shape laid over a text: each match it takes stood in for, a match its check refuses passed over by
// one character, as a pattern that looks around goes on from the next place.
fn replace(shape: &Shape, text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  let mut at = 0;

  while at <= text.len() {
    let Some(found) = shape.pattern.captures_at(text, at) else { break };
    let whole = found.get(0).expect("a match has its whole");

    if shape.check.is_none_or(|check| check(text, &found)) {
      out.push_str(&text[at..whole.start()]);
      out.push_str(&(shape.stand_in)(&found));
      at = if whole.end() > whole.start() { whole.end() } else { whole.end() + 1 };
      continue;
    }

    let next = whole.start() + text[whole.start()..].chars().next().map_or(1, char::len_utf8);

    out.push_str(&text[at..next.min(text.len())]);
    at = next;
  }

  if at < text.len() {
    out.push_str(&text[at..]);
  }

  out
}

/// One piece of text with every credential shape in it masked.
pub fn mask_secret_text(text: &str) -> String {
  SHAPES.iter().fold(text.to_owned(), |masked, shape| replace(shape, &masked))
}
