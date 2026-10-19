//! A page a research run could not read by either road, as it is recorded (docs/site-contract.md, 「A page the
//! searcher could not read」): the site it belongs to and the shape the failure took. The reading is pure; the
//! run's web reading writes the records into the store's one list of them (docs/store.md).

use std::path::Path;
use std::sync::LazyLock;

use crate::forms::{Failure, WebError};
use regex::Regex;

/// The roads a recorded page failed by, as its record names them: the direct read, then the real Chrome.
pub const BOTH_ROADS: &str = "direct+chrome";

/// The site an address belongs to: its host, lower-cased, with a leading `www.`, `m.` or `mobile.` dropped so
/// a site's desktop and mobile pages are one site. None for what is not an address.
pub fn site_of(address: &str) -> Option<String> {
  let host = url::Url::parse(address).ok()?.host_str()?.to_lowercase();

  Some(
    ["www.", "m.", "mobile."]
      .iter()
      .find_map(|prefix| host.strip_prefix(prefix))
      .unwrap_or(&host)
      .to_owned(),
  )
}

static HOST: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}").expect("a fixed pattern")
});

/// The hosts a line of words names, as sites: `Example Finance (m.finance.example.com)` names finance.example.com.
pub fn sites_named_in(text: &str) -> Vec<String> {
  let lower = text.to_lowercase();
  let mut sites: Vec<String> = Vec::new();

  for found in HOST.find_iter(&lower) {
    // A host stands apart from the words around it: no word character, dot or hyphen touching either side, and
    // no single slash before it, which makes it a path's segment — `gasazip.com/search.html` names one site.
    let before = lower[..found.start()].chars().next_back();
    let after = lower[found.end()..].chars().next();
    let apart = |c: Option<char>, dot: bool| {
      c.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || (dot && c == '.')))
    };
    let in_path = before == Some('/') && !lower[..found.start()].ends_with("//");

    if !apart(before, true) || !apart(after, false) || in_path {
      continue;
    }

    if let Some(site) = site_of(&format!("https://{}/", found.as_str()))
      && !sites.contains(&site)
    {
      sites.push(site);
    }
  }

  sites
}

/// Every shape a failure is recorded as.
fn http_shape(status: Option<u16>) -> &'static str {
  match status {
    Some(401 | 407) => "login",
    Some(402) => "paywall",
    Some(404 | 410) => "gone",
    Some(408 | 504 | 524) => "timeout",
    Some(status) if status >= 500 => "server-error",
    Some(403 | 405 | 406 | 418 | 420 | 451) => "refused",
    _ => "other",
  }
}

/// What stood in the way of one fetch, read off the error it failed with; none where the real Chrome was not
/// tried after the direct read failed, and where the failure is this machine's own — the research closing,
/// the pacing past a deadline, Chrome not starting, an address refused as unsafe, a body too large.
pub fn fetch_failure_shape(error: &WebError) -> Option<(&'static str, String)> {
  if !error.chrome_fallback {
    return None;
  }

  let detail = error.message.clone();
  let lower = detail.to_lowercase();

  match error.kind {
    // The reason the page was classified with names the shape; every block met in Chrome also ends saying no
    // CAPTCHA was attempted, which names nothing about the page.
    Failure::Blocked => {
      let shape = if lower.contains("redirected to a login page") {
        "login"
      } else if lower.contains("application shell") || lower.contains("empty html page") {
        "script-only"
      } else if lower.contains("captcha or browser challenge") {
        "challenge"
      } else if error.status == Some(429) {
        "rate-limited"
      } else if error.status.is_some_and(|status| status >= 400) {
        http_shape(error.status)
      } else {
        "refused"
      };

      Some((shape, detail))
    }
    Failure::Deadline if lower.contains("host pacing") => None,
    Failure::Deadline => Some(("timeout", detail)),
    _ => None,
  }
}

/// The ERROR a web tool answers with. An address this machine's own policy refused is said to be the
/// machine's, so the run that met it does not name the page as one its site would not serve.
pub fn fetch_error_text(error: &WebError) -> String {
  let text = format!("ERROR: {}", error.message);

  if error.kind == Failure::UnsafeAddress {
    format!(
      "{text}\nThis machine's own address policy refused it before the site was asked: not a page to name as unread."
    )
  } else {
    text
  }
}

/// One page a research run could not read, as it is written.
pub struct Unread<'a> {
  pub site: &'a str,
  pub url: &'a str,
  pub shape: &'a str,
  pub detail: &'a str,
  /// The kind of run that met it: search, browse.
  pub run: &'a str,
  pub recorded_at: u64,
}

/// The record written into the store's list of unread pages, the store at `place`.
pub async fn record(place: &Path, unread: &Unread<'_>) -> Result<(), String> {
  let mut connection = crate::store::open(place).await?;

  sqlx::query(
    "INSERT INTO page_read_failures (site, url, shape, detail, via, run, recorded_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
  )
  .bind(unread.site)
  .bind(unread.url)
  .bind(unread.shape)
  .bind(unread.detail)
  .bind(BOTH_ROADS)
  .bind(unread.run)
  .bind(i64::try_from(unread.recorded_at).unwrap_or(i64::MAX))
  .execute(&mut connection)
  .await
  .map(drop)
  .map_err(|why| why.to_string())
}
