//! The address check every outward request of the web layer passes (docs/web-access.md, 「Address checks」): only
//! `http` and `https`, no credentials in the address, and a name read only where every address it resolves
//! to stands on the public internet. The address that passed is what the connection is made to, so a name
//! that answers otherwise a moment later is never asked again.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::Arc;

use url::Url;

use crate::web::clock::Clock;
use crate::web::error::{Failure, WebError, WebResult};

/// How a name is resolved: the system's resolver, or a test's own.
pub trait Lookup: Send + Sync {
  fn lookup<'a>(
    &'a self,
    host: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>>;
}

/// The system's resolver.
pub struct SystemLookup;

impl Lookup for SystemLookup {
  fn lookup<'a>(
    &'a self,
    host: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    Box::pin(async move {
      tokio::net::lookup_host((host, 0))
        .await
        .map(|found| found.map(|address| address.ip()).collect())
        .map_err(|why| why.to_string())
    })
  }
}

// An IPv4 address in a special-purpose range, each by its registered prefix: a public block beside one —
// a hosting provider's 192.0.64.0/18 beside the protocol block 192.0.0.0/24 — is not refused.
fn forbidden_v4(address: Ipv4Addr) -> bool {
  const RANGES: [([u8; 4], u32); 13] = [
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 0, 2, 0], 24),
    ([192, 168, 0, 0], 16),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 3),
  ];
  let bits = u32::from(address);

  RANGES.iter().any(|(network, prefix)| {
    let mask = if *prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };

    bits & mask == u32::from(Ipv4Addr::from(*network)) & mask
  })
}

fn forbidden_v6(address: Ipv6Addr) -> bool {
  if let Some(v4) = address.to_ipv4_mapped() {
    return forbidden_v4(v4);
  }

  let first = address.segments()[0];

  address.is_unspecified()
    || address.is_loopback()
    // Unique local fc00::/7, link-local fe80::/10, multicast ff00::/8.
    || first & 0xfe00 == 0xfc00
    || first & 0xffc0 == 0xfe80
    || first & 0xff00 == 0xff00
    // Documentation 2001:db8::/32.
    || (first == 0x2001 && address.segments()[1] == 0x0db8)
}

/// Whether an address is one this machine never connects to at an agent's asking.
pub fn is_forbidden(address: IpAddr) -> bool {
  match address {
    IpAddr::V4(v4) => forbidden_v4(v4),
    IpAddr::V6(v6) => forbidden_v6(v6),
  }
}

/// A URL that passed the check, and the address it passed with. A trusted road that allows the private
/// network resolves nothing and has no address to hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeTarget {
  pub url: Url,
  pub address: Option<IpAddr>,
}

/// How an address is checked: by which resolver, on which clock the reads under it are paced, and whether the
/// private network is the caller's own trusted road.
#[derive(Clone)]
pub struct Check {
  pub allow_private: bool,
  pub lookup: Arc<dyn Lookup>,
  pub clock: Arc<dyn Clock>,
}

impl Default for Check {
  fn default() -> Self {
    Self {
      allow_private: false,
      lookup: Arc::new(SystemLookup),
      clock: crate::web::clock::system(),
    }
  }
}

/// An address as it is written into a URL's host.
pub fn host_of(url: &Url) -> String {
  url
    .host_str()
    .unwrap_or("")
    .trim_start_matches('[')
    .trim_end_matches(']')
    .to_lowercase()
}

/// A text read as a web address, or the check's refusal of it.
pub fn web_url(value: &str) -> WebResult<Url> {
  Url::parse(value).map_err(|_| WebError::new(format!("Invalid web URL: {value}"), Failure::UnsafeAddress))
}

impl Check {
  /// The check a browser request passes, answering with the address it passed at.
  pub fn guard(&self) -> crate::web::chrome::UrlGuard {
    let check = self.clone();

    Arc::new(move |url: String| {
      let check = check.clone();

      Box::pin(async move { Ok(check.resolve(&web_url(&url)?).await?.address) })
    })
  }

  /// The URL checked: its scheme, its credentials and every address its name resolves to, the lookup waited
  /// for until the resolver answers; the first address answers where the connection goes.
  pub async fn resolve(&self, url: &Url) -> WebResult<SafeTarget> {
    let text = url.as_str();

    if url.scheme() != "http" && url.scheme() != "https" {
      return Err(
        WebError::new(format!("Unsupported web URL protocol: {}:", url.scheme()), Failure::UnsafeAddress)
          .with_url(text),
      );
    }

    if !url.username().is_empty() || url.password().is_some() {
      return Err(
        WebError::new("Credentials in web URLs are not allowed", Failure::UnsafeAddress).with_url(text),
      );
    }

    let host = host_of(url);

    if self.allow_private {
      return Ok(SafeTarget {
        url: url.clone(),
        address: None,
      });
    }

    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
      return Err(
        WebError::new(format!("Private web address is not allowed: {host}"), Failure::UnsafeAddress)
          .with_url(text),
      );
    }

    let addresses = match host.parse::<IpAddr>() {
      Ok(literal) => vec![literal],
      Err(_) => self.lookup.lookup(&host).await.map_err(|why| {
        WebError::new(format!("Could not resolve web host {host}: {why}"), Failure::Transport)
          .with_url(text)
      })?,
    };

    if addresses.is_empty() || addresses.iter().copied().any(is_forbidden) {
      return Err(
        WebError::new(
          format!("Private or non-routable web address is not allowed: {host}"),
          Failure::UnsafeAddress,
        )
        .with_url(text),
      );
    }

    Ok(SafeTarget {
      url: url.clone(),
      address: addresses.first().copied(),
    })
  }
}
