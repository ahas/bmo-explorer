//! The site contract (docs/site-contract.md): what a site tool declares of itself and runs, and what it is given to
//! reach its site with. The tools' crate writes the tools against it and serves them; the reach a test reads
//! through stands here beside it, so a tool is tested against the same contract it runs under.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
// The web types the contract's signatures name, and the slice a tool cuts its answer to, said here so a
// placed tool names the contract alone.
pub use crate::forms::{
  DEFAULT_FETCH_CHARS, Failure, MAX_FETCH_CHARS, SearchHit, SearchResults, WebError, WebResult,
  slice_content,
};

/// A page as the real browser rendered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rendered {
  pub url: String,
  pub status: u16,
  pub html: String,
  pub text: String,
}

/// What a server answered about one address asked for without its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHead {
  pub url: String,
  pub status: u16,
  /// Its headers by their lower-cased names.
  pub headers: BTreeMap<String, String>,
}

/// A read of a page other than a plain one: a body the site's own page posts there, and headers laid over a
/// browser's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
  pub body: Option<String>,
  pub headers: Vec<(String, String)>,
}

/// A slice of a page read the way `web_fetch` reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slice {
  pub offset: Option<usize>,
  pub max_chars: Option<usize>,
  /// A PDF read in its layout, keeping a table's rows apart.
  pub layout: bool,
}

/// When a page opened in the real browser has arrived, for a site whose own browser check moves on to the page by
/// itself: a page stands on the check while its address's path or its title begins as the check's does, and an
/// untitled page has not arrived where `titled` says so, unless its address carries one of `untitled_at`'s query
/// pairs. It is data rather than a test, since a placed tool hands it to the record's web helper on its socket.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrival {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub check_paths: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub check_titles: Vec<String>,
  #[serde(default)]
  pub titled: bool,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub untitled_at: Vec<(String, String)>,
}

impl Arrival {
  /// Whether the browser standing at `url` under `title` stands on the page rather than on the site's check.
  pub fn arrived(&self, url: &str, title: &str) -> bool {
    let address = Url::parse(url).ok();

    if address
      .as_ref()
      .is_some_and(|address| self.check_paths.iter().any(|path| address.path().starts_with(path.as_str())))
      || self.check_titles.iter().any(|check| title.starts_with(check.as_str()))
    {
      return false;
    }

    !self.titled
      || !title.is_empty()
      || address.is_some_and(|address| {
        address.query_pairs().any(|(key, value)| {
          self
            .untitled_at
            .iter()
            .any(|(named, valued)| key == named.as_str() && value == valued.as_str())
        })
      })
  }
}

/// What a site tool is given to reach its site with. A tool never opens a connection itself: the run that
/// serves it brings the reading, so one place holds the pacing and the policy every outward
/// request of this machine goes through.
pub trait Reach: Send + Sync {
  /// One fixed public page or endpoint, fetched as a browser would; `request` posts a body to it, or asks for
  /// it with headers of its own.
  fn page<'a>(
    &'a self,
    url: &'a str,
    request: Option<PageRequest>,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>>;
  /// One page opened in the real browser; `fresh` in a browser of its own, `arrival` for a site whose own
  /// browser check moves on to the page by itself.
  fn rendered<'a>(
    &'a self,
    url: &'a str,
    fresh: bool,
    arrival: Option<Arrival>,
  ) -> Pin<Box<dyn Future<Output = WebResult<Rendered>> + Send + 'a>>;
  /// The search engines, for a site that refuses this machine's readers outright.
  fn engine<'a>(
    &'a self,
    query: &'a str,
    count: usize,
  ) -> Pin<Box<dyn Future<Output = WebResult<SearchResults>> + Send + 'a>>;
  /// One page read and sliced the way `web_fetch` reads it, and answered as `web_fetch` answers.
  fn document<'a>(
    &'a self,
    url: &'a str,
    slice: Slice,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>>;
  /// What a server answers about one address asked for without its body, whatever the status.
  fn head<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = WebResult<PageHead>> + Send + 'a>>;
}

/// A site tool, as its author writes it.
pub trait SiteTool: Send + Sync + 'static {
  /// The name the tool is called by, which is the name an agent sees.
  const NAME: &'static str;
  const TITLE: &'static str;
  /// What it brings back, in one line, and the site it reads it from: what the catalogue shows.
  const WHAT: &'static str;
  const SITE: &'static str;
  /// When the tool was made, as a date: how old the reading is, the first question when a site has changed
  /// shape under it.
  const SINCE: &'static str;
  const DESCRIPTION: &'static str;
  /// Whether it reads through the search engines rather than a page of its own, and is held to the same
  /// refusals the plain web search is.
  const ENGINE: bool = false;
  /// The forms of the page addresses it claims, where it claims any.
  const FORMS: Option<&'static str> = None;
  /// The hosts it claims pages on that its forms name no host whole for: the parent of a pattern of
  /// subdomains written as `<account>.example.com`, or a second host abbreviated beside the first.
  const ALSO_CLAIMS: &'static [&'static str] = &[];

  type Input: DeserializeOwned + JsonSchema + Send;

  /// Its arguments for a page address it claims, where the address is one of its forms.
  fn claim(_url: &Url) -> Option<Value> {
    None
  }

  fn run(&self, input: Self::Input, reach: &dyn Reach) -> impl Future<Output = WebResult<String>> + Send;
}

/// The HTTP status a failed read was answered with, where it was one: the status the reading kept, or the one
/// its message names.
pub fn status_of(error: &WebError) -> Option<u16> {
  static STATUS: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"HTTP (\d{3})").expect("a fixed pattern"));

  error
    .status
    .or_else(|| STATUS.captures(&error.message).and_then(|found| found[1].parse().ok()))
}

/// The stand-in reach a site tool's test reads through: every page answered from fixtures by address, and
/// nothing else reached. A page opened in the browser may be answered by several pages in turn, the last of
/// them again once the others are spent.
#[derive(Default)]
pub struct Fixtures {
  pub pages: BTreeMap<String, String>,
  /// Pages a plain request is refused at, with what the refusal says.
  pub refusals: BTreeMap<String, String>,
  /// Refusals a page answers first, one to each read of it, before it answers as the rest of the fixtures say.
  pub first_refusals: std::sync::Mutex<BTreeMap<String, Vec<String>>>,
  /// Answers to a body posted to an address, by the address and the body.
  pub answers: BTreeMap<(String, String), String>,
  /// What a plain request for any page the fixtures do not hold is refused with, where a test says so.
  pub otherwise: Option<String>,
  pub rendered: std::sync::Mutex<BTreeMap<String, Vec<Rendered>>>,
  pub documents: BTreeMap<String, String>,
  pub heads: BTreeMap<String, PageHead>,
  pub engine: Option<SearchResults>,
  /// Every read asked for, in order: `page <url>`, `rendered <url>`, `rendered fresh <url>`, `document <url>`,
  /// `head <url>` and `engine <query>`.
  pub asked: std::sync::Mutex<Vec<String>>,
  /// Every slice a document was read in, with the address it was read at.
  pub slices: std::sync::Mutex<Vec<(String, Slice)>>,
  /// Every request made of a page other than a plain one, with the address it went to.
  pub requests: std::sync::Mutex<Vec<(String, PageRequest)>>,
  /// Every body posted, with the address it went to.
  pub posted: std::sync::Mutex<Vec<(String, String)>>,
}

fn locked<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Fixtures {
  pub fn page(mut self, url: &str, body: impl Into<String>) -> Self {
    self.pages.insert(url.to_owned(), body.into());
    self
  }

  pub fn refused_first(self, url: &str, said: &[&str]) -> Self {
    locked(&self.first_refusals).insert(url.to_owned(), said.iter().map(|one| (*one).to_owned()).collect());
    self
  }

  pub fn refused(mut self, url: &str, said: impl Into<String>) -> Self {
    self.refusals.insert(url.to_owned(), said.into());
    self
  }

  pub fn answer(mut self, url: &str, body: impl Into<String>, answer: impl Into<String>) -> Self {
    self.answers.insert((url.to_owned(), body.into()), answer.into());
    self
  }

  /// The search engines' answer to every search, as a fixture holds it: `{ hits: [{ title, url, snippet, engine }],
  /// unresponsive: [..] }`.
  pub fn searched(mut self, found: &Value) -> Self {
    let names = |key: &str| {
      found[key]
        .as_array()
        .map(|names| names.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
    };
    let hits = found["hits"]
      .as_array()
      .map(|hits| {
        hits
          .iter()
          .map(|hit| SearchHit {
            title: hit["title"].as_str().unwrap_or("").to_owned(),
            url: hit["url"].as_str().unwrap_or("").to_owned(),
            snippet: hit["snippet"].as_str().unwrap_or("").to_owned(),
            engine: hit["engine"].as_str().unwrap_or("").to_owned(),
          })
          .collect()
      })
      .unwrap_or_default();

    self.engine = Some(SearchResults {
      hits,
      unresponsive: names("unresponsive"),
      off_topic: names("offTopic"),
      health: Vec::new(),
    });
    self
  }

  pub fn otherwise(mut self, said: impl Into<String>) -> Self {
    self.otherwise = Some(said.into());
    self
  }

  pub fn rendered(self, url: &str, page: Rendered) -> Self {
    locked(&self.rendered).entry(url.to_owned()).or_default().push(page);
    self
  }

  pub fn headed(mut self, url: &str, head: PageHead) -> Self {
    self.heads.insert(url.to_owned(), head);
    self
  }

  pub fn document(mut self, url: &str, body: impl Into<String>) -> Self {
    self.documents.insert(url.to_owned(), body.into());
    self
  }

  pub fn asked(&self) -> Vec<String> {
    locked(&self.asked).clone()
  }

  pub fn posted(&self) -> Vec<(String, String)> {
    locked(&self.posted).clone()
  }

  fn note(&self, said: String) {
    locked(&self.asked).push(said);
  }
}

fn missing(url: &str) -> WebError {
  WebError::new(format!("no fixture for {url}"), Failure::Transport)
}

impl Reach for Fixtures {
  fn page<'a>(
    &'a self,
    url: &'a str,
    request: Option<PageRequest>,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    self.note(format!("page {url}"));

    if let Some(request) = &request {
      locked(&self.requests).push((url.to_owned(), request.clone()));
    }

    let posted = request.and_then(|request| request.body);

    if let Some(body) = &posted {
      locked(&self.posted).push((url.to_owned(), body.clone()));
    }

    let answered = posted.and_then(|body| self.answers.get(&(url.to_owned(), body)).cloned());
    let first_refusal = locked(&self.first_refusals)
      .get_mut(url)
      .filter(|left| !left.is_empty())
      .map(|left| left.remove(0));

    if let Some(said) = first_refusal {
      return Box::pin(async move { Err(WebError::new(said, Failure::Http)) });
    }

    let found = match (answered, self.refusals.get(url)) {
      (Some(answer), _) => Ok(answer),
      (None, Some(said)) => Err(WebError::new(said.clone(), Failure::Http)),
      (None, None) => self.pages.get(url).cloned().ok_or_else(|| match &self.otherwise {
        Some(said) => WebError::new(said.clone(), Failure::Http),
        None => missing(url),
      }),
    };

    Box::pin(async move { found })
  }

  fn rendered<'a>(
    &'a self,
    url: &'a str,
    fresh: bool,
    _: Option<Arrival>,
  ) -> Pin<Box<dyn Future<Output = WebResult<Rendered>> + Send + 'a>> {
    self.note(format!("rendered{} {url}", if fresh { " fresh" } else { "" }));

    let found = {
      let mut held = locked(&self.rendered);

      match held.get_mut(url) {
        Some(pages) if pages.len() > 1 => Ok(pages.remove(0)),
        Some(pages) if !pages.is_empty() => Ok(pages[0].clone()),
        _ => Err(missing(url)),
      }
    };

    Box::pin(async move { found })
  }

  fn engine<'a>(
    &'a self,
    query: &'a str,
    _: usize,
  ) -> Pin<Box<dyn Future<Output = WebResult<SearchResults>> + Send + 'a>> {
    self.note(format!("engine {query}"));

    let found = self.engine.clone().ok_or_else(|| missing(query));

    Box::pin(async move { found })
  }

  fn document<'a>(
    &'a self,
    url: &'a str,
    slice: Slice,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    self.note(format!("document {url}"));
    locked(&self.slices).push((url.to_owned(), slice));

    let found = self.documents.get(url).cloned().ok_or_else(|| missing(url));

    Box::pin(async move { found })
  }

  fn head<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = WebResult<PageHead>> + Send + 'a>> {
    self.note(format!("head {url}"));

    let found = self.heads.get(url).cloned().ok_or_else(|| missing(url));

    Box::pin(async move { found })
  }
}

/// A page as the browser drew it, for a test.
pub fn drawn(url: &str, status: u16, html: impl Into<String>, text: impl Into<String>) -> Rendered {
  Rendered {
    url: url.to_owned(),
    status,
    html: html.into(),
    text: text.into(),
  }
}
