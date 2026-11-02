//! What a read of the web failed with, as every caller reads it: the kind of failure, the status and the
//! address where they are known, and whether the real Chrome was tried after the direct read failed. It
//! crosses a socket as it stands, a site tool's road answered by the web helper, so the site contract
//! names these without the web layer.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The kinds a web read fails by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
  /// A refusal or a challenge in place of the page.
  Blocked,
  /// This machine's own address policy refused the address before the site was asked.
  UnsafeAddress,
  /// The body was more than this machine reads.
  TooLarge,
  /// The connection, the name or the reading failed.
  Transport,
  /// An HTTP error that a browser would meet too.
  Http,
  /// The real Chrome, or a program this machine needs to read the page, could not be had.
  BrowserUnavailable,
}

/// A read of the web that did not give its page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebError {
  pub message: String,
  pub kind: Failure,
  pub status: Option<u16>,
  pub url: Option<String>,
  /// Whether the real Chrome, opened after the direct read failed, failed the page too: both roads a
  /// page is read by were tried.
  pub chrome_fallback: bool,
}

impl WebError {
  pub fn new(message: impl Into<String>, kind: Failure) -> Self {
    Self {
      message: message.into(),
      kind,
      status: None,
      url: None,
      chrome_fallback: false,
    }
  }

  pub fn at(mut self, status: Option<u16>, url: Option<&str>) -> Self {
    self.status = status;
    self.url = url.map(str::to_owned);
    self
  }

  pub fn with_url(mut self, url: &str) -> Self {
    self.url = Some(url.to_owned());
    self
  }

  /// The same failure, marked as the real Chrome's after the direct read's.
  pub fn after_chrome(mut self) -> Self {
    self.chrome_fallback = true;
    self
  }

  pub fn is(&self, kind: Failure) -> bool {
    self.kind == kind
  }
}

impl fmt::Display for WebError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.message)
  }
}

impl std::error::Error for WebError {}

pub type WebResult<T> = Result<T, WebError>;
