//! The roads a site tool reaches its site by, as one record's web reading carries them (docs/site-contract.md,
//! 「What a tool is given」): a page fetched as a browser asks for it, a page the real Chrome rendered, the
//! search engines, a document read as `web_fetch` reads it, and a head request.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use reqwest::redirect::Policy;

use crate::site::contract::{Arrival, PageHead, PageRequest, Reach, Rendered, Slice};
use crate::web::address::{host_of, web_url};
use crate::web::chrome::{Arrived, FetchOptions};
use crate::web::client::{DIRECT_HOST_INTERVAL_MS, MAX_REDIRECTS};
use crate::web::pacing::wait_for_host;
use crate::web::search::{Search, SearchResults, SearchRoads, render_through, search_web};
use crate::web::session::render_slice;
use crate::web::{Failure, WebError, WebResult};
use url::Url;

use crate::web_helper::run::WebRun;

const PAGE_TIMEOUT: Duration = Duration::from_secs(20);

fn plain_failure(why: impl std::fmt::Display) -> WebError {
  WebError::new(why.to_string(), Failure::Transport)
}

// A client for one hop, connecting to the address the hop's check passed rather than resolving the name again.
fn hop_client(url: &Url, address: Option<std::net::IpAddr>) -> WebResult<reqwest::Client> {
  let mut builder = reqwest::Client::builder()
    .timeout(PAGE_TIMEOUT)
    .redirect(Policy::none())
    .gzip(true)
    .brotli(true)
    .deflate(true)
    .zstd(true);

  if let (Some(address), Some(port)) = (address, url.port_or_known_default()) {
    builder = builder.resolve(&host_of(url), SocketAddr::new(address, port));
  }

  builder.build().map_err(plain_failure)
}

impl WebRun {
  /// One request of a road, sent under the reading's checks (docs/web-access.md, 「Address checks」 and
  /// 「Pacing」): each hop's address checked and the connection made to the address that passed, each hop paced
  /// per host, and the redirects followed one hop at a time, so a road reaches no place the reading refuses.
  /// The request keeps the road's own shape: the browser's name and language, and the tool's own headers.
  async fn send_checked(
    &self,
    method: Method,
    url: &str,
    body: Option<String>,
    headers: &[(String, String)],
  ) -> WebResult<reqwest::Response> {
    let check = self.session.check();
    let deadline = self.guard.deadline().unwrap_or_else(|| {
      crate::web::clock::now_ms() + u64::try_from(PAGE_TIMEOUT.as_millis()).unwrap_or(u64::MAX)
    });
    let interval = self.research.host_interval_ms.unwrap_or(DIRECT_HOST_INTERVAL_MS);
    let mut current = web_url(url)?;
    let mut method = method;
    let mut body = body;

    for _ in 0..=MAX_REDIRECTS {
      let target = check.resolve(&current, Some(deadline)).await?;

      wait_for_host(
        &**self.session.store(),
        &format!("page:{}", host_of(&current)),
        interval,
        deadline,
        &*check.clock,
      )
      .await?;

      let mut asked = hop_client(&current, target.address)?
        .request(method.clone(), current.clone())
        .header("user-agent", crate::web::identity::DEFAULT.user_agent)
        .header("accept-language", "ko-KR,ko;q=0.9");

      for (name, value) in headers {
        asked = asked.header(name.as_str(), value.as_str());
      }
      if let Some(body) = &body {
        asked = asked.body(body.clone());
      }

      let answer = asked.send().await.map_err(plain_failure)?;
      let status = answer.status();
      let location = answer.headers().get("location").and_then(|value| value.to_str().ok());

      match location {
        Some(location) if status.is_redirection() => {
          current = current.join(location).map_err(|_| {
            WebError::new(format!("Invalid web URL: {location}"), Failure::UnsafeAddress)
              .with_url(current.as_str())
          })?;
          // A 307 or a 308 asks again as it was asked; any other redirect asks for the new address plainly.
          if !matches!(status.as_u16(), 307 | 308) {
            method = Method::GET;
            body = None;
          }
        }
        _ => return Ok(answer),
      }
    }

    Err(WebError::new(format!("Too many redirects for {url}"), Failure::Http).with_url(url))
  }
}

impl Reach for WebRun {
  fn page<'a>(
    &'a self,
    url: &'a str,
    request: Option<PageRequest>,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    Box::pin(async move {
      let request = request.unwrap_or_default();
      let method = if request.body.is_some() { Method::POST } else { Method::GET };
      let answer = self.send_checked(method, url, request.body.clone(), &request.headers).await?;
      let status = answer.status();

      if !status.is_success() {
        let host = Url::parse(url)
          .ok()
          .and_then(|url| url.host_str().map(str::to_owned))
          .unwrap_or_default();

        return Err(
          WebError::new(format!("{host} answered HTTP {}", status.as_u16()), Failure::Http)
            .at(Some(status.as_u16()), Some(url)),
        );
      }

      answer.text().await.map_err(plain_failure)
    })
  }

  fn rendered<'a>(
    &'a self,
    url: &'a str,
    fresh: bool,
    arrival: Option<Arrival>,
  ) -> Pin<Box<dyn Future<Output = WebResult<Rendered>> + Send + 'a>> {
    Box::pin(async move {
      if self.research.direct_only() {
        let host = Url::parse(url)
          .ok()
          .and_then(|url| url.host_str().map(str::to_owned))
          .unwrap_or_default();

        return Err(WebError::new(
          format!("{host} is served only to a real browser, and this run reads the web directly only"),
          Failure::Blocked,
        ));
      }

      let deadline = self
        .guard
        .deadline()
        .unwrap_or_else(|| crate::web::clock::now_ms() + crate::web::session::FETCH_TIMEOUT_MS);
      let arrived =
        arrival.map(|arrival| -> Arrived { Arc::new(move |at, title| arrival.arrived(at, title)) });
      let page = self.session.render(url, deadline, fresh, FetchOptions { arrived }).await?;

      Ok(Rendered {
        url: page.url,
        status: page.status,
        html: page.html,
        text: page.text,
      })
    })
  }

  fn engine<'a>(
    &'a self,
    query: &'a str,
    count: usize,
  ) -> Pin<Box<dyn Future<Output = WebResult<SearchResults>> + Send + 'a>> {
    Box::pin(async move { self.search(query, Some(count), Some("ko"), None).await })
  }

  fn document<'a>(
    &'a self,
    url: &'a str,
    slice: Slice,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    Box::pin(async move {
      Ok(render_slice(
        &self.read_page(url, slice.offset, slice.max_chars, false, slice.layout).await?,
      ))
    })
  }

  fn head<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = WebResult<PageHead>> + Send + 'a>> {
    Box::pin(async move {
      let answer = self.send_checked(Method::HEAD, url, None, &[]).await?;

      Ok(PageHead {
        url: answer.url().to_string(),
        status: answer.status().as_u16(),
        headers: answer
          .headers()
          .iter()
          .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| (name.as_str().to_lowercase(), value.to_owned()))
          })
          .collect(),
      })
    })
  }
}

impl WebRun {
  /// One search through the run's roads: the shared store, its deadline and engine gap, and the real browser
  /// an engine's recipe may send it by — unless the run reads direct only.
  pub async fn search(
    &self,
    query: &str,
    count: Option<usize>,
    language: Option<&str>,
    category: Option<&str>,
  ) -> WebResult<SearchResults> {
    let render = (!self.research.direct_only()).then(|| render_through(self.session.clone()));

    search_web(
      &Search {
        query,
        count,
        language,
        category,
        deadline_ms: self.guard.deadline(),
        engine_interval_ms: self.research.engine_interval_ms,
        engines: &[],
      },
      &SearchRoads {
        store: &**self.session.store(),
        check: self.session.check(),
        transport: &**self.session.transport(),
        render,
      },
    )
    .await
  }
}
