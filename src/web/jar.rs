//! A deliberately host-only cookie jar (docs/web-access.md, 「One session, one investigation」): a cookie is
//! kept for the host that set it and sent back only to that host, so an upstream cannot plant one for its
//! sibling hosts. A parent-domain cookie would need the public-suffix list to accept safely; keeping every
//! cookie host-only keeps sessions working without letting one fetched host touch another.

use std::collections::BTreeMap;

use cookie::{Cookie, Expiration};
use url::Url;

/// One cookie as a store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredCookie {
  pub host: String,
  pub path: String,
  pub name: String,
  pub value: String,
  pub secure: bool,
  pub expires_at: Option<u64>,
}

#[derive(Clone, Debug)]
struct Held {
  cookie: StoredCookie,
  created: u64,
}

#[derive(Default, Debug)]
pub struct Jar {
  cookies: BTreeMap<(String, String, String), Held>,
  sequence: u64,
}

// The path a cookie set without one takes: the directory of the page that set it.
fn default_path(path: &str) -> String {
  if !path.starts_with('/') || path == "/" {
    return "/".into();
  }

  match path.rfind('/') {
    Some(0) | None => "/".into(),
    Some(end) => path[..end].into(),
  }
}

fn token(name: &str) -> bool {
  !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+.^_`|~-".contains(c))
}

impl Jar {
  /// Every Set-Cookie line of an answer from `url`, taken in.
  pub fn capture<'a>(&mut self, url: &Url, lines: impl IntoIterator<Item = &'a str>, now: u64) {
    for line in lines {
      self.set(url, line, now);
    }
  }

  fn set(&mut self, url: &Url, line: &str, now: u64) {
    let Ok(parsed) = Cookie::parse(line.to_owned()) else {
      return;
    };
    let name = parsed.name().to_owned();
    let value = parsed.value().to_owned();

    if !token(&name) || value.contains(['\0', '\r', '\n', ';']) {
      return;
    }

    let host = crate::web::address::host_of(url);
    let domain = parsed.domain().map(|domain| domain.trim_start_matches('.').to_lowercase());

    if domain.as_ref().is_some_and(|domain| *domain != host) {
      return;
    }

    let path = parsed
      .path()
      .filter(|path| path.starts_with('/'))
      .map_or_else(|| default_path(url.path()), str::to_owned);
    let secure = parsed.secure() == Some(true);
    let https = url.scheme() == "https";

    if name.starts_with("__Secure-") && (!secure || !https) {
      return;
    }

    if name.starts_with("__Host-") && (!secure || !https || path != "/" || domain.is_some()) {
      return;
    }

    let (expires_at, remove) = match parsed.max_age() {
      Some(age) if age.whole_seconds() <= 0 => (Some(now), true),
      Some(age) => (Some(now + age.whole_seconds() as u64 * 1_000), false),
      None => match parsed.expires() {
        Some(Expiration::DateTime(at)) => {
          let ms = at.unix_timestamp_nanos() / 1_000_000;

          (Some(ms.max(0) as u64), false)
        }
        _ => (None, false),
      },
    };
    let key = (host.clone(), path.clone(), name.clone());

    if remove || expires_at.is_some_and(|at| at <= now) {
      self.cookies.remove(&key);
      return;
    }

    self.sequence += 1;
    self.cookies.insert(
      key,
      Held {
        cookie: StoredCookie {
          host,
          path,
          name,
          value,
          secure,
          expires_at,
        },
        created: self.sequence,
      },
    );
  }

  fn drop_spent(&mut self, now: u64) {
    self.cookies.retain(|_, held| held.cookie.expires_at.is_none_or(|at| at > now));
  }

  /// The Cookie header this jar sends to `url`, the longest paths first, or none.
  pub fn header(&mut self, url: &Url, now: u64) -> Option<String> {
    self.drop_spent(now);

    let host = crate::web::address::host_of(url);
    let path = url.path();
    let mut matches: Vec<&Held> = self
      .cookies
      .values()
      .filter(|held| held.cookie.host == host && (!held.cookie.secure || url.scheme() == "https"))
      .filter(|held| {
        let at = &held.cookie.path;
        let prefix = if at.ends_with('/') { at.clone() } else { format!("{at}/") };

        path == at || path.starts_with(&prefix)
      })
      .collect();

    matches.sort_by(|a, b| b.cookie.path.len().cmp(&a.cookie.path.len()).then(a.created.cmp(&b.created)));

    (!matches.is_empty()).then(|| {
      matches
        .iter()
        .map(|held| format!("{}={}", held.cookie.name, held.cookie.value))
        .collect::<Vec<_>>()
        .join("; ")
    })
  }

  /// Every cookie still alive, as a store keeps them.
  pub fn held(&mut self, now: u64) -> Vec<StoredCookie> {
    self.drop_spent(now);
    self.cookies.values().map(|held| held.cookie.clone()).collect()
  }

  /// Cookies a store kept, put back as if their hosts had just set them.
  pub fn hold(&mut self, cookies: &[StoredCookie], now: u64) {
    for cookie in cookies {
      if cookie.expires_at.is_some_and(|at| at <= now) {
        continue;
      }

      self.sequence += 1;
      self.cookies.insert(
        (cookie.host.clone(), cookie.path.clone(), cookie.name.clone()),
        Held {
          cookie: cookie.clone(),
          created: self.sequence,
        },
      );
    }
  }

  pub fn clear(&mut self) {
    self.cookies.clear();
  }
}
