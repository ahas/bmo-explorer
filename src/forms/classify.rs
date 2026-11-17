//! What a fetched page is, read off its status, its markup and where its redirects ended: the content
//! itself, a refusal or a challenge in its place, an empty script shell, an error, or no content at all
//! (docs/web-access.md, 「How a fetched page is classified」). Page fetches, the search engines and the site
//! tools read a page the same way, so the rules stand in one place, below the web layer and the site
//! contract; and the page's text as a reader sees it.

use std::sync::LazyLock;

use ego_tree::NodeRef;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

// The page text's tidying, which a reader of the classified page reads by the same names.
pub use crate::forms::text::{tidy, tidy_blank_runs};

// What a reader never sees, and the page's chrome, which stands apart from its content as a break.
const UNSEEN: [&str; 7] = ["script", "style", "noscript", "template", "svg", "canvas", "iframe"];
const CHROME: [&str; 4] = ["nav", "header", "footer", "aside"];
// Elements whose end is the end of a line, and the table cells whose end is a tab.
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

fn seen_text(node: NodeRef<'_, Node>, out: &mut String) {
  for child in node.children() {
    match child.value() {
      Node::Text(text) => out.push_str(text),
      Node::Element(element) if !UNSEEN.contains(&element.name()) => seen_text(child, out),
      _ => {}
    }
  }
}

fn reader_text(node: NodeRef<'_, Node>, out: &mut String) {
  for child in node.children() {
    match child.value() {
      Node::Text(text) => out.push_str(text),
      Node::Element(_) => element_text(child, out),
      _ => {}
    }
  }
}

fn element_text(node: NodeRef<'_, Node>, out: &mut String) {
  let Node::Element(element) = node.value() else {
    return;
  };
  let name = element.name();

  if UNSEEN.contains(&name) {
    return;
  }

  if CHROME.contains(&name) || name == "br" || name == "hr" {
    out.push('\n');
    return;
  }

  if name == "img" {
    let alt = element.attr("alt").unwrap_or("").trim();

    if !alt.is_empty() {
      out.push_str(&format!("[image: {alt}]"));
    }

    return;
  }

  let href = if name == "a" { element.attr("href").unwrap_or("") } else { "" };

  if !href.is_empty() {
    let mut label = String::new();

    seen_text(node, &mut label);

    let label = label.trim();
    let lower = href.to_lowercase();

    if label.is_empty() || lower.starts_with('#') || lower.starts_with("javascript:") {
      out.push_str(label);
    } else {
      out.push_str(&format!("{label} ({href})"));
    }

    return;
  }

  reader_text(node, out);

  if LINE_ENDS.contains(&name) {
    out.push('\n');
  } else if CELL_ENDS.contains(&name) {
    out.push('\t');
  }
}

/// HTML to the text a reader would see: scripts, styles and the page's chrome are dropped, block elements
/// become line breaks, links keep their target after the text. Nothing is interpreted; a hidden instruction
/// in the page comes out as the words it is, as data.
pub fn html_to_text(html: &str) -> String {
  let page = Html::parse_document(html);

  readable_text(page.tree.root())
}

/// The same reading of a node a tool has already parsed out of a page.
pub fn readable_text(node: NodeRef<'_, Node>) -> String {
  let mut out = String::new();

  reader_text(node, &mut out);
  tidy(&out)
}

/// The same reading of one element and what it holds.
pub fn element_readable_text(element: ElementRef<'_>) -> String {
  let mut out = String::new();

  element_text(*element, &mut out);
  tidy(&out)
}

/// The readable body text past which a challenge widget is taken as the page's own form guard.
const OWN_BODY_CHARS: usize = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
  Content,
  Blocked,
  EmptyShell,
  HttpError,
  NoContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
  pub disposition: Disposition,
  pub reason: Option<String>,
  pub is_html: bool,
  pub text: String,
}

/// The words a page standing in place of its site's own is named with; the refusal statuses it came with are
/// added in parentheses where there are any.
pub const CHALLENGE: &str = "CAPTCHA or browser challenge";

impl Classified {
  /// Whether this classification named the page a challenge, with or without its statuses.
  pub fn is_challenge(&self) -> bool {
    self
      .reason
      .as_deref()
      .is_some_and(|reason| reason == CHALLENGE || reason.starts_with(&format!("{CHALLENGE} (")))
  }
}

/// What a page is classified from.
#[derive(Default)]
pub struct Fetched<'a> {
  pub status: u16,
  pub content_type: &'a str,
  pub body: &'a str,
  pub title: Option<&'a str>,
  pub visible_text: Option<&'a str>,
  /// The address asked for and the one the redirects ended on, where the caller followed any.
  pub requested_url: Option<&'a str>,
  pub final_url: Option<&'a str>,
}

// A widget's own token, never a word that merely contains one: `refresh-captcha` in a page's configuration
// is not `h-captcha`. Cloudflare's passive scripts under the challenge path's `scripts/` stand in ordinary
// pages it serves whole and are no challenge, and the field a solved widget fills in, `h-captcha-response`
// and its kin, is no widget either.
fn token_stands(body: &str, token: &str, not_followed_by: Option<&str>) -> bool {
  body.match_indices(token).any(|(at, _)| {
    let before = body[..at].chars().next_back();
    let after = &body[at + token.len()..];

    before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
      && not_followed_by.is_none_or(|tail| !after.starts_with(tail))
  })
}

fn cloudflare_challenge(lower: &str) -> bool {
  token_stands(lower, "cf-chl-", None)
    || token_stands(lower, "cdn-cgi/challenge-platform", Some("/scripts/"))
}

fn challenge_widget(lower: &str) -> bool {
  token_stands(lower, "g-recaptcha", Some("-response"))
    || token_stands(lower, "h-captcha", Some("-response"))
    || ["cf-turnstile", "arkose", "funcaptcha", "cap-widget"]
      .iter()
      .any(|token| token_stands(lower, token, None))
}

static CHALLENGE_TITLE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)^(?:(?:captcha|recaptcha|hcaptcha)\s*(?:$|[|—:-])|(?:just a moment|attention required|security verification|client challenge|are you (?:a )?human)(?:\b|\s*[|—:-])|verification\s*(?:$|[|—:-]))")
    .expect("a fixed pattern")
});
static NAMED_CHALLENGE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)(?:verify (?:that )?you are human|checking your browser before accessing|enable javascript and cookies to continue|performing security verification|unusual traffic from your computer network)")
    .expect("a fixed pattern")
});
static LOGIN_PATH: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)(?:^|[^a-z])(?:[a-z]*login|log-in|sign-?in)(?:[^a-z]|$)").expect("a fixed pattern")
});
static HTML_START: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)^\s*<(?:!doctype|html)").expect("a fixed pattern"));

fn selector(text: &str) -> Selector {
  Selector::parse(text).expect("a fixed selector")
}

// A login page by its path: a segment or file name that is a login or sign-in.
fn login_path(url: Option<&str>) -> bool {
  url
    .and_then(|url| Url::parse(url).ok())
    .is_some_and(|url| LOGIN_PATH.is_match(url.path()))
}

// Whether a form or a field of the page names a challenge: a form whose attributes name a captcha, a
// challenge or a check, or an input or a frame whose attributes name a captcha or a challenge.
fn challenge_form(page: &Html) -> bool {
  let names = |element: ElementRef<'_>, words: &[&str]| {
    element.value().attrs().any(|(name, value)| {
      let said = format!("{name} {value}").to_lowercase();

      words.iter().any(|word| said.contains(word))
    })
  };

  page
    .select(&selector("form"))
    .any(|form| names(form, &["captcha", "challenge", "verify"]))
    || page
      .select(&selector("input, iframe"))
      .any(|field| names(field, &["captcha", "challenge"]))
}

// Whether the body is a script application's shell: a script or noscript in it, or one of the roots the
// frameworks mount on.
fn application_shell(page: &Html) -> bool {
  page
    .select(&selector("body script, body noscript, #root, #app, #__next, #__nuxt"))
    .next()
    .is_some()
}

pub fn classify(input: &Fetched<'_>) -> Classified {
  let is_html = {
    let kind = input.content_type.to_lowercase();

    kind.contains("html")
      || kind.contains("xml")
      || (input.content_type.is_empty() && HTML_START.is_match(input.body))
  };
  let page = is_html.then(|| Html::parse_document(input.body));
  let converted = page
    .as_ref()
    .map_or_else(|| input.body.to_owned(), |page| readable_text(page.tree.root()));
  let squeeze = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
  let visible = squeeze(input.visible_text.unwrap_or(&converted));
  let title = match (input.title, &page) {
    (Some(title), _) => title.to_lowercase(),
    (None, Some(page)) => page
      .select(&selector("title"))
      .next()
      .map(|title| squeeze(&title.text().collect::<String>()).to_lowercase())
      .unwrap_or_default(),
    (None, None) => String::new(),
  };
  let lower = format!("{title}\n{}", visible.to_lowercase());
  let raw = input.body.to_lowercase();
  let cloudflare = cloudflare_challenge(&raw);
  let widget = challenge_widget(&raw);
  let body_text = match (&page, input.visible_text) {
    (Some(_), Some(visible)) => squeeze(visible),
    (Some(page), None) => {
      let body = page.select(&selector("body")).next();

      squeeze(&body.map_or_else(|| converted.clone(), element_readable_text))
    }
    (None, _) => visible.clone(),
  };
  let structural = cloudflare || (widget && body_text.chars().count() < OWN_BODY_CHARS);
  let challenge_title = CHALLENGE_TITLE.is_match(&title);
  let named = NAMED_CHALLENGE.is_match(&lower);
  let form = page.as_ref().is_some_and(challenge_form);
  let classified = |disposition, reason: Option<String>, text: String| Classified {
    disposition,
    reason,
    is_html,
    text,
  };
  let status = input.status;

  // 418 and 420 are refusals some sites send in place of 403 and 429. A refusal whose page is a challenge
  // is named one: a site that puts a CAPTCHA in front of one network answers it with 403.
  let refusal = matches!(status, 403 | 418 | 420);

  if refusal && (structural || challenge_title || named) {
    return classified(Disposition::Blocked, Some(format!("{CHALLENGE} (HTTP {status})")), converted);
  }

  if refusal || status == 429 {
    return classified(Disposition::Blocked, Some(format!("HTTP {status}")), converted);
  }

  if input.final_url.is_some()
    && input.final_url != input.requested_url
    && login_path(input.final_url)
    && !login_path(input.requested_url)
  {
    return classified(Disposition::Blocked, Some("redirected to a login page".into()), converted);
  }

  if structural || challenge_title || (named && (form || status == 503)) {
    return classified(Disposition::Blocked, Some(CHALLENGE.into()), converted);
  }

  if !(200..300).contains(&status) {
    return classified(Disposition::HttpError, Some(format!("HTTP {status}")), converted);
  }

  if status == 204 || status == 205 {
    return classified(Disposition::NoContent, Some(format!("HTTP {status}")), String::new());
  }

  if let Some(page) = &page
    && body_text.is_empty()
  {
    let reason = if application_shell(page) {
      "JavaScript application shell has no readable body"
    } else {
      "empty HTML page"
    };

    return classified(Disposition::EmptyShell, Some(reason.into()), converted);
  }

  classified(Disposition::Content, None, converted)
}
