use std::sync::{Arc, Mutex};

use bmo_explorer::forms::Failure;
use bmo_explorer::web::address::Check;
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::session::{SessionParts, WebSession};

use bmo_explorer::site::contract::Reach;
use bmo_explorer::web_helper::run::{Research, WebRun};

// A reading on the trusted road test fixtures take (docs/web-access.md, 「Address checks」), or on the road every
// other read takes.
fn reading(trusted: bool) -> WebRun {
  let clock = StandingClock::at(1_000);

  WebRun::with(
    Research::default(),
    WebSession::new(SessionParts {
      store: Arc::new(MemoryPacing::default()),
      transport: bmo_explorer::web::client::network(),
      check: Check {
        allow_private: trusted,
        ..Check::default()
      },
      host_interval_ms: 0,
      browsers: None,
    }),
    clock,
  )
}

fn run() -> WebRun {
  reading(true)
}

#[tokio::test]
async fn a_page_and_a_head_refuse_a_private_address_before_they_connect() {
  let reading = reading(false);
  let page = reading.page("http://10.1.2.3/", None).await.unwrap_err();
  let head = reading.head("http://192.168.1.1/file.zip").await.unwrap_err();

  assert_eq!((page.kind, head.kind), (Failure::UnsafeAddress, Failure::UnsafeAddress));
}

// Carried from packages/web/market-tools.test.ts: asks without the body as a browser would, and answers a refusal's status and headers instead of failing
#[tokio::test]
async fn a_head_asks_without_the_body_and_answers_a_refusals_status_and_headers() {
  let methods = Arc::new(Mutex::new(Vec::new()));
  let seen = methods.clone();
  let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
    let seen = seen.clone();

    async move {
      seen.lock().unwrap().push(request.method().to_string());
      (
        axum::http::StatusCode::FORBIDDEN,
        [("content-type", "application/xml"), ("x-cache", "Error from cloudfront")],
      )
    }
  });
  let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
  let address = format!("http://{}/1/missing.zip", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

  let head = run().head(&address).await.unwrap();

  assert_eq!(*methods.lock().unwrap(), ["HEAD"]);
  assert_eq!((head.url.as_str(), head.status), (address.as_str(), 403));
  assert_eq!(head.headers["content-type"], "application/xml");
  assert_eq!(head.headers["x-cache"], "Error from cloudfront");
}
