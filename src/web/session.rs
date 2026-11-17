//! One investigation's web reading (docs/web-access.md, 「One session, one investigation」): the direct
//! client's cookie jar, the pacing every request takes, and the real browser a page falls back to, kept for
//! the session and ended with it. A page comes back sliced, with where the rest starts.

use std::sync::Arc;

use url::Url;

use crate::web::address::{Check, host_of, web_url};
use crate::web::browser::{BrowserDocument, BrowserFetcher, FetchOptions, Pace, UrlGuard};
use crate::web::classify::{Disposition, Fetched, classify};
use crate::web::client::{Direct, DirectBytes, Transport, decode};
use crate::web::clock::Clock;
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::jar::Jar;
use crate::web::pacing::{Pacing, wait_for_host};
use crate::web::pdf::{is_pdf, pdf_text};

// The slice a fetch returns, the length it cuts to and the answer it writes are the web's forms, shared with
// the site contract.
pub use crate::forms::fetch::MAX_FETCH_CHARS;
pub use crate::forms::slice::{DEFAULT_FETCH_CHARS, FetchSlice, render_slice, slice_content};

/// A GitHub file page shows a rendered screen around the file; what an agent wants is the file.
pub fn raw_url_for(url: &str) -> String {
  let Ok(parsed) = Url::parse(url) else {
    return url.to_owned();
  };
  let segments: Vec<&str> = parsed.path().trim_start_matches('/').split('/').collect();

  match parsed.host_str() {
    Some("github.com")
      if segments.len() >= 5
        && segments[2] == "blob"
        && segments[4..].iter().all(|part| !part.is_empty()) =>
    {
      format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        segments[0],
        segments[1],
        segments[3],
        segments[4..].join("/")
      )
    }
    Some("gist.github.com")
      if (segments.len() == 2 || (segments.len() == 3 && segments[2].is_empty()))
        && !segments[1].is_empty()
        && segments[1].chars().all(|c| c.is_ascii_hexdigit()) =>
    {
      format!("https://gist.githubusercontent.com/{}/{}/raw", segments[0], segments[1])
    }
    _ => url.to_owned(),
  }
}

// The page as the browser rendered it, with the links its text does not already name.
// A page host's turn in its pacing queue: every request to a host, and every top-level navigation of a browser,
// takes one.
async fn wait_for_page(
  store: &dyn Pacing,
  clock: &dyn Clock,
  interval_ms: u64,
  url: &str,
) -> WebResult<()> {
  let host = web_url(url).map(|url| host_of(&url)).unwrap_or_default();

  wait_for_host(store, &format!("page:{host}"), interval_ms, clock).await?;
  Ok(())
}

fn browser_text(document: &BrowserDocument) -> String {
  let text = crate::web::classify::tidy(&document.text.replace('\u{a0}', " "));
  let links: Vec<String> = document
    .links
    .iter()
    .filter_map(|link| {
      let label = link.text.split_whitespace().collect::<Vec<_>>().join(" ");

      (!label.is_empty() && !text.contains(&format!("({})", link.url)))
        .then(|| format!("{label} ({})", link.url))
    })
    .collect();

  if links.is_empty() { text } else { format!("{text}\n\nLINKS:\n{}", links.join("\n")) }
}

/// How a page is read.
#[derive(Clone, Debug, Default)]
pub struct FetchPage {
  pub offset: Option<usize>,
  pub max_chars: Option<usize>,
  pub raw: bool,
  /// Whether the real browser may read a page the direct client could not.
  pub browser_fallback: bool,
  /// Read the page in the real browser without a direct attempt first.
  pub browser_first: bool,
  /// Read a PDF in its layout rather than in reading order.
  pub pdf_layout: bool,
}

/// What a session reads with.
pub struct SessionParts {
  pub store: Arc<dyn Pacing>,
  pub transport: Arc<dyn Transport>,
  pub check: Check,
  /// The pacing between two requests to one host.
  pub host_interval_ms: u64,
  /// How the browser a session falls back to is made, under the session's guard; the machine's browser where
  /// none is named, a test's stand-in otherwise.
  pub browsers: Option<BrowserFactory>,
}

/// What makes a browser under a guard.
pub type BrowserFactory = Arc<dyn Fn(UrlGuard) -> Arc<dyn BrowserFetcher> + Send + Sync>;

pub struct WebSession {
  parts: SessionParts,
  jar: tokio::sync::Mutex<Jar>,
  browser: tokio::sync::Mutex<Option<Arc<dyn BrowserFetcher>>>,
}

impl WebSession {
  pub fn new(parts: SessionParts) -> Self {
    Self {
      parts,
      jar: tokio::sync::Mutex::new(Jar::default()),
      browser: tokio::sync::Mutex::new(None),
    }
  }

  pub fn store(&self) -> &Arc<dyn Pacing> {
    &self.parts.store
  }

  pub fn check(&self) -> &Check {
    &self.parts.check
  }

  pub fn transport(&self) -> &Arc<dyn Transport> {
    &self.parts.transport
  }

  /// The check a browser request passes, answering with the address it passed at.
  pub fn guard(&self) -> UrlGuard {
    self.parts.check.guard()
  }

  async fn browser(&self) -> Arc<dyn BrowserFetcher> {
    let mut held = self.browser.lock().await;

    held.get_or_insert_with(|| self.own_browser()).clone()
  }

  /// A fresh browser of its own, under the same guard.
  fn own_browser(&self) -> Arc<dyn BrowserFetcher> {
    match &self.parts.browsers {
      Some(make) => make(self.guard()),
      None => crate::web::browser::on_this_machine(self.guard(), self.page_pace()),
    }
  }

  // A page host's turn in its pacing queue, as a browser's second read of a page waits for it.
  fn page_pace(&self) -> Pace {
    let (store, clock, interval_ms) =
      (self.parts.store.clone(), self.parts.check.clock.clone(), self.parts.host_interval_ms);

    Arc::new(move |url: String| {
      let (store, clock) = (store.clone(), clock.clone());

      Box::pin(async move { wait_for_page(&*store, &*clock, interval_ms, &url).await })
    })
  }

  async fn wait_for_page_host(&self, url: &str) -> WebResult<()> {
    wait_for_page(&*self.parts.store, &*self.parts.check.clock, self.parts.host_interval_ms, url).await
  }

  fn direct(&self) -> Direct<'_> {
    let mut direct = Direct::new(&*self.parts.store, &self.jar, &self.parts.check, &*self.parts.transport);

    direct.interval_ms = self.parts.host_interval_ms;
    direct
  }

  /// One page as the fetch tool reads it: the direct client first, the real browser after a block where
  /// that is allowed, a PDF as its text.
  pub async fn fetch_page(&self, url: &str, options: &FetchPage) -> WebResult<FetchSlice> {
    let target = raw_url_for(url);
    let clock = &self.parts.check.clock;
    let max_chars = options.max_chars.unwrap_or(DEFAULT_FETCH_CHARS);

    if options.browser_first && options.browser_fallback {
      self.parts.check.resolve(&web_url(&target)?).await?;
      self.wait_for_page_host(&target).await?;

      return Box::pin(self.rendered(&target, options, None)).await;
    }

    let fetched = self.direct().bytes(&target).await?;

    // A PDF is answered with its text, never with its bytes decoded as if they were text.
    if (200..300).contains(&fetched.status)
      && !fetched.bytes.is_empty()
      && is_pdf(&fetched.content_type, &fetched.bytes)
    {
      return self.pdf_slice(&fetched, options).await;
    }

    let body = decode(&fetched.bytes, &fetched.raw_content_type);
    let classified = classify(&Fetched {
      status: fetched.status,
      content_type: &fetched.content_type,
      body: &body,
      requested_url: Some(&target),
      final_url: Some(&fetched.url),
      ..Default::default()
    });
    let content_type = |fallback: &str| {
      if fetched.content_type.is_empty() {
        fallback.to_owned()
      } else {
        fetched.content_type.clone()
      }
    };

    match classified.disposition {
      Disposition::NoContent => {
        let said = format!(
          "NO CONTENT: the page answered {} with no content.",
          classified.reason.unwrap_or_default()
        );

        return Ok(slice_content(&fetched.url, &content_type("text/plain"), &said, 0, max_chars).read_as(
          fetched.status,
          "direct",
          false,
        ));
      }
      Disposition::Content => {
        let text = if classified.is_html && !options.raw { classified.text } else { body };

        return Ok(
          slice_content(
            &fetched.url,
            &content_type(if classified.is_html { "text/html" } else { "text/plain" }),
            &text,
            options.offset.unwrap_or(0),
            max_chars,
          )
          .read_as(fetched.status, "direct", fetched.truncated),
        );
      }
      Disposition::HttpError => {
        let message = format!("{} {}", classified.reason.unwrap_or_default(), fetched.status_text)
          .trim()
          .to_owned();

        return Err(
          WebError::new(format!("{message} for {}", fetched.url), Failure::Http)
            .at(Some(fetched.status), Some(&fetched.url)),
        );
      }
      Disposition::Blocked | Disposition::EmptyShell => {}
    }

    let reason = classified.reason.unwrap_or_else(|| "Blocked page".into());

    if !options.browser_fallback {
      return Err(
        WebError::new(format!("{reason} for {}", fetched.url), Failure::Blocked)
          .at(Some(fetched.status), Some(&fetched.url)),
      );
    }

    let page_key = format!("page:{}", web_url(&fetched.url).map(|url| host_of(&url)).unwrap_or_default());

    if fetched.status == 429 {
      let now = clock.now();

      self
        .parts
        .store
        .defer(&page_key, now + fetched.retry_after_ms.unwrap_or(5_000), now)
        .await
        .map_err(crate::web::pacing::store_failure)?;
    }

    // A site's own Retry-After is waited out, however long it is, before the browser asks again.
    wait_for_host(&*self.parts.store, &page_key, self.parts.host_interval_ms, &**clock).await?;

    // The browser opens the address that was asked for and follows its redirects itself: a page that sent the
    // direct client to its login page may serve a browser the page itself. What it fails with says that
    // both roads were tried.
    Box::pin(self.rendered(&target, options, Some((reason, fetched.status))))
      .await
      .map_err(WebError::after_chrome)
  }

  /// A page opened in the real browser and handed back as it rendered, unclassified: the road a search
  /// engine's recipe may send it by. `fresh` opens it in a browser of its own, ended once the page is read.
  pub async fn render(&self, url: &str, fresh: bool, options: FetchOptions) -> WebResult<BrowserDocument> {
    self.parts.check.resolve(&web_url(url)?).await?;
    self.wait_for_page_host(url).await?;

    if !fresh {
      return self.browser().await.fetch(url.to_owned(), options).await;
    }

    let browser = self.own_browser();
    let read = browser.fetch(url.to_owned(), options).await;

    browser.close().await;
    read
  }

  // The page as the real browser renders it: after a classified direct block, or first of all under the
  // browser-first strategy, where there is no direct attempt to name.
  async fn rendered(
    &self,
    url: &str,
    options: &FetchPage,
    direct: Option<(String, u16)>,
  ) -> WebResult<FetchSlice> {
    let rendered = match self.browser().await.fetch(url.to_owned(), FetchOptions::default()).await {
      Ok(rendered) => rendered,
      Err(error) if matches!(error.kind, Failure::Blocked | Failure::UnsafeAddress) => {
        return Err(error);
      }
      Err(error) => {
        let (first, status) = match &direct {
          Some((reason, status)) => (format!("{reason}; real browser fallback failed"), Some(*status)),
          None => ("Browser-first fetch; the real browser failed".to_owned(), None),
        };

        return Err(
          WebError::new(format!("{first}: {}", error.message), Failure::BrowserUnavailable)
            .at(status, Some(url)),
        );
      }
    };

    // A browser shows a PDF in its viewer, whose page holds no text: the file is the direct client's to read,
    // unless the direct client was what the site refused.
    if is_pdf(&rendered.content_type, b"") {
      if let Some((reason, status)) = direct {
        return Err(
          WebError::new(
            format!("{reason}; the real browser was served a PDF, whose viewer holds no text"),
            Failure::Blocked,
          )
          .at(Some(status), Some(url)),
        );
      }

      let again = FetchPage {
        browser_first: false,
        browser_fallback: false,
        ..options.clone()
      };

      return Box::pin(self.fetch_page(url, &again)).await;
    }

    let classified = classify(&Fetched {
      status: rendered.status,
      content_type: &rendered.content_type,
      body: &rendered.html,
      title: Some(&rendered.title),
      visible_text: Some(&rendered.text),
      requested_url: Some(url),
      final_url: Some(&rendered.url),
    });

    if classified.disposition != Disposition::Content {
      let what = classified
        .reason
        .unwrap_or_else(|| format!("{:?}", classified.disposition).to_lowercase());

      return Err(
        WebError::new(
          format!(
            "The real browser still reached {what} at {}; no CAPTCHA or challenge was attempted",
            rendered.url
          ),
          Failure::Blocked,
        )
        .at(Some(rendered.status), Some(&rendered.url)),
      );
    }

    let text = if options.raw { rendered.html.clone() } else { browser_text(&rendered) };
    let content_type = if rendered.content_type.is_empty() {
      "text/html".to_owned()
    } else {
      rendered.content_type.clone()
    };

    Ok(
      slice_content(
        &rendered.url,
        &content_type,
        &text,
        options.offset.unwrap_or(0),
        options.max_chars.unwrap_or(DEFAULT_FETCH_CHARS),
      )
      .read_as(rendered.status, "browser", rendered.truncated),
    )
  }

  async fn pdf_slice(&self, fetched: &DirectBytes, options: &FetchPage) -> WebResult<FetchSlice> {
    let text = pdf_text(&fetched.bytes, &fetched.url, fetched.truncated, options.pdf_layout).await?;
    let content_type = if fetched.content_type.is_empty() {
      "application/pdf"
    } else {
      fetched.content_type.as_str()
    };

    Ok(
      slice_content(
        &fetched.url,
        content_type,
        &text,
        options.offset.unwrap_or(0),
        options.max_chars.unwrap_or(DEFAULT_FETCH_CHARS),
      )
      .read_as(fetched.status, "direct", fetched.truncated),
    )
  }

  /// The session's cookies dropped and the browser it made ended.
  pub async fn close(&self) {
    self.jar.lock().await.clear();

    let browser = self.browser.lock().await.take();

    if let Some(browser) = browser {
      browser.close().await;
    }
  }
}
