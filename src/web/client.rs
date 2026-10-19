//! The direct client (docs/web-access.md, 「The web session inside one investigation」): a request as a
//! browser sends it, its redirects followed here one hop at a time with each hop checked and paced again,
//! the connection made to exactly the address that passed the check, the body read up to a bound, and its
//! text decoded in the charset it declared.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use url::Url;

use crate::web::address::{Check, SafeTarget, host_of};
use crate::web::clock::left;
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::identity::{self, FetchSite, Identity};
use crate::web::jar::Jar;
use crate::web::pacing::{Pacing, wait_for_host};

pub const DIRECT_HOST_INTERVAL_MS: u64 = 1_000;
pub const DIRECT_BYTE_CAP: usize = 8_000_000;
pub const MAX_REDIRECTS: usize = 10;

/// One request as it goes out: where the connection is made, under which name, with which headers.
pub struct Outgoing {
  pub method: Method,
  pub url: Url,
  /// The checked address the connection goes to; none on the trusted road, where the name is resolved.
  pub connect: Option<IpAddr>,
  pub headers: HeaderMap,
  pub body: Option<Vec<u8>>,
  pub timeout: Duration,
}

/// What came back, its body still to be read.
pub struct Incoming {
  pub status: u16,
  pub status_text: String,
  pub headers: HeaderMap,
  pub body: BoxStream<'static, Result<Bytes, String>>,
}

/// What sends a request: the network, or a test's stand-in.
pub trait Transport: Send + Sync {
  fn send(&self, outgoing: Outgoing) -> crate::web::pacing::Boxed<'_, Incoming>;
}

/// The network, through reqwest: one client per request, its redirects left to the caller and its name
/// pinned to the address the check passed, so the name rides only in the Host header and the TLS server name
/// the certificate is verified against.
pub struct Network;

impl Transport for Network {
  fn send(&self, outgoing: Outgoing) -> crate::web::pacing::Boxed<'_, Incoming> {
    Box::pin(async move {
      let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(outgoing.timeout);

      if let (Some(address), Some(host)) = (outgoing.connect, outgoing.url.host_str()) {
        let port = outgoing.url.port_or_known_default().unwrap_or(443);

        builder = builder
          .resolve(host.trim_start_matches('[').trim_end_matches(']'), SocketAddr::new(address, port));
      }

      let client = builder.build().map_err(|why| why.to_string())?;
      let mut request = client.request(outgoing.method, outgoing.url).headers(outgoing.headers);

      if let Some(body) = outgoing.body {
        request = request.body(body);
      }

      let response = request.send().await.map_err(|why| why.to_string())?;
      let status = response.status();

      Ok(Incoming {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_owned(),
        headers: response.headers().clone(),
        body: response.bytes_stream().map(|chunk| chunk.map_err(|why| why.to_string())).boxed(),
      })
    })
  }
}

/// A response read as bytes, before any decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectBytes {
  pub url: String,
  pub status: u16,
  pub status_text: String,
  /// The media type alone, lower-cased, and the header as it was sent.
  pub content_type: String,
  pub raw_content_type: String,
  pub bytes: Vec<u8>,
  pub truncated: bool,
  pub retry_after_ms: Option<u64>,
}

/// A response read as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectDocument {
  pub url: String,
  pub status: u16,
  pub status_text: String,
  pub content_type: String,
  pub body: String,
  pub truncated: bool,
  pub retry_after_ms: Option<u64>,
}

/// Everything one direct request goes by.
pub struct Direct<'a> {
  pub store: &'a dyn Pacing,
  pub jar: &'a tokio::sync::Mutex<Jar>,
  pub deadline_ms: u64,
  pub check: &'a Check,
  pub transport: &'a dyn Transport,
  pub interval_ms: u64,
  /// Headers of the caller's own, which never replace the browser's.
  pub headers: Vec<(String, String)>,
  pub identity: Identity,
  pub byte_cap: usize,
}

impl<'a> Direct<'a> {
  pub fn new(
    store: &'a dyn Pacing,
    jar: &'a tokio::sync::Mutex<Jar>,
    deadline_ms: u64,
    check: &'a Check,
    transport: &'a dyn Transport,
  ) -> Self {
    Self {
      store,
      jar,
      deadline_ms,
      check,
      transport,
      interval_ms: DIRECT_HOST_INTERVAL_MS,
      headers: Vec::new(),
      identity: identity::DEFAULT,
      byte_cap: DIRECT_BYTE_CAP,
    }
  }
}

// How the navigation to `current` stands to the one before it. Exact hosts and a parent and its subdomain
// are certainly one site; sibling domains would need the public-suffix list to prove it, so they are called
// cross-site rather than guessed.
fn fetch_site(first: &Url, previous: Option<&Url>, current: &Url) -> FetchSite {
  let Some(previous) = previous else {
    return FetchSite::None;
  };

  if previous.origin() == current.origin() {
    return FetchSite::SameOrigin;
  }

  let source = host_of(first);
  let destination = host_of(current);

  if source == destination
    || source.ends_with(&format!(".{destination}"))
    || destination.ends_with(&format!(".{source}"))
  {
    FetchSite::SameSite
  } else {
    FetchSite::CrossSite
  }
}

const SENSITIVE: [&str; 5] = ["authorization", "proxy-authorization", "cookie", "origin", "referer"];

fn redirect_status(status: u16) -> bool {
  matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn retry_after(headers: &HeaderMap, now: u64) -> Option<u64> {
  let raw = headers.get("retry-after")?.to_str().ok()?.trim();

  if let Ok(seconds) = raw.parse::<u64>() {
    return Some(seconds * 1_000);
  }

  let at = httpdate::parse_http_date(raw).ok()?;
  let ms = at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;

  Some(ms.saturating_sub(now))
}

fn origin_of(url: &Url) -> String {
  url.origin().ascii_serialization()
}

/// The body read up to `cap` bytes, and whether the rest was cut.
pub async fn read_bounded(
  mut body: BoxStream<'static, Result<Bytes, String>>,
  cap: usize,
) -> Result<(Vec<u8>, bool), String> {
  let mut bytes = Vec::new();

  while let Some(chunk) = body.next().await {
    let chunk = chunk?;
    let room = cap.saturating_sub(bytes.len());

    if chunk.len() > room {
      bytes.extend_from_slice(&chunk[..room]);
      return Ok((bytes, true));
    }

    bytes.extend_from_slice(&chunk);
  }

  Ok((bytes, false))
}

impl Direct<'_> {
  /// The page at `value`, as bytes.
  pub async fn bytes(&self, value: &str) -> WebResult<DirectBytes> {
    self.send(Method::GET, value, None).await
  }

  /// The page at `value`, as text.
  pub async fn document(&self, value: &str) -> WebResult<DirectDocument> {
    let read = self.bytes(value).await?;

    Ok(DirectDocument {
      body: decode(&read.bytes, &read.raw_content_type),
      url: read.url,
      status: read.status,
      status_text: read.status_text,
      content_type: read.content_type,
      truncated: read.truncated,
      retry_after_ms: read.retry_after_ms,
    })
  }

  /// One request, its redirects followed one hop at a time.
  pub async fn send(&self, method: Method, value: &str, body: Option<Vec<u8>>) -> WebResult<DirectBytes> {
    let clock = &*self.check.clock;
    let first = crate::web::address::web_url(value)?;

    self.check.resolve(&first, Some(self.deadline_ms)).await?;

    let mut current = first.clone();
    let mut previous: Option<Url> = None;

    for redirects in 0..=MAX_REDIRECTS {
      let target: SafeTarget = self.check.resolve(&current, Some(self.deadline_ms)).await?;
      let key = format!("page:{}", host_of(&current));

      wait_for_host(self.store, &key, self.interval_ms, self.deadline_ms, clock).await?;

      if clock.now() >= self.deadline_ms {
        return Err(
          WebError::new(format!("Web deadline reached before {current}"), Failure::Deadline)
            .with_url(current.as_str()),
        );
      }

      let site = fetch_site(&first, previous.as_ref(), &current);
      let mut headers = HeaderMap::new();

      for (name, value) in identity::navigation_headers(site, self.identity) {
        if let Ok(value) = HeaderValue::from_str(&value) {
          headers.insert(name, value);
        }
      }

      // A caller may add credentials or an application header, but never turn the coherent browser identity
      // back into a mixed one.
      for (name, value) in &self.headers {
        if let (Ok(name), Ok(value)) =
          (HeaderName::try_from(name.to_lowercase()), HeaderValue::from_str(value))
          && !headers.contains_key(&name)
        {
          headers.insert(name, value);
        }
      }

      if let Some(previous) = &previous {
        if previous.origin() != current.origin() {
          for name in SENSITIVE {
            headers.remove(name);
          }

          headers.insert("sec-fetch-site", HeaderValue::from_static(site.header()));
          if let Ok(referer) = HeaderValue::from_str(&format!("{}/", origin_of(previous))) {
            headers.insert("referer", referer);
          }
        } else if !headers.contains_key("referer")
          && let Ok(referer) = HeaderValue::from_str(previous.as_str())
        {
          headers.insert("referer", referer);
        }
      }

      if let Some(held) = self.jar.lock().await.header(&current, clock.now()) {
        let joined = match headers.get("cookie").and_then(|supplied| supplied.to_str().ok()) {
          Some(supplied) => format!("{supplied}; {held}"),
          None => held,
        };

        if let Ok(value) = HeaderValue::from_str(&joined) {
          headers.insert("cookie", value);
        }
      }

      let literal = host_of(&current).parse::<IpAddr>().is_ok();
      let outgoing = Outgoing {
        method: if redirects == 0 { method.clone() } else { Method::GET },
        url: current.clone(),
        connect: if literal { None } else { target.address },
        headers,
        body: if redirects == 0 { body.clone() } else { None },
        timeout: left(self.deadline_ms, clock.now()),
      };
      let sent =
        tokio::time::timeout(left(self.deadline_ms, clock.now()), self.transport.send(outgoing)).await;
      let incoming = match sent {
        Err(_) => {
          return Err(
            WebError::new(format!("Web deadline reached while reading {current}"), Failure::Deadline)
              .with_url(current.as_str()),
          );
        }
        Ok(Err(why)) => {
          return Err(
            WebError::new(format!("Web request failed for {current}: {why}"), Failure::Transport)
              .with_url(current.as_str()),
          );
        }
        Ok(Ok(incoming)) => incoming,
      };
      let set_cookies: Vec<String> = incoming
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect();

      self
        .jar
        .lock()
        .await
        .capture(&current, set_cookies.iter().map(String::as_str), clock.now());

      if redirect_status(incoming.status) {
        let location = incoming
          .headers
          .get("location")
          .and_then(|value| value.to_str().ok())
          .map(str::to_owned);
        let Some(location) = location else {
          return Err(
            WebError::new(
              format!("HTTP {} redirect without Location for {current}", incoming.status),
              Failure::Http,
            )
            .at(Some(incoming.status), Some(current.as_str())),
          );
        };

        if redirects == MAX_REDIRECTS {
          return Err(
            WebError::new(format!("Too many redirects for {first}"), Failure::Http)
              .at(Some(incoming.status), Some(current.as_str())),
          );
        }

        let next = current.join(&location).map_err(|_| {
          WebError::new(format!("Invalid web URL: {location}"), Failure::UnsafeAddress)
            .with_url(current.as_str())
        })?;

        previous = Some(current);
        current = next;
        continue;
      }

      let left_now = left(self.deadline_ms, clock.now());
      let (bytes, truncated) =
        match tokio::time::timeout(left_now, read_bounded(incoming.body, self.byte_cap)).await {
          Err(_) => {
            return Err(
              WebError::new(format!("Web deadline reached while reading {current}"), Failure::Deadline)
                .with_url(current.as_str()),
            );
          }
          Ok(Err(why)) => {
            return Err(
              WebError::new(format!("Web request failed for {current}: {why}"), Failure::Transport)
                .with_url(current.as_str()),
            );
          }
          Ok(Ok(read)) => read,
        };
      let raw_content_type = incoming
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
      let content_type = raw_content_type.split(';').next().unwrap_or("").trim().to_lowercase();

      return Ok(DirectBytes {
        // Redirects are followed here, so the URL answered is the one asked, by its name.
        url: current.to_string(),
        status: incoming.status,
        status_text: incoming.status_text,
        content_type,
        raw_content_type,
        bytes,
        truncated,
        retry_after_ms: retry_after(&incoming.headers, clock.now()),
      });
    }

    Err(WebError::new(format!("Too many redirects for {first}"), Failure::Http).with_url(current.as_str()))
  }
}

/// The charset a body declared: the header's, or for an HTML or XML page its own meta declaration, read off
/// the page's head as a parser reads it.
fn declared_charset(content_type: &str, bytes: &[u8]) -> Option<String> {
  let header = content_type.split(';').skip(1).find_map(|part| {
    let (name, value) = part.split_once('=')?;

    (name.trim().eq_ignore_ascii_case("charset")).then(|| value.trim().trim_matches(['"', '\'']).to_owned())
  });

  if header.is_some() {
    return header;
  }

  let lower = content_type.to_lowercase();

  if !(lower.contains("text/html")
    || lower.contains("application/xhtml+xml")
    || lower.contains("application/xml"))
  {
    return None;
  }

  // The head is read as single bytes, which every charset a page may declare agrees with in ASCII.
  let head: String = bytes.iter().take(4_096).map(|byte| char::from(*byte)).collect();
  let page = scraper::Html::parse_document(&head);
  let meta = scraper::Selector::parse("meta").ok()?;

  page.select(&meta).find_map(|element| {
    let attributes = element.value();

    if let Some(charset) = attributes.attr("charset") {
      return Some(charset.trim().to_owned());
    }

    let content = attributes.attr("content")?;

    content.split(';').find_map(|part| {
      let (name, value) = part.split_once('=')?;

      (name.trim().eq_ignore_ascii_case("charset"))
        .then(|| value.trim().trim_matches(['"', '\'']).to_owned())
    })
  })
}

/// A body as text, in the charset it declared, UTF-8 where it declared none this reader knows.
pub fn decode(bytes: &[u8], content_type: &str) -> String {
  let declared = declared_charset(content_type, bytes).unwrap_or_default().to_lowercase();
  let label = if declared == "ks_c_5601-1987" || declared == "ks-c-5601-1987" {
    "euc-kr"
  } else {
    declared.as_str()
  };
  let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
  let (text, _, _) = encoding.decode(bytes);

  text.into_owned()
}

/// The transport every read takes where its caller hands none.
pub fn network() -> Arc<dyn Transport> {
  Arc::new(Network)
}
