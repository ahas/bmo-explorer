use bmo_explorer::web::address::Check;
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::jar::Jar;
use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use url::Url;

use bmo_explorer::web::client::*;
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::pacing::MemoryPacing;

/// One answer of the script: its status, its headers and its body.
pub type ScriptedAnswer = (u16, Vec<(&'static str, String)>, Vec<u8>);

/// A transport that answers from a script and remembers what it was asked.
pub struct Scripted {
  pub answers: Mutex<Vec<ScriptedAnswer>>,
  pub asked: Mutex<Vec<(Url, Option<IpAddr>, HeaderMap)>>,
}

impl Transport for Scripted {
  fn send(&self, outgoing: Outgoing) -> bmo_explorer::web::pacing::Boxed<'_, Incoming> {
    self
      .asked
      .lock()
      .unwrap()
      .push((outgoing.url.clone(), outgoing.connect, outgoing.headers.clone()));

    let (status, headers, body) = self.answers.lock().unwrap().remove(0);
    let mut map = HeaderMap::new();

    for (name, value) in headers {
      map.append(name, HeaderValue::from_str(&value).unwrap());
    }

    Box::pin(async move {
      Ok(Incoming {
        status,
        status_text: String::new(),
        headers: map,
        body: futures_util::stream::iter(vec![Ok(Bytes::from(body))]).boxed(),
      })
    })
  }
}

struct Answers(Vec<(&'static str, IpAddr)>);

impl bmo_explorer::web::address::Lookup for Answers {
  fn lookup<'a>(
    &'a self,
    host: &'a str,
  ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<IpAddr>, String>> + Send + 'a>> {
    let found: Vec<IpAddr> =
      self.0.iter().filter(|(name, _)| *name == host).map(|(_, address)| *address).collect();

    Box::pin(async move { Ok(found) })
  }
}

fn check(names: Vec<(&'static str, IpAddr)>) -> Check {
  Check {
    allow_private: false,
    lookup: Arc::new(Answers(names)),
    clock: StandingClock::at(1_000_000),
  }
}

#[tokio::test]
async fn follows_redirects_itself_keeps_same_host_cookies_and_drops_sensitive_headers_across_origins() {
  let transport = Scripted {
    answers: Mutex::new(vec![
      (302, vec![("location", "/next".into()), ("set-cookie", "sid=1; Path=/".into())], vec![]),
      (302, vec![("location", "https://other.example/land".into())], vec![]),
      (200, vec![("content-type", "text/html; charset=utf-8".into())], b"<p>done</p>".to_vec()),
    ]),
    asked: Mutex::new(Vec::new()),
  };
  let public: IpAddr = "93.184.216.34".parse().unwrap();
  let check = check(vec![("shop.example", public), ("other.example", "93.184.216.35".parse().unwrap())]);
  let store = MemoryPacing::default();
  let jar = tokio::sync::Mutex::new(Jar::default());
  let mut direct = Direct::new(&store, &jar, &check, &transport);

  direct.headers.push(("authorization".into(), "secret".into()));

  let read = direct.document("https://shop.example/start").await.unwrap();
  let asked = transport.asked.lock().unwrap();

  assert_eq!(read.url, "https://other.example/land");
  assert_eq!(read.body, "<p>done</p>");
  assert_eq!(asked[0].1, Some(public));
  assert_eq!(asked[1].2.get("cookie").unwrap(), "sid=1");
  assert_eq!(asked[1].2.get("referer").unwrap(), "https://shop.example/start");
  assert!(asked[2].2.get("authorization").is_none());
  assert!(asked[2].2.get("cookie").is_none());
  assert_eq!(asked[2].2.get("referer").unwrap(), "https://shop.example/");
  assert_eq!(asked[2].2.get("sec-fetch-site").unwrap(), "cross-site");
}

#[tokio::test]
async fn a_redirect_to_a_private_address_is_refused_without_connecting() {
  let transport = Scripted {
    answers: Mutex::new(vec![(
      302,
      vec![("location", format!("http://{}.{}.{}.1/admin", 10, 0, 0))],
      vec![],
    )]),
    asked: Mutex::new(Vec::new()),
  };
  let check = check(vec![("shop.example", "93.184.216.34".parse().unwrap())]);
  let store = MemoryPacing::default();
  let jar = tokio::sync::Mutex::new(Jar::default());
  let failed = Direct::new(&store, &jar, &check, &transport)
    .bytes("https://shop.example/")
    .await
    .unwrap_err();

  assert_eq!(failed.kind, Failure::UnsafeAddress);
  assert_eq!(transport.asked.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn the_body_stops_at_the_cap_and_says_it_was_cut() {
  let transport = Scripted {
    answers: Mutex::new(vec![(200, vec![], vec![b'x'; 100])]),
    asked: Mutex::new(Vec::new()),
  };
  let check = check(vec![("shop.example", "93.184.216.34".parse().unwrap())]);
  let store = MemoryPacing::default();
  let jar = tokio::sync::Mutex::new(Jar::default());
  let mut direct = Direct::new(&store, &jar, &check, &transport);

  direct.byte_cap = 10;

  let read = direct.bytes("https://shop.example/").await.unwrap();

  assert_eq!(read.bytes.len(), 10);
  assert!(read.truncated);
}

#[test]
fn a_korean_page_is_read_in_the_charset_it_declared() {
  let (euc, _, _) = encoding_rs::EUC_KR.encode("한글 페이지");

  assert_eq!(decode(&euc, "text/html; charset=euc-kr"), "한글 페이지");
  assert_eq!(decode(&euc, "text/html; charset=ks_c_5601-1987"), "한글 페이지");

  let mut page = b"<html><head><meta charset=\"euc-kr\"></head><body>".to_vec();

  page.extend_from_slice(&euc);
  assert!(decode(&page, "text/html").ends_with("한글 페이지"));
  assert_eq!(decode("plain".as_bytes(), "text/plain"), "plain");
}
