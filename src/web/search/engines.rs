//! Every search engine the web layer asks, each its own module answering for itself (docs/web-access.md, 「Rosters」):
//! how it is asked, how its answer is read, and its own block pages where it has them.

use url::Url;

use super::engine::{
  Cause, Diagnosis, Engine, EngineAnswer, EngineHit, EngineImage, EngineLicence, EngineQuery, attribute_of,
  collect_items, hits_of, kept_hits, kept_images, pixels, squeeze, string, string_where, text, text_of,
  text_where,
};

fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
  let mut url = Url::parse(base).expect("a fixed address");

  url.query_pairs_mut().extend_pairs(pairs);
  url.to_string()
}

// ── Naver ──

/// Naver's own search: its web documents and its news tab, read off the result markup Naver writes into the
/// page.
pub struct Naver {
  news: bool,
}

pub const NAVER: Naver = Naver { news: false };
pub const NAVER_NEWS: Naver = Naver { news: true };

const NAVER_SEARCH: &str = "https://search.naver.com/search.naver";

// Naver's own check for automated searching, which it answers with a page of no results.
const NAVER_CHECK: [&str; 4] =
  ["보안 확인을 완료해 주세요", "자동입력 방지", "비정상적인 검색", "비정상적인 접근"];

/// A web document is a card of its own; its title is the headline text, its address the first link out of
/// Naver that is not Naver's own save-for-later.
pub fn parse_naver_web(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "div.fds-web-normal-doc-root",
      &[
        ("title", text_of(".sds-comps-text-type-headline1")),
        ("url", attribute_of("a[href^=\"http\"]:not([href*=\"keep.naver.com\"])", "href")),
        ("snippet", text_of(".sds-comps-text-type-body1")),
      ],
    ),
    NAVER_SEARCH,
    false,
  )
}

/// A news article's card carries the paper's own link first; the article is the link that holds the headline.
pub fn parse_naver_news(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "div.sds-comps-base-layout.sds-comps-full-layout",
      &[
        ("title", text_of(".sds-comps-text-type-headline1")),
        ("url", attribute_of("a[nocr=\"1\"][href]:not([data-heatmap-target=\".prof\"])", "href")),
        ("snippet", text_of(".sds-comps-text-type-body1")),
      ],
    ),
    NAVER_SEARCH,
    true,
  )
}

impl Engine for Naver {
  fn name(&self) -> &'static str {
    if self.news { "naver news" } else { "naver" }
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(
      NAVER_SEARCH,
      &[("where", if self.news { "news" } else { "web" }), ("query", query.query)],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    if self.news { parse_naver_news(body) } else { parse_naver_web(body) }
  }

  // Naver's 접속 제한 page lists every cause it might have, whichever one it met. Measured from this machine
  // it answers every direct client at any pace and from every address of this network, which rules out the
  // rate, and Naver's book pages are then served to a real browser; so it is read as the client's shape.
  fn recognise(&self, answer: &EngineAnswer) -> Option<Diagnosis> {
    if answer.body.contains("접속이 일시적으로 제한") {
      return Some(Diagnosis {
        cause: Cause::ClientShape,
        evidence: format!("HTTP {}, Naver's \"접속이 일시적으로 제한\" page", answer.status),
      });
    }

    let check = NAVER_CHECK.iter().find(|words| answer.body.contains(*words))?;

    if answer.status >= 300 || !self.parse(&answer.body).is_empty() {
      return None;
    }

    Some(Diagnosis {
      cause: Cause::ClientShape,
      evidence: format!("HTTP {}, Naver's automated-search check \"{check}\"", answer.status),
    })
  }

  fn warm_up(&self) -> Option<&'static str> {
    Some("https://www.naver.com/")
  }
}

// ── Daum ──

/// Daum's search: its integrated results and its news tab on search.daum.net.
pub struct Daum {
  news: bool,
}

pub const DAUM: Daum = Daum { news: false };
pub const DAUM_NEWS: Daum = Daum { news: true };

const DAUM_SEARCH: &str = "https://search.daum.net/search";

/// The integrated results are cards whose title element names its own address.
pub fn parse_daum_web(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "c-card",
      &[
        ("title", text_of("c-title")),
        ("url", attribute_of("c-title", "data-href")),
        ("snippet", text_of("c-contents-desc")),
      ],
    ),
    DAUM_SEARCH,
    false,
  )
}

/// The news tab is a plain list: the headline link, and the lead under it.
pub fn parse_daum_news(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "ul.c-list-basic > li",
      &[
        ("title", text_of(".item-title a")),
        ("url", attribute_of(".item-title a", "href")),
        ("snippet", text_of(".conts-desc")),
      ],
    ),
    DAUM_SEARCH,
    false,
  )
}

impl Engine for Daum {
  fn name(&self) -> &'static str {
    if self.news { "daum news" } else { "daum" }
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(DAUM_SEARCH, &[("w", if self.news { "news" } else { "tot" }), ("q", query.query)])
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    if self.news { parse_daum_news(body) } else { parse_daum_web(body) }
  }

  fn warm_up(&self) -> Option<&'static str> {
    Some("https://www.daum.net/")
  }
}

// ── DuckDuckGo ──

/// DuckDuckGo's lite page. Its HTML page tells this machine that the browser is not supported; the lite page
/// answers, and it is a table: one row holds a result's link, the next its snippet.
pub struct DuckDuckGo;

const DUCKDUCKGO_LITE: &str = "https://lite.duckduckgo.com/lite/";

// A result link goes through DuckDuckGo's own redirect, which carries the address in `uddg`.
fn duckduckgo_target(href: &str) -> String {
  let Ok(url) = Url::parse(DUCKDUCKGO_LITE).and_then(|base| base.join(href)) else {
    return String::new();
  };

  url
    .query_pairs()
    .find(|(name, _)| name == "uddg")
    .map_or_else(|| url.to_string(), |(_, value)| value.into_owned())
}

pub fn parse_duckduckgo_lite(body: &str) -> Vec<EngineHit> {
  let page = scraper::Html::parse_document(body);
  let pieces = scraper::Selector::parse("a.result-link, td.result-snippet").expect("a fixed selector");
  let mut hits: Vec<EngineHit> = Vec::new();

  for piece in page.select(&pieces) {
    if piece.value().name() == "a" {
      hits.push(EngineHit {
        title: squeeze(&piece.text().collect::<String>()),
        url: duckduckgo_target(piece.value().attr("href").unwrap_or("")),
        snippet: String::new(),
      });
    } else if let Some(last) = hits.last_mut() {
      last.snippet = squeeze(&piece.text().collect::<String>());
    }
  }

  kept_hits(hits)
}

impl Engine for DuckDuckGo {
  fn name(&self) -> &'static str {
    "duckduckgo"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(DUCKDUCKGO_LITE, &[("q", query.query)])
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_duckduckgo_lite(body)
  }

  // DuckDuckGo's refusal of an automated-looking client: a page of no results that asks the reader to prove
  // they are not a robot.
  fn recognise(&self, answer: &EngineAnswer) -> Option<Diagnosis> {
    let lower = answer.body.to_lowercase();

    ["anomaly-modal", "if this error persists", "confirm this search was made by a human"]
      .iter()
      .any(|words| lower.contains(words))
      .then(|| Diagnosis {
        cause: Cause::ClientShape,
        evidence: format!("HTTP {}, DuckDuckGo's anomaly page", answer.status),
      })
  }
}

// ── ZapMeta ──

/// ZapMeta's result page: a list of articles, each a headline link and a description.
pub struct ZapMeta;

const ZAPMETA_SEARCH: &str = "https://www.zapmeta.com/search";

pub fn parse_zapmeta(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "article.organic-results-item",
      &[
        ("title", text_of("h2")),
        ("url", attribute_of("h2 a", "href")),
        ("snippet", text_of("p")),
      ],
    ),
    ZAPMETA_SEARCH,
    false,
  )
}

impl Engine for ZapMeta {
  fn name(&self) -> &'static str {
    "zapmeta"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(ZAPMETA_SEARCH, &[("q", query.query)])
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_zapmeta(body)
  }
}

// ── Yandex ──

/// Yandex's site-search page. Its ordinary search page sends this machine to a CAPTCHA; the site-search page
/// answers the whole web with plain markup.
pub struct Yandex;

const YANDEX_SITE: &str = "https://yandex.com/search/site/";

pub fn parse_yandex_site(body: &str) -> Vec<EngineHit> {
  hits_of(
    &collect_items(
      body,
      "li.b-serp-item",
      &[
        ("title", text_of("h3.b-serp-item__title")),
        ("url", attribute_of("a.b-serp-item__title-link", "href")),
        ("snippet", text_of(".b-serp-item__text")),
      ],
    ),
    YANDEX_SITE,
    false,
  )
}

impl Engine for Yandex {
  fn name(&self) -> &'static str {
    "yandex"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    let language = query.language.split('-').next().unwrap_or("").to_lowercase();
    let mut pairs = vec![
      ("tmpl_version", "releases"),
      ("text", query.query),
      ("web", "1"),
      ("frame", "1"),
      ("searchid", "3131712"),
    ];

    if language.len() == 2 && language.chars().all(|c| c.is_ascii_lowercase()) {
      pairs.push(("lang", language.as_str()));
      return with_query(YANDEX_SITE, &pairs);
    }

    with_query(YANDEX_SITE, &pairs)
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_yandex_site(body)
  }

  // Yandex's CAPTCHA is a page of its own that the request is sent to.
  fn recognise(&self, answer: &EngineAnswer) -> Option<Diagnosis> {
    let lower = answer.url.to_lowercase();

    (lower.contains("showcaptcha") || lower.contains("checkcaptcha")).then(|| Diagnosis {
      cause: Cause::ClientShape,
      evidence: format!(
        "HTTP {}, sent to Yandex's CAPTCHA at {}",
        answer.status,
        answer.url.split('?').next().unwrap_or("")
      ),
    })
  }
}

// ── Bing News ──

/// Bing News as its RSS feed. Bing's web search answers this network with the query's leading term alone;
/// its news feed answers the query it was asked.
pub struct BingNews;

const BING_FEED: &str = "https://www.bing.com/news/search";

// A field of an item as text. A field may carry markup of its own, escaped or in a CDATA block, and what it
// says is the text of that markup, each piece the markup sets apart standing apart by a space.
fn feed_field(item: roxmltree::Node<'_, '_>, name: &str) -> String {
  let markup: String = item
    .children()
    .find(|child| child.is_element() && child.tag_name().name() == name)
    .map(|found| {
      found
        .descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect()
    })
    .unwrap_or_default();
  let fragment = scraper::Html::parse_fragment(&markup);

  squeeze(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

// An item's link goes through Bing's click counter, which carries the article's address in `url`.
fn bing_article(link: &str) -> String {
  match Url::parse(link) {
    Ok(url) => url
      .query_pairs()
      .find(|(name, _)| name == "url")
      .map_or_else(|| link.to_owned(), |(_, value)| value.into_owned()),
    Err(_) => String::new(),
  }
}

pub fn parse_bing_news_rss(body: &str) -> Vec<EngineHit> {
  let Ok(feed) = roxmltree::Document::parse_with_options(
    body,
    roxmltree::ParsingOptions {
      allow_dtd: true,
      ..Default::default()
    },
  ) else {
    return Vec::new();
  };
  let hits = feed
    .descendants()
    .filter(|node| node.is_element() && node.tag_name().name() == "item")
    .map(|item| {
      let source = feed_field(item, "Source");
      let published = feed_field(item, "pubDate");
      let byline = [source, published]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
      let snippet = [feed_field(item, "description"), byline]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" — ");

      EngineHit {
        title: feed_field(item, "title"),
        url: bing_article(&feed_field(item, "link")),
        snippet,
      }
    })
    .collect();

  kept_hits(hits)
}

impl Engine for BingNews {
  fn name(&self) -> &'static str {
    "bing news"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    let mut pairs = vec![("q", query.query), ("format", "rss")];

    if query.language.to_lowercase().starts_with("ko") {
      pairs.push(("setlang", "ko"));
    }

    with_query(BING_FEED, &pairs)
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_bing_news_rss(body)
  }
}

// ── The image APIs ──

fn json(body: &str) -> serde_json::Value {
  serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

fn joined(parts: &[String], by: &str) -> String {
  parts.iter().filter(|part| !part.is_empty()).cloned().collect::<Vec<_>>().join(by)
}

// A JSON value's address, where it is an http or https one.
fn web_where(value: &serde_json::Value) -> Option<String> {
  string_where(value).filter(|address| address.starts_with("http://") || address.starts_with("https://"))
}

/// Openverse's image API: a result's address is the page the picture stands on, and the picture's own
/// address rides in its snippet.
pub struct Openverse;

pub fn parse_openverse(body: &str) -> Vec<EngineHit> {
  let payload = json(body);
  let Some(results) = payload["results"].as_array() else {
    return Vec::new();
  };

  kept_hits(
    results
      .iter()
      .filter(|result| result.is_object())
      .map(|result| {
        let creator = text(&result["creator"]);
        let licence = text(&result["license"]);

        EngineHit {
          title: text(&result["title"]),
          url: string(&result["foreign_landing_url"]),
          snippet: joined(
            &[
              result["url"].as_str().map_or_else(String::new, |url| format!("image: {url}")),
              if creator.is_empty() { String::new() } else { format!("by {creator}") },
              if licence.is_empty() {
                String::new()
              } else {
                format!("licence {licence} {}", text(&result["license_version"])).trim().to_owned()
              },
            ],
            " — ",
          ),
        }
      })
      .collect(),
  )
}

// The licence Openverse names: its slug and version, the address it gives for it and its attribution line.
fn openverse_licence(result: &serde_json::Value) -> Option<EngineLicence> {
  let slug = string_where(&result["license"])?;
  let version = string(&result["license_version"]);

  Some(EngineLicence {
    id: openverse_licence_id(&slug, &version),
    url: string_where(&result["license_url"]),
    attribution: text_where(&result["attribution"]),
  })
}

fn openverse_licence_id(slug: &str, version: &str) -> String {
  let family = match slug {
    "cc0" => "CC0".to_owned(),
    "pdm" => "PDM".to_owned(),
    other => format!("CC-{}", other.to_uppercase()),
  };

  if version.is_empty() { family } else { format!("{family}-{version}") }
}

// The media type Openverse names in its filetype; a filetype it does not state is none.
fn openverse_media_type(result: &serde_json::Value) -> Option<String> {
  let kind = string_where(&result["filetype"])?;
  let mime = match kind.to_lowercase().as_str() {
    "jpg" | "jpeg" => "image/jpeg",
    "png" => "image/png",
    "gif" => "image/gif",
    "webp" => "image/webp",
    "svg" => "image/svg+xml",
    "tif" | "tiff" => "image/tiff",
    "avif" => "image/avif",
    _ => return None,
  };

  Some(mime.to_owned())
}

pub fn parse_openverse_images(body: &str) -> Vec<EngineImage> {
  let payload = json(body);
  let Some(results) = payload["results"].as_array() else {
    return Vec::new();
  };

  kept_images(
    results
      .iter()
      .filter(|result| result.is_object())
      .map(|result| EngineImage {
        url: string(&result["url"]),
        page_url: web_where(&result["foreign_landing_url"]),
        title: text(&result["title"]),
        creator: text_where(&result["creator"]),
        publisher: text_where(&result["provider"]).or_else(|| text_where(&result["source"])),
        width: pixels(&result["width"]),
        height: pixels(&result["height"]),
        media_type: openverse_media_type(result),
        licence: openverse_licence(result),
      })
      .collect(),
  )
}

impl Engine for Openverse {
  fn name(&self) -> &'static str {
    "openverse"
  }

  fn image_source(&self) -> bool {
    true
  }

  fn parse_images(&self, body: &str) -> Option<Vec<EngineImage>> {
    Some(parse_openverse_images(body))
  }

  // Without `format=json` the API answers a browser with its HTML console.
  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(
      "https://api.openverse.org/v1/images/",
      &[("page", "1"), ("page_size", "20"), ("format", "json"), ("q", query.query)],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_openverse(body)
  }
}

/// Wikimedia Commons through the MediaWiki query API, searching file pages for pictures and drawings.
pub struct WikimediaCommons;

pub fn parse_wikimedia_commons(body: &str) -> Vec<EngineHit> {
  let payload = json(body);
  let Some(pages) = payload["query"]["pages"].as_object() else {
    return Vec::new();
  };
  let mut ordered: Vec<&serde_json::Value> = pages.values().filter(|page| page.is_object()).collect();

  // The API hands the pages back keyed by id; `index` is the order the search ranked them in.
  ordered.sort_by(|a, b| a["index"].as_f64().unwrap_or(0.0).total_cmp(&b["index"].as_f64().unwrap_or(0.0)));

  kept_hits(
    ordered
      .into_iter()
      .map(|page| {
        let info = &page["imageinfo"][0];
        let size = match (info["width"].as_u64(), info["height"].as_u64()) {
          (Some(width), Some(height)) => format!("{width}×{height}"),
          _ => String::new(),
        };
        let title = text(&page["title"]);

        EngineHit {
          title: title.strip_prefix("File:").unwrap_or(&title).to_owned(),
          url: string(&info["descriptionurl"]),
          snippet: joined(
            &[
              info["url"]
                .as_str()
                .map_or_else(String::new, |url| format!("image: {}", url.split('?').next().unwrap_or(""))),
              string(&info["mime"]),
              size,
            ],
            " — ",
          ),
        }
      })
      .collect(),
  )
}

// The licence Commons states in a file's metadata: its short name, the address it gives and the artist it
// names; a file whose metadata states none carries no licence.
fn commons_licence(metadata: &serde_json::Value, creator: Option<&str>) -> Option<EngineLicence> {
  let short = text(&metadata["LicenseShortName"]["value"]);
  let template = text(&metadata["License"]["value"]);
  let id = commons_licence_id(&short, &template);

  if id.is_empty() {
    return None;
  }

  let named = if short.is_empty() { template } else { short };
  let parts: Vec<&str> = [creator.unwrap_or(""), named.as_str()]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();

  Some(EngineLicence {
    id,
    url: string_where(&metadata["LicenseUrl"]["value"]),
    attribution: (!parts.is_empty()).then(|| parts.join(" — ")),
  })
}

fn commons_licence_id(short: &str, template: &str) -> String {
  let named = if short.trim().is_empty() { template.trim() } else { short.trim() };
  let upper = named.to_uppercase();

  if upper.starts_with("CC") {
    upper.split_whitespace().collect::<Vec<_>>().join("-")
  } else {
    named.to_owned()
  }
}

pub fn parse_wikimedia_commons_images(body: &str) -> Vec<EngineImage> {
  let payload = json(body);
  let Some(pages) = payload["query"]["pages"].as_object() else {
    return Vec::new();
  };
  let mut ordered: Vec<&serde_json::Value> = pages.values().filter(|page| page.is_object()).collect();

  ordered.sort_by(|a, b| a["index"].as_f64().unwrap_or(0.0).total_cmp(&b["index"].as_f64().unwrap_or(0.0)));

  kept_images(
    ordered
      .into_iter()
      .map(|page| {
        let info = &page["imageinfo"][0];
        let metadata = &info["extmetadata"];
        let title = text(&page["title"]);
        let creator =
          text_where(&metadata["Artist"]["value"]).or_else(|| text_where(&metadata["Credit"]["value"]));

        EngineImage {
          url: string(&info["url"]).split('?').next().unwrap_or("").to_owned(),
          page_url: web_where(&info["descriptionurl"]),
          title: title.strip_prefix("File:").unwrap_or(&title).to_owned(),
          creator: creator.clone(),
          publisher: Some("Wikimedia Commons".to_owned()),
          width: pixels(&info["width"]),
          height: pixels(&info["height"]),
          media_type: string_where(&info["mime"]),
          licence: commons_licence(metadata, creator.as_deref()),
        }
      })
      .collect(),
  )
}

impl Engine for WikimediaCommons {
  fn name(&self) -> &'static str {
    "wikicommons.images"
  }

  fn image_source(&self) -> bool {
    true
  }

  fn parse_images(&self, body: &str) -> Option<Vec<EngineImage>> {
    Some(parse_wikimedia_commons_images(body))
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    let language = query.language.split('-').next().filter(|language| !language.is_empty()).unwrap_or("en");
    let search = format!("filetype:bitmap|drawing {}", query.query);

    with_query(
      "https://commons.wikimedia.org/w/api.php",
      &[
        ("format", "json"),
        ("uselang", language),
        ("action", "query"),
        ("prop", "info|imageinfo"),
        ("generator", "search"),
        ("gsrnamespace", "6"),
        ("gsrprop", "snippet"),
        ("gsrlimit", "10"),
        ("gsrsearch", &search),
        ("iiprop", "url|size|mime|extmetadata"),
      ],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_wikimedia_commons(body)
  }
}

/// Unsplash's own public search, the one its site pages call, which needs no key.
pub struct Unsplash;

pub fn parse_unsplash(body: &str) -> Vec<EngineHit> {
  let payload = json(body);
  let Some(results) = payload["results"].as_array() else {
    return Vec::new();
  };

  kept_hits(
    results
      .iter()
      .filter(|result| result.is_object())
      .map(|result| {
        let described = text(&result["description"]);
        let alt = text(&result["alt_description"]);
        let title = if !alt.is_empty() {
          alt
        } else if !described.is_empty() {
          described.clone()
        } else {
          "untitled photo".to_owned()
        };

        EngineHit {
          title,
          url: string(&result["links"]["html"]),
          snippet: joined(
            &[
              result["urls"]["regular"]
                .as_str()
                .map_or_else(String::new, |url| format!("image: {url}")),
              described,
            ],
            " — ",
          ),
        }
      })
      .collect(),
  )
}

pub fn parse_unsplash_images(body: &str) -> Vec<EngineImage> {
  let payload = json(body);
  let Some(results) = payload["results"].as_array() else {
    return Vec::new();
  };

  kept_images(
    results
      .iter()
      .filter(|result| result.is_object())
      .map(|result| {
        let described = text(&result["description"]);
        let alt = text(&result["alt_description"]);
        let title = if !alt.is_empty() {
          alt
        } else if !described.is_empty() {
          described
        } else {
          "untitled photo".to_owned()
        };
        // The largest rendition the answer names, so the dimensions beside it describe the address given.
        let url = ["full", "regular", "small"]
          .iter()
          .find_map(|size| string_where(&result["urls"][size]))
          .unwrap_or_default();

        EngineImage {
          url,
          page_url: web_where(&result["links"]["html"]),
          title,
          creator: text_where(&result["user"]["name"]).or_else(|| text_where(&result["user"]["username"])),
          publisher: Some("Unsplash".to_owned()),
          width: pixels(&result["width"]),
          height: pixels(&result["height"]),
          // The search answer states neither a media type nor a licence for one photo; none is guessed.
          media_type: None,
          licence: None,
        }
      })
      .collect(),
  )
}

impl Engine for Unsplash {
  fn name(&self) -> &'static str {
    "unsplash"
  }

  fn image_source(&self) -> bool {
    true
  }

  fn parse_images(&self, body: &str) -> Option<Vec<EngineImage>> {
    Some(parse_unsplash_images(body))
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(
      "https://unsplash.com/napi/search/photos",
      &[("query", query.query), ("page", "1"), ("per_page", "20")],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_unsplash(body)
  }
}

// ── IT search ──

/// GitHub's repository search API, ranked by stars.
pub struct GitHub;

pub fn parse_github_repositories(body: &str) -> Vec<EngineHit> {
  let payload = json(body);
  let Some(items) = payload["items"].as_array() else {
    return Vec::new();
  };

  kept_hits(
    items
      .iter()
      .filter(|item| item.is_object())
      .map(|item| EngineHit {
        title: text(&item["full_name"]),
        url: string(&item["html_url"]),
        snippet: joined(
          &[
            text(&item["description"]),
            item["stargazers_count"].as_u64().map_or_else(String::new, |stars| format!("★ {stars}")),
            text(&item["language"]),
            item["updated_at"].as_str().map_or_else(String::new, |at| {
              format!("updated {}", at.chars().take(10).collect::<String>())
            }),
          ],
          " · ",
        ),
      })
      .collect(),
  )
}

impl Engine for GitHub {
  fn name(&self) -> &'static str {
    "github"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    with_query(
      "https://api.github.com/search/repositories",
      &[("sort", "stars"), ("order", "desc"), ("q", query.query)],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_github_repositories(body)
  }
}

/// Stack Overflow through the Stack Exchange API, ranked by relevance.
pub struct StackOverflow;

pub fn parse_stack_exchange(body: &str) -> Vec<EngineHit> {
  let payload = json(body);
  let Some(items) = payload["items"].as_array() else {
    return Vec::new();
  };

  kept_hits(
    items
      .iter()
      .filter(|item| item.is_object())
      .map(|item| EngineHit {
        title: text(&item["title"]),
        url: string(&item["link"]),
        snippet: joined(
          &[
            item["tags"].as_array().map_or_else(String::new, |tags| {
              format!(
                "[{}]",
                tags.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(", ")
              )
            }),
            if item["is_answered"].as_bool() == Some(true) {
              "answered".into()
            } else {
              "unanswered".into()
            },
            item["score"].as_i64().map_or_else(String::new, |score| format!("score {score}")),
            item["answer_count"]
              .as_i64()
              .map_or_else(String::new, |count| format!("{count} answers")),
          ],
          " · ",
        ),
      })
      .collect(),
  )
}

impl Engine for StackOverflow {
  fn name(&self) -> &'static str {
    "stackoverflow"
  }

  fn request(&self, query: &EngineQuery<'_>) -> String {
    let size = query.count.min(30).to_string();

    with_query(
      "https://api.stackexchange.com/2.3/search/advanced",
      &[
        ("order", "desc"),
        ("sort", "relevance"),
        ("q", query.query),
        ("site", "stackoverflow"),
        ("pagesize", &size),
      ],
    )
  }

  fn parse(&self, body: &str) -> Vec<EngineHit> {
    parse_stack_exchange(body)
  }
}
