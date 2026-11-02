//! What the tests of this crate stand on: a web answered by host, a resolver that names every host public,
//! and a browser that answers with a page it was handed.

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue};

use bmo_explorer::web::address::{Check, Lookup};
use bmo_explorer::web::chrome::{BrowserDocument, BrowserFetcher, FetchOptions};
use bmo_explorer::web::client::{Incoming, Outgoing, Transport};
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::error::WebResult;

/// One answer: its status, its headers and its body.
pub struct Answer {
  pub status: u16,
  pub headers: Vec<(&'static str, String)>,
  pub body: Vec<u8>,
}

pub fn html(body: &str) -> Answer {
  Answer {
    status: 200,
    headers: vec![("content-type", "text/html; charset=utf-8".into())],
    body: body.as_bytes().to_vec(),
  }
}

pub fn answer(status: u16, headers: &[(&'static str, &str)], body: &[u8]) -> Answer {
  Answer {
    status,
    headers: headers.iter().map(|(name, value)| (*name, (*value).to_owned())).collect(),
    body: body.to_vec(),
  }
}

/// What one request was: the host it named, its path, and its headers.
#[derive(Clone, Debug)]
pub struct Asked {
  pub host: String,
  pub path: String,
  pub user_agent: String,
  pub cookie: Option<String>,
}

pub type Route = Box<dyn Fn(&Asked) -> Answer + Send + Sync>;

/// The web as the hosts named answer it; a host not named answers 404.
#[derive(Default)]
pub struct Web {
  pub routes: HashMap<String, Route>,
  pub asked: Mutex<Vec<Asked>>,
}

impl Web {
  pub fn route(mut self, host: &str, route: impl Fn(&Asked) -> Answer + Send + Sync + 'static) -> Self {
    self.routes.insert(host.to_owned(), Box::new(route));
    self
  }

  pub fn asked(&self) -> Vec<Asked> {
    self.asked.lock().unwrap().clone()
  }

  pub fn forget(&self) {
    self.asked.lock().unwrap().clear();
  }
}

impl Transport for Web {
  fn send(&self, outgoing: Outgoing) -> bmo_explorer::web::pacing::Boxed<'_, Incoming> {
    let header = |headers: &HeaderMap, name: &str| {
      headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
    };
    let asked = Asked {
      host: outgoing.url.host_str().unwrap_or("").to_owned(),
      path: outgoing.url.path().to_owned(),
      user_agent: header(&outgoing.headers, "user-agent").unwrap_or_default(),
      cookie: header(&outgoing.headers, "cookie"),
    };

    self.asked.lock().unwrap().push(asked.clone());

    let found = match self.routes.get(&asked.host) {
      Some(route) => route(&asked),
      None => answer(404, &[("content-type", "text/html")], b"<p>no such host</p>"),
    };
    let mut headers = HeaderMap::new();

    for (name, value) in found.headers {
      headers.append(name, HeaderValue::from_str(&value).unwrap());
    }

    Box::pin(async move {
      Ok(Incoming {
        status: found.status,
        status_text: String::new(),
        headers,
        body: futures_util::stream::iter(vec![Ok(Bytes::from(found.body))]).boxed(),
      })
    })
  }
}

/// Every name resolves to one public address.
pub struct Public;

impl Lookup for Public {
  fn lookup<'a>(
    &'a self,
    _: &'a str,
  ) -> Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    Box::pin(async { Ok(vec!["93.184.216.34".parse().unwrap()]) })
  }
}

/// The check every test reads through: public names on a standing clock.
pub fn check(clock: Arc<StandingClock>) -> Check {
  Check {
    allow_private: false,
    lookup: Arc::new(Public),
    clock,
  }
}

/// A browser answering every page with the one it was handed, counting its calls and whether it was closed.
pub struct FakeBrowser {
  pub document: BrowserDocument,
  pub calls: Mutex<Vec<(String, bool)>>,
  pub closed: Mutex<bool>,
}

impl FakeBrowser {
  pub fn new(document: BrowserDocument) -> Arc<Self> {
    Arc::new(Self {
      document,
      calls: Mutex::new(Vec::new()),
      closed: Mutex::new(false),
    })
  }

  pub fn calls(&self) -> usize {
    self.calls.lock().unwrap().len()
  }

  pub fn closed(&self) -> bool {
    *self.closed.lock().unwrap()
  }
}

impl BrowserFetcher for FakeBrowser {
  fn fetch(
    &self,
    url: String,
    options: FetchOptions,
  ) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send + '_>> {
    self.calls.lock().unwrap().push((url, options.arrived.is_some()));

    let document = self.document.clone();

    Box::pin(async move { Ok(document) })
  }

  fn close(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
    *self.closed.lock().unwrap() = true;
    Box::pin(async {})
  }
}

/// A fixture file of this crate's, as text.
pub fn fixture(path: &str) -> String {
  std::fs::read_to_string(format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// A small PDF: one page per string, each line set in Helvetica, and an empty string a page with no text
/// layer, as a scan's page is.
pub fn pdf_of(pages: &[&str]) -> Vec<u8> {
  let mut objects: Vec<String> = vec![
    "<< /Type /Catalog /Pages 2 0 R >>".into(),
    String::new(),
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
  ];
  let mut kids = Vec::new();

  for page in pages {
    let content = if page.is_empty() {
      String::new()
    } else {
      let lines: Vec<String> = page
        .split('\n')
        .map(|line| format!("({}) '", line.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")))
        .collect();

      format!("BT /F1 12 Tf 72 720 Td 14 TL {} ET", lines.join(" "))
    };

    objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));

    let content_ref = objects.len();

    objects.push(format!(
      "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_ref} 0 R >>"
    ));
    kids.push(objects.len());
  }

  objects[1] = format!(
    "<< /Type /Pages /Kids [{}] /Count {} >>",
    kids.iter().map(|kid| format!("{kid} 0 R")).collect::<Vec<_>>().join(" "),
    kids.len()
  );

  let mut body = "%PDF-1.4\n".to_owned();
  let mut offsets = Vec::new();

  for (index, object) in objects.iter().enumerate() {
    offsets.push(body.len());
    body.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
  }

  let xref = body.len();

  body.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
  for offset in offsets {
    body.push_str(&format!("{offset:010} 00000 n \n"));
  }
  body.push_str(&format!(
    "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
    objects.len() + 1
  ));
  body.into_bytes()
}
