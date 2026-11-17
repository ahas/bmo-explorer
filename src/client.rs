//! The client (docs/api.md, 「The client」): the six operations this repository serves, each done once here and
//! answered in the common wire format. It holds one web reading — its direct client, the real browser it falls
//! back to and their cookie stores — from the moment it is opened until it is closed, on the store's pacing,
//! so its requests and every other process's are paced together.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use schemars::JsonSchema;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::forms::classify::readable_text;
use crate::forms::fetch::MAX_FETCH_CHARS;
use crate::forms::text::squeeze;
use crate::store;
use crate::web::address::Check;
use crate::web::client::{DIRECT_HOST_INTERVAL_MS, Direct, DirectBytes, Transport, decode, network};
use crate::web::identity::Presentation;
use crate::web::jar::Jar;
use crate::web::pacing::{Pacing, StorePacing};
use crate::web::picture::{fetch_raster, raster};
use crate::web::search::engine::declared;
use crate::web::search::roster::ROSTER;
use crate::web::search::{
  DEFAULT_SEARCH_COUNT, ENGINES, MAX_SEARCH_COUNT, Search, SearchRoads, image_engines_named,
  render_through, search_images as search_images_web, search_web,
};
use crate::web::session::{FetchPage, SessionParts, WebSession};
use crate::web::{Failure, WebError};

/// How much of a page is read for its head and its markup: the tags a preview needs sit in the head, and a page
/// larger than this is not going to be previewed well anyway.
pub const PAGE_HEAD_BYTES: usize = 512 * 1024;

/// Between two requests for a page's head to one host.
pub const PAGE_HEAD_HOST_INTERVAL_MS: u64 = 250;

/// The most characters a preview's title and site name are answered in, and its description.
const PREVIEW_TITLE_CHARS: usize = 300;
const PREVIEW_DESCRIPTION_CHARS: usize = 600;

/// The quality a picture narrowed to a width is written at, on the JPEG encoder's scale of 1 to 100.
const NARROWED_QUALITY: u8 = 85;

/// A failure in the wire format's error shape: a kebab-case code and the sentence saying what went wrong.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClientError {
  pub code: String,
  pub error: String,
}

impl ClientError {
  pub fn new(code: &str, error: impl Into<String>) -> Self {
    Self {
      code: code.to_owned(),
      error: error.into(),
    }
  }

  fn bad_request(error: impl Into<String>) -> Self {
    Self::new("bad-request", error)
  }
}

impl std::fmt::Display for ClientError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(formatter, "{}: {}", self.code, self.error)
  }
}

impl From<WebError> for ClientError {
  fn from(error: WebError) -> Self {
    let code = match error.kind {
      Failure::UnsafeAddress => "unsafe-address",
      Failure::TooLarge => "too-large",
      Failure::BrowserUnavailable => "failed",
      Failure::Blocked | Failure::Transport | Failure::Http => "upstream-failed",
    };

    Self::new(code, error.message)
  }
}

/// A search of the engines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebSearchInput {
  /// The query.
  pub q: String,
  /// The engines to ask: each an engine's name as explorer_search_engines lists it, or a category (general,
  /// news, images, it) standing for every engine of it. An empty list asks the general engines.
  pub engines: Vec<String>,
  /// How many hits to answer, 1 to 30; 10 where absent.
  #[serde(default)]
  pub limit: Option<u32>,
  /// The language the hits are wanted in, such as "ko" or "en"; any where absent.
  #[serde(default)]
  pub language: Option<String>,
}

/// The engines' hits, mixed one from each engine in turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchOutput {
  pub hits: Vec<Hit>,
  /// The engines that did not answer, each with why.
  pub unresponsive: Vec<String>,
  /// The engines whose answer did not carry the query's terms.
  pub off_topic: Vec<String>,
}

/// One hit as its engine answered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
  pub title: String,
  pub url: String,
  pub snippet: String,
  /// The engine that answered it.
  pub engine: String,
}

/// A search of the image engines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageSearchInput {
  /// The query.
  pub q: String,
  /// The image engines to ask: each an engine's name as explorer_search_engines lists it, or a category
  /// standing for its image engines. An empty list asks every image engine.
  #[serde(default)]
  pub engines: Vec<String>,
  /// How many pictures to answer, 1 to 30; 10 where absent.
  #[serde(default)]
  pub limit: Option<u32>,
  /// The language the query is written in, such as "ko" or "en"; any where absent.
  #[serde(default)]
  pub language: Option<String>,
}

/// The image engines' pictures, mixed one from each engine in turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageSearchOutput {
  pub images: Vec<Picture>,
  /// The engines that did not answer, each with why.
  pub unresponsive: Vec<String>,
  /// The engines whose answer did not carry the query's terms.
  pub off_topic: Vec<String>,
}

/// One picture as its engine answered it, each field the source's own statement or null.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
  /// The direct image address.
  pub url: String,
  /// The page the picture stands on; null where the engine names none.
  pub page_url: Option<String>,
  pub title: String,
  pub creator: Option<String>,
  pub publisher: Option<String>,
  /// The picture's width in pixels, where the source states it.
  pub width: Option<u32>,
  /// The picture's height in pixels, where the source states it.
  pub height: Option<u32>,
  pub media_type: Option<String>,
  /// The licence the source states for this picture; null where it states none.
  pub licence: Option<Licence>,
  /// The engine that answered it.
  pub engine: String,
}

/// The licence a source states for one picture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Licence {
  pub id: String,
  pub url: Option<String>,
  pub attribution: Option<String>,
}

/// One address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UrlInput {
  /// The page's address, http or https.
  pub url: String,
}

/// A page read as words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageText {
  /// The address the read ended at, after every redirect.
  pub url: String,
  /// The page's title; null where it has none.
  pub title: Option<String>,
  /// What a reader sees of the page; a PDF's text.
  pub text: String,
  /// Every http or https address the page links to, once each, made absolute.
  pub links: Vec<String>,
  /// True where the text was cut at 100,000 characters or the page at the most that is read of it.
  pub truncated: bool,
}

/// A link's preview, as the page's head declares it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
  /// The address the read ended at, after every redirect.
  pub url: String,
  pub title: Option<String>,
  pub description: Option<String>,
  /// The site's own name for itself, as its Open Graph tags declare it.
  pub site_name: Option<String>,
  /// The preview picture's absolute address.
  pub image: Option<String>,
}

/// A page's markup as it was served.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageSource {
  /// The address the read ended at, after every redirect.
  pub url: String,
  pub status: u16,
  /// The content type as the server sent it.
  pub content_type: String,
  /// At most 512 KiB of the markup, decoded as its charset says.
  pub html: String,
}

/// A picture asked for by its address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RasterInput {
  /// The picture's address, http or https.
  pub url: String,
  /// The most pixels wide the picture is answered at; a wider one is redrawn as JPEG that wide.
  #[serde(default)]
  pub width: Option<u32>,
}

/// A picture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Raster {
  /// The picture's bytes, base64 encoded.
  pub image: String,
  pub mime_type: String,
}

/// Nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

/// The engines a search may ask, with their standing, and the rest each refusal costs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchEnginesOutput {
  /// In the roster's order.
  pub engines: Vec<EngineStanding>,
  pub cooldowns: Vec<Cooldown>,
}

/// One engine as its last request left it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EngineStanding {
  pub name: String,
  pub category: String,
  /// Asked first for a Korean query and last for any other.
  pub korean_native: bool,
  /// healthy, degraded, suspended, or unknown where it has not been read.
  pub state: String,
  /// The cause and evidence that decided the state.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observed_at: Option<u64>,
  /// Until when it rests.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub suspended_until: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub elapsed_ms: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub result_count: Option<u64>,
}

/// How long an engine rests after one kind of refusal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cooldown {
  pub reason: String,
  pub rest_ms: i64,
}

/// The client over one web reading.
pub struct Explorer {
  session: Arc<WebSession>,
}

impl Explorer {
  /// The client over the data root's store, by the real network and resolver.
  pub fn open(data_root: &Path) -> Self {
    Self::over(WebSession::new(SessionParts {
      store: Arc::new(StorePacing::new(store::place(data_root))),
      transport: network(),
      check: Check::default(),
      host_interval_ms: DIRECT_HOST_INTERVAL_MS,
      browsers: None,
      vision: crate::web::vision::Gateway::open(data_root)
        .map(|road| Arc::new(road) as Arc<dyn crate::web::vision::Vision>),
    }))
  }

  /// The client over a session a test hands in.
  pub fn over(session: WebSession) -> Self {
    Self {
      session: Arc::new(session),
    }
  }

  /// The engines asked directly, the hits as they answered them.
  pub async fn web_search(&self, asked: &WebSearchInput) -> Result<WebSearchOutput, ClientError> {
    let query = asked.q.trim();

    if query.is_empty() {
      return Err(ClientError::bad_request("The query is empty."));
    }

    let count = match asked.limit {
      None => DEFAULT_SEARCH_COUNT,
      Some(limit @ 1..=30) => limit as usize,
      Some(limit) => {
        return Err(ClientError::bad_request(format!(
          "The limit is {limit}; it is 1 to {MAX_SEARCH_COUNT}."
        )));
      }
    };
    let engines = engines_named(&asked.engines)?;
    let found = search_web(
      &Search {
        query,
        count: Some(count),
        language: asked.language.as_deref(),
        category: None,
        engine_interval_ms: None,
        engines: &engines,
      },
      &SearchRoads {
        store: &**self.session.store(),
        check: self.session.check(),
        transport: &**self.session.transport(),
        render: Some(render_through(self.session.clone())),
      },
    )
    .await?;

    Ok(WebSearchOutput {
      hits: found
        .hits
        .into_iter()
        .map(|hit| Hit {
          title: hit.title,
          url: hit.url,
          snippet: hit.snippet,
          engine: hit.engine,
        })
        .collect(),
      unresponsive: found.unresponsive,
      off_topic: found.off_topic,
    })
  }

  /// The image engines asked directly, each picture answered as the fields its source states.
  pub async fn search_images(&self, asked: &ImageSearchInput) -> Result<ImageSearchOutput, ClientError> {
    let query = asked.q.trim();

    if query.is_empty() {
      return Err(ClientError::bad_request("The query is empty."));
    }

    let count = match asked.limit {
      None => DEFAULT_SEARCH_COUNT,
      Some(limit @ 1..=30) => limit as usize,
      Some(limit) => {
        return Err(ClientError::bad_request(format!(
          "The limit is {limit}; it is 1 to {MAX_SEARCH_COUNT}."
        )));
      }
    };
    let engines = image_engines_named(&asked.engines).map_err(ClientError::bad_request)?;
    let found = search_images_web(
      &Search {
        query,
        count: Some(count),
        language: asked.language.as_deref(),
        category: None,
        engine_interval_ms: None,
        engines: &engines,
      },
      &SearchRoads {
        store: &**self.session.store(),
        check: self.session.check(),
        transport: &**self.session.transport(),
        render: Some(render_through(self.session.clone())),
      },
    )
    .await?;

    Ok(ImageSearchOutput {
      images: found
        .images
        .into_iter()
        .map(|image| Picture {
          url: image.url,
          page_url: image.page_url,
          title: image.title,
          creator: image.creator,
          publisher: image.publisher,
          width: image.width,
          height: image.height,
          media_type: image.media_type,
          licence: image.licence.map(|licence| Licence {
            id: licence.id,
            url: licence.url,
            attribution: licence.attribution,
          }),
          engine: image.engine,
        })
        .collect(),
      unresponsive: found.unresponsive,
      off_topic: found.off_topic,
    })
  }

  /// One page read as words: the direct client first, the real browser after a block, a PDF as its text.
  pub async fn web_fetch(&self, url: &str) -> Result<PageText, ClientError> {
    let read = self
      .session
      .fetch_page(
        url,
        &FetchPage {
          max_chars: Some(usize::MAX),
          raw: true,
          browser_fallback: true,
          ..FetchPage::default()
        },
      )
      .await?;
    let markup = read.content_type.contains("html");
    let (title, text, links) = if markup {
      let page = Html::parse_document(&read.text);
      let title = page.select(&TITLE).next().map(|title| squeeze(&title.text().collect::<String>()));

      (
        title.filter(|title| !title.is_empty()),
        readable_text(page.tree.root()),
        links_of(&page, &read.url),
      )
    } else {
      (None, read.text, Vec::new())
    };
    let cut = text.chars().count() > MAX_FETCH_CHARS;

    Ok(PageText {
      url: read.url,
      title,
      text: if cut { text.chars().take(MAX_FETCH_CHARS).collect() } else { text },
      links,
      truncated: cut || read.source_truncated,
    })
  }

  /// A link's preview as the page's head declares it.
  pub async fn link_preview(&self, url: &str) -> Result<LinkPreview, ClientError> {
    link_preview(url, &**self.session.store(), self.session.check(), &**self.session.transport()).await
  }

  /// A page's markup as it was served, whatever its status.
  pub async fn fetch_page(&self, url: &str) -> Result<PageSource, ClientError> {
    let read = self.head(url).await?;

    if !is_markup(&read) {
      let served = if read.raw_content_type.is_empty() {
        "no content type"
      } else {
        &read.raw_content_type
      };

      return Err(ClientError::new("not-markup", format!("{} answered {served}, not markup.", read.url)));
    }

    Ok(PageSource {
      html: decode(&read.bytes, &read.raw_content_type),
      url: read.url,
      status: read.status,
      content_type: read.raw_content_type,
    })
  }

  /// The picture at an address, narrowed to a width where one is named.
  pub async fn fetch_raster(&self, asked: &RasterInput) -> Result<Raster, ClientError> {
    if asked.width == Some(0) {
      return Err(ClientError::bad_request("The width is 0; it is at least 1."));
    }

    let picture =
      fetch_raster(&asked.url, &**self.session.store(), self.session.check(), &**self.session.transport())
        .await?;
    let (bytes, kind) = match asked.width {
      Some(width) => {
        let fitted = raster::fitted_to_width(&picture.bytes, width, NARROWED_QUALITY).map_err(|why| {
          ClientError::new("upstream-failed", format!("The picture at {} does not read: {why}", asked.url))
        })?;

        if fitted.source_width > width {
          (fitted.jpeg, "image/jpeg")
        } else {
          (picture.bytes, picture.kind)
        }
      }
      None => (picture.bytes, picture.kind),
    };

    Ok(Raster {
      image: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
      mime_type: kind.to_owned(),
    })
  }

  /// The engines a search may ask, with their standing.
  pub async fn search_engines(&self) -> Result<SearchEnginesOutput, ClientError> {
    engine_standing(&**self.session.store()).await
  }

  /// The reading's browser and profile ended.
  pub async fn close(&self) {
    self.session.close().await;
  }

  async fn head(&self, url: &str) -> Result<DirectBytes, ClientError> {
    Ok(page_head(url, &**self.session.store(), self.session.check(), &**self.session.transport()).await?)
  }
}

/// A page's head and markup read directly: the address and each redirect checked, at most 512 KiB of it,
/// paced per host.
pub async fn page_head(
  url: &str,
  store: &dyn Pacing,
  check: &Check,
  transport: &dyn Transport,
) -> Result<DirectBytes, WebError> {
  let jar = tokio::sync::Mutex::new(Jar::default());
  let mut direct = Direct::new(store, &jar, check, transport);

  direct.interval_ms = PAGE_HEAD_HOST_INTERVAL_MS;
  direct.byte_cap = PAGE_HEAD_BYTES;
  direct.bytes(url).await
}

/// A link's preview as the page's head declares it, read as a link previewer (docs/api.md, 「The MCP server」):
/// the address checked and each redirect, at most 512 KiB of it, paced per host.
pub async fn link_preview(
  url: &str,
  store: &dyn Pacing,
  check: &Check,
  transport: &dyn Transport,
) -> Result<LinkPreview, ClientError> {
  let jar = tokio::sync::Mutex::new(Jar::default());
  let mut direct = Direct::new(store, &jar, check, transport);

  direct.interval_ms = PAGE_HEAD_HOST_INTERVAL_MS;
  direct.byte_cap = PAGE_HEAD_BYTES;
  direct.presentation = Presentation::Previewer;

  let read = direct.bytes(url).await?;

  if !(200..300).contains(&read.status) {
    return Err(ClientError::new("upstream-failed", format!("HTTP {} for {}", read.status, read.url)));
  }

  if !is_markup(&read) {
    return Ok(LinkPreview {
      url: read.url,
      title: None,
      description: None,
      site_name: None,
      image: None,
    });
  }

  let page = Html::parse_document(&decode(&read.bytes, &read.raw_content_type));
  let title = declared_in(&page, &["og:title", "twitter:title"]).or_else(|| {
    page
      .select(&TITLE)
      .next()
      .map(|title| squeeze(&title.text().collect::<String>()))
      .filter(|title| !title.is_empty())
  });
  let description = declared_in(&page, &["og:description", "twitter:description", "description"]);
  let site_name = declared_in(&page, &["og:site_name"]);
  let image = declared_in(
    &page,
    &["og:image", "og:image:url", "og:image:secure_url", "twitter:image", "twitter:image:src"],
  )
  .or_else(|| page.select(&IMAGE_SOURCE).find_map(|link| link.value().attr("href").map(str::to_owned)))
  .and_then(|image| absolute(&read.url, &image));

  Ok(LinkPreview {
    url: read.url,
    title: title.map(|title| shortened(title, PREVIEW_TITLE_CHARS)),
    description: description.map(|description| shortened(description, PREVIEW_DESCRIPTION_CHARS)),
    site_name: site_name.map(|name| shortened(name, PREVIEW_TITLE_CHARS)),
    image,
  })
}

// The text as it stands where it holds at most `most` characters, else its first ones and an ellipsis, `most`
// in all.
fn shortened(text: String, most: usize) -> String {
  if text.chars().count() <= most {
    return text;
  }

  let mut cut: String = text.chars().take(most - 1).collect();

  cut.push('\u{2026}');
  cut
}

/// Whether a read is markup: a page that is not — a PDF, a raw source file — has no head to read.
pub fn is_markup(read: &DirectBytes) -> bool {
  matches!(read.content_type.as_str(), "text/html" | "application/xhtml+xml")
}

/// Each engine of the roster with its standing as the store last saw it, and the rests of the refusals.
pub async fn engine_standing(store: &dyn Pacing) -> Result<SearchEnginesOutput, ClientError> {
  let mut engines = Vec::new();

  for engine in &ROSTER.engine {
    let health = store
      .health(&engine.name)
      .await
      .map_err(|why| ClientError::new("failed", format!("The engines' states could not be read: {why}")))?;
    let standing = EngineStanding {
      name: engine.name.clone(),
      category: engine.category.clone(),
      korean_native: engine.korean_native,
      state: "unknown".to_owned(),
      reason: None,
      observed_at: None,
      suspended_until: None,
      elapsed_ms: None,
      result_count: None,
    };

    engines.push(match health {
      Some(seen) => EngineStanding {
        state: seen.state.name().to_owned(),
        reason: seen.reason,
        observed_at: seen.observed_at,
        suspended_until: seen.suspended_until,
        elapsed_ms: seen.elapsed_ms,
        result_count: seen.result_count,
        ..standing
      },
      None => standing,
    });
  }

  let cooldowns = ROSTER
    .cooldown
    .iter()
    .map(|line| Cooldown {
      reason: line.reason.clone(),
      rest_ms: ROSTER.rests.named(&line.rest).unwrap_or(0),
    })
    .collect();

  Ok(SearchEnginesOutput { engines, cooldowns })
}

/// The roster names the asked engines stand for, in the roster's order: an engine by its own name, a category
/// by each engine of it. None asked stands for the general engines, which the search takes where it is handed
/// none.
fn engines_named(asked: &[String]) -> Result<Vec<&'static str>, ClientError> {
  let mut wanted: Vec<&'static str> = Vec::new();

  for name in asked.iter().map(|name| name.trim()) {
    let standing: Vec<&'static str> = ENGINES
      .iter()
      .map(|engine| engine.name())
      .filter(|engine| *engine == name || declared(engine).category == name)
      .collect();

    if standing.is_empty() {
      let known: Vec<&str> = ENGINES.iter().map(|engine| engine.name()).collect();

      return Err(ClientError::bad_request(format!(
        "No engine or category is named {name:?}; the engines are {} and the categories general, news, \
         images and it.",
        known.join(", ")
      )));
    }

    wanted.extend(standing);
  }

  Ok(
    ENGINES
      .iter()
      .map(|engine| engine.name())
      .filter(|engine| wanted.contains(engine))
      .collect(),
  )
}

static TITLE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("title").expect("a fixed selector"));
static ANCHOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").expect("a fixed selector"));
static IMAGE_SOURCE: LazyLock<Selector> =
  LazyLock::new(|| Selector::parse(r#"link[rel="image_src"][href]"#).expect("a fixed selector"));
static META: LazyLock<Selector> =
  LazyLock::new(|| Selector::parse("meta[content]").expect("a fixed selector"));

/// The first value the head declares under one of `keys`, in the order of the keys, by `property` or `name`.
fn declared_in(page: &Html, keys: &[&str]) -> Option<String> {
  keys.iter().find_map(|key| {
    page.select(&META).find_map(|meta| {
      let element = meta.value();
      let named = [element.attr("property"), element.attr("name")]
        .into_iter()
        .flatten()
        .any(|named| named.eq_ignore_ascii_case(key));
      let content = squeeze(element.attr("content").unwrap_or_default());

      (named && !content.is_empty()).then_some(content)
    })
  })
}

/// Every http or https address the page links to, absolute, without its fragment, once each in page order.
fn links_of(page: &Html, base: &str) -> Vec<String> {
  let mut links: Vec<String> = Vec::new();

  for anchor in page.select(&ANCHOR) {
    if let Some(link) = anchor.value().attr("href").and_then(|href| absolute(base, href))
      && !links.contains(&link)
    {
      links.push(link);
    }
  }

  links
}

/// `reference` made absolute against `base`, its fragment dropped; none where it is no http or https address.
fn absolute(base: &str, reference: &str) -> Option<String> {
  let mut address = Url::parse(base).ok()?.join(reference.trim()).ok()?;

  address.set_fragment(None);
  matches!(address.scheme(), "http" | "https").then(|| address.to_string())
}
