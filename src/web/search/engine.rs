//! A search engine as the web layer asks it itself (docs/web-access.md, 「Search engines」). Each engine is a module of
//! its own that says how it is asked and how its answer is read; this is the road every one of them goes by —
//! the recipe the engine is asked with, the browser it speaks as, the cookies it keeps, its pacing, the reading
//! of a refusal, the one retry with the remedy that refusal calls for, and what is written down afterwards.
//! It names no engine and no site.

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use url::Url;

use crate::web::address::Check;
use crate::web::chrome::BrowserDocument;
use crate::web::classify::{Disposition, Fetched, classify};
use crate::web::client::{Direct, Transport};
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::identity;
use crate::web::jar::Jar;
use crate::web::pacing::{Pacing, Recipe, Route, store_failure, wait_for_host};

/// One result as an engine lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineHit {
  pub title: String,
  pub url: String,
  pub snippet: String,
}

pub struct EngineQuery<'a> {
  pub query: &'a str,
  /// `ko-KR`, `en` and the like: what the query is written in.
  pub language: &'a str,
  pub count: usize,
}

/// What came back for one request, by whichever road it went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineAnswer {
  pub requested_url: String,
  pub url: String,
  pub status: u16,
  pub content_type: String,
  pub body: String,
  pub retry_after_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
  Rate,
  Network,
  ClientShape,
  Cookie,
  ScriptCheck,
  Consent,
}

impl Cause {
  pub fn name(self) -> &'static str {
    match self {
      Self::Rate => "rate",
      Self::Network => "network",
      Self::ClientShape => "client-shape",
      Self::Cookie => "cookie",
      Self::ScriptCheck => "script-check",
      Self::Consent => "consent",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnosis {
  pub cause: Cause,
  /// What decided it, as the answer showed it: the status, the wording, the address it ended on.
  pub evidence: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
  Slow,
  Warm,
  Reidentify,
  Alternate,
  Browser,
}

impl Remedy {
  pub fn name(self) -> &'static str {
    match self {
      Self::Slow => "slow",
      Self::Warm => "warm",
      Self::Reidentify => "reidentify",
      Self::Alternate => "alternate",
      Self::Browser => "browser",
    }
  }
}

/// What one engine declares of itself: its entry in the roster of the shared policy, how it is asked and how
/// its answer is read.
pub trait Engine: Send + Sync {
  /// The name its health, recipe and cookies are kept under; its roster entry is read by it.
  fn name(&self) -> &'static str;
  /// The address the query is asked at.
  fn request(&self, query: &EngineQuery<'_>) -> String;
  /// The engine's answer read into results, in the engine's own order.
  fn parse(&self, body: &str) -> Vec<EngineHit>;
  /// The engine's own block pages, beyond what every engine's answer is read for.
  fn recognise(&self, _answer: &EngineAnswer) -> Option<Diagnosis> {
    None
  }
  /// A page whose cookies a first request should carry.
  fn warm_up(&self) -> Option<&'static str> {
    None
  }
  /// Another form of the same search — mobile, lite — to try when the usual one is refused: how its address
  /// is built.
  fn alternate(&self) -> Option<fn(&EngineQuery<'_>) -> String> {
    None
  }
}

/// What the roster declares of an engine.
pub struct Declared {
  pub category: &'static str,
  pub korean_native: bool,
  pub english_web: bool,
}

/// The roster's entry for an engine by its name.
pub fn declared(name: &str) -> Declared {
  let entry = super::roster::ROSTER
    .engine
    .iter()
    .find(|one| one.name == name)
    .unwrap_or_else(|| panic!("the engine roster names no engine {name}"));

  Declared {
    category: entry.category.as_str(),
    korean_native: entry.korean_native,
    english_web: entry.english_web,
  }
}

/// How long an engine rests after a refusal its one retry did not clear; `None` for a failure that is no
/// refusal.
pub fn rest_ms(cause: Option<Cause>, after_browser: bool) -> u64 {
  let rests = &super::roster::ROSTER.rests;
  let ms = match cause {
    None => rests.failure_ms,
    Some(Cause::Network) => rests.network_ms,
    Some(_) if after_browser => rests.network_ms,
    Some(_) => rests.refusal_ms,
  };

  ms.max(0) as u64
}

/// The slowest an engine's own interval grows to.
pub const MAX_ENGINE_INTERVAL_MS: u64 = 60_000;

static RATE_WORDS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)too many requests|rate.?limit|slow down|너무 많은 요청|짧은 시간[^.]{0,30}많은 요청")
    .expect("a fixed pattern")
});
static NETWORK_WORDS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)unusual traffic from your computer network|(?:this|your) network|IP address|해당 네트워크|IP 주소").expect("a fixed pattern")
});
static SCRIPT_WORDS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)enable javascript|javascript is (?:disabled|required)|browser is not supported|자바스크립트를 (?:켜|활성화)|지원하지 않는 브라우저")
    .expect("a fixed pattern")
});

// The words around what matched, so the evidence carries the page's own wording.
fn wording(text: &str, pattern: &Regex) -> String {
  let Some(found) = pattern.find(text) else {
    return String::new();
  };
  let before: String = text[..found.start()]
    .chars()
    .rev()
    .take(40)
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect();
  let after: String = text[found.end()..].chars().take(40).collect();
  let line = format!("{before}{}{after}", found.as_str())
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");

  format!(" \"{line}\"")
}

fn consent_page(answer: &EngineAnswer) -> bool {
  if answer.url == answer.requested_url {
    return false;
  }

  Url::parse(&answer.url).is_ok_and(|url| {
    let host = url.host_str().unwrap_or("").to_lowercase();
    let path = url.path().to_lowercase();

    host.starts_with("consent.")
      || host.contains(".consent.")
      || path.split('/').any(|segment| segment == "consent" || segment.starts_with("consent?"))
  })
}

/// What an answer says about why it is not a list of results, or nothing where it is one.
pub fn diagnose(engine: &dyn Engine, answer: &EngineAnswer) -> Option<Diagnosis> {
  if let Some(own) = engine.recognise(answer) {
    return Some(own);
  }

  let page = classify(&Fetched {
    status: answer.status,
    content_type: &answer.content_type,
    body: &answer.body,
    requested_url: Some(&answer.requested_url),
    final_url: Some(&answer.url),
    ..Default::default()
  });
  let text: String = page.text.chars().take(20_000).collect();
  let refused = page.disposition == Disposition::Blocked || answer.status >= 400;
  let moved = if answer.url == answer.requested_url {
    String::new()
  } else {
    format!(" at {}", answer.url)
  };
  let status = answer.status;

  if status == 429
    || status == 420
    || (refused && (answer.retry_after_ms.is_some() || RATE_WORDS.is_match(&text)))
  {
    let after = answer
      .retry_after_ms
      .map_or_else(String::new, |ms| format!(", Retry-After {} s", (ms as f64 / 1_000.0).round()));

    return Some(Diagnosis {
      cause: Cause::Rate,
      evidence: format!("HTTP {status}{after}{}", wording(&text, &RATE_WORDS)),
    });
  }

  if refused && NETWORK_WORDS.is_match(&text) {
    return Some(Diagnosis {
      cause: Cause::Network,
      evidence: format!("HTTP {status}{}", wording(&text, &NETWORK_WORDS)),
    });
  }

  if consent_page(answer) {
    return Some(Diagnosis {
      cause: Cause::Consent,
      evidence: format!("HTTP {status}, sent to {}", answer.url),
    });
  }

  if page.disposition == Disposition::Blocked {
    return Some(Diagnosis {
      cause: Cause::ClientShape,
      evidence: format!("HTTP {status}, {}{moved}", page.reason.as_deref().unwrap_or("refused")),
    });
  }

  // A page that lists results is not asking for script, whatever else it carries, and a feed or a JSON answer
  // with nothing in it is an empty answer, not a script shell.
  let html_page = answer.content_type.to_lowercase().contains("html") || answer.content_type.is_empty();

  if html_page
    && (page.disposition == Disposition::EmptyShell || (status < 400 && SCRIPT_WORDS.is_match(&text)))
    && engine.parse(&answer.body).is_empty()
  {
    return Some(Diagnosis {
      cause: Cause::ScriptCheck,
      evidence: format!(
        "HTTP {status}, {}{}",
        page.reason.as_deref().unwrap_or("asks for script"),
        wording(&text, &SCRIPT_WORDS)
      ),
    });
  }

  None
}

/// The recipe an engine is asked with before anything has been learned about it.
pub fn fresh_recipe(engine: &dyn Engine, now: u64) -> Recipe {
  Recipe {
    engine: engine.name().to_owned(),
    identity: identity::DEFAULT.name.to_owned(),
    interval_ms: 0,
    route: Route::Direct,
    warm_up: false,
    cause: None,
    evidence: None,
    set_at: now,
  }
}

/// The remedy a cause calls for, given what the recipe already carries: never one already in it, and never
/// one this road cannot take.
pub fn remedy_for(cause: Cause, recipe: &Recipe, engine: &dyn Engine, can_render: bool) -> Option<Remedy> {
  let browser = (can_render && recipe.route != Route::Browser).then_some(Remedy::Browser);
  let alternate =
    (engine.alternate().is_some() && recipe.route == Route::Direct).then_some(Remedy::Alternate);

  match cause {
    Cause::Rate => Some(Remedy::Slow),
    Cause::Network => None,
    Cause::Cookie | Cause::Consent => {
      if engine.warm_up().is_some() && !recipe.warm_up {
        Some(Remedy::Warm)
      } else {
        browser
      }
    }
    Cause::ScriptCheck => alternate.or(browser),
    Cause::ClientShape => {
      if recipe.identity == identity::DEFAULT.name && recipe.route == Route::Direct {
        Some(Remedy::Reidentify)
      } else {
        alternate.or(browser)
      }
    }
  }
}

/// The recipe with a remedy taken into it.
pub fn with_remedy(
  recipe: &Recipe,
  remedy: Remedy,
  diagnosis: &Diagnosis,
  profile_interval_ms: u64,
  now: u64,
) -> Recipe {
  let mut next = Recipe {
    cause: Some(diagnosis.cause.name().to_owned()),
    evidence: Some(diagnosis.evidence.chars().take(1_000).collect()),
    set_at: now,
    ..recipe.clone()
  };

  match remedy {
    Remedy::Slow => {
      next.interval_ms =
        MAX_ENGINE_INTERVAL_MS.min((2 * recipe.interval_ms.max(profile_interval_ms)).max(1_000));
    }
    Remedy::Warm => next.warm_up = true,
    Remedy::Reidentify => {
      next.identity = identity::next(identity::named(Some(&recipe.identity))).name.to_owned()
    }
    Remedy::Alternate => next.route = Route::Alternate,
    Remedy::Browser => next.route = Route::Browser,
  }

  next
}

/// What an engine's pacing is kept under.
pub fn engine_key(engine: &dyn Engine) -> String {
  format!("engine:{}", engine.name().replace(' ', "_"))
}

/// The real browser an engine's recipe may send it by, where the caller has one to lend.
pub type Render =
  Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = WebResult<BrowserDocument>> + Send>> + Send + Sync>;

/// Everything an engine is asked through.
pub struct EngineRoads<'a> {
  pub store: &'a dyn Pacing,
  /// The research run's engine gap; an engine whose recipe asks for more gets more.
  pub interval_ms: u64,
  pub check: &'a Check,
  pub transport: &'a dyn Transport,
  pub render: Option<Render>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
  Answered {
    hits: Vec<EngineHit>,
    elapsed_ms: u64,
    remedy: Option<Remedy>,
    diagnosis: Option<Diagnosis>,
  },
  Refused {
    diagnosis: Diagnosis,
    tried: Option<Remedy>,
    after_browser: bool,
    retry_after_ms: Option<u64>,
    elapsed_ms: u64,
  },
  Failed {
    reason: String,
    elapsed_ms: u64,
  },
}

const ENGINE_BYTE_CAP: usize = 3_000_000;

// A request that went round in circles is a cookie the engine wanted and never got.
fn redirect_loop(error: &WebError) -> bool {
  error.message.contains("Too many redirects")
}

enum Read {
  Done(Outcome),
  Refused(Diagnosis),
}

/// One engine asked once, and once more with a remedy if it refused: what it answered, or why not.
pub async fn ask_engine(
  engine: &dyn Engine,
  query: &EngineQuery<'_>,
  roads: &EngineRoads<'_>,
) -> WebResult<Outcome> {
  let clock = &*roads.check.clock;
  let store = roads.store;
  let mut recipe = store
    .recipe(engine.name())
    .await
    .map_err(store_failure)?
    .unwrap_or_else(|| fresh_recipe(engine, clock.now()));
  let jar = tokio::sync::Mutex::new(Jar::default());

  jar
    .lock()
    .await
    .hold(&store.cookies(engine.name(), clock.now()).await.map_err(store_failure)?, clock.now());

  wait_for_host(store, &engine_key(engine), roads.interval_ms.max(recipe.interval_ms), clock).await?;

  let started = clock.now();
  let elapsed = || clock.now().saturating_sub(started);
  let attempt = async |held: &Recipe| -> WebResult<EngineAnswer> {
    let asked = match held.route {
      Route::Alternate => engine.alternate().map_or_else(|| engine.request(query), |build| build(query)),
      _ => engine.request(query),
    };

    if held.route == Route::Browser {
      let Some(render) = &roads.render else {
        return Err(WebError::new(
          format!("No real browser on this road for {}", engine.name()),
          Failure::BrowserUnavailable,
        ));
      };
      let page = render(asked.clone()).await?;

      return Ok(EngineAnswer {
        requested_url: asked,
        url: page.url,
        status: page.status,
        content_type: page.content_type,
        body: page.html,
        retry_after_ms: None,
      });
    }

    let mut direct = Direct::new(store, &jar, roads.check, roads.transport);

    direct.identity = identity::named(Some(&held.identity));
    direct.byte_cap = ENGINE_BYTE_CAP;

    if held.warm_up
      && let Some(warm) = engine.warm_up()
      && let Ok(warm_url) = Url::parse(warm)
      && jar.lock().await.header(&warm_url, clock.now()).is_none()
    {
      // A warm-up that fails leaves the request to answer for itself.
      let _ = direct.document(warm).await;
    }

    let page = direct.document(&asked).await?;

    Ok(EngineAnswer {
      requested_url: asked,
      url: page.url,
      status: page.status,
      content_type: page.content_type,
      body: page.body,
      retry_after_ms: page.retry_after_ms,
    })
  };
  // An answer that is neither a refusal nor results is a failure of the ordinary kind.
  let read = |answer: &EngineAnswer| -> Read {
    if let Some(diagnosis) = diagnose(engine, answer) {
      return Read::Refused(diagnosis);
    }

    if answer.status >= 400 {
      return Read::Done(Outcome::Failed {
        reason: format!("HTTP {}", answer.status),
        elapsed_ms: elapsed(),
      });
    }

    Read::Done(Outcome::Answered {
      hits: engine.parse(&answer.body),
      elapsed_ms: elapsed(),
      remedy: None,
      diagnosis: None,
    })
  };
  let failure = |error: &WebError| Outcome::Failed {
    reason: error.message.clone(),
    elapsed_ms: elapsed(),
  };
  let keep_cookies = async || -> WebResult<()> {
    let held = jar.lock().await.held(clock.now());

    store.set_cookies(engine.name(), held, clock.now()).await.map_err(store_failure)
  };
  let mut retry_after_ms = None;
  let first = match attempt(&recipe).await {
    Ok(answer) => {
      retry_after_ms = answer.retry_after_ms;
      read(&answer)
    }
    Err(error) if redirect_loop(&error) => Read::Refused(Diagnosis {
      cause: Cause::Cookie,
      evidence: format!("redirected in a loop: {}", error.message),
    }),
    Err(error) => {
      keep_cookies().await?;
      return Ok(failure(&error));
    }
  };
  let diagnosis = match first {
    Read::Done(outcome) => {
      keep_cookies().await?;
      return Ok(outcome);
    }
    Read::Refused(diagnosis) => diagnosis,
  };
  let Some(remedy) = remedy_for(diagnosis.cause, &recipe, engine, roads.render.is_some()) else {
    keep_cookies().await?;
    return Ok(Outcome::Refused {
      after_browser: recipe.route == Route::Browser,
      diagnosis,
      tried: None,
      retry_after_ms,
      elapsed_ms: elapsed(),
    });
  };

  // The remedy stays in the recipe whether or not the retry answers, so a block that comes back moves on to
  // the next remedy rather than trying this one again.
  recipe = with_remedy(&recipe, remedy, &diagnosis, roads.interval_ms, clock.now());
  store.set_recipe(recipe.clone()).await.map_err(store_failure)?;

  if remedy == Remedy::Reidentify {
    jar.lock().await.clear();
  }

  if remedy == Remedy::Slow {
    let now = clock.now();

    store
      .defer(&engine_key(engine), now + recipe.interval_ms.max(retry_after_ms.unwrap_or(0)), now)
      .await
      .map_err(store_failure)?;

    // The engine's own Retry-After is waited out, however long it is, before the one retry.
    wait_for_host(store, &engine_key(engine), recipe.interval_ms, clock).await?;
  }

  let second = match attempt(&recipe).await {
    Ok(answer) => {
      retry_after_ms = answer.retry_after_ms.or(retry_after_ms);
      read(&answer)
    }
    Err(error) => {
      keep_cookies().await?;

      if redirect_loop(&error) {
        return Ok(Outcome::Refused {
          diagnosis: Diagnosis {
            cause: Cause::Cookie,
            evidence: error.message.clone(),
          },
          tried: Some(remedy),
          after_browser: recipe.route == Route::Browser,
          retry_after_ms,
          elapsed_ms: elapsed(),
        });
      }

      return Ok(failure(&error));
    }
  };

  keep_cookies().await?;

  Ok(match second {
    Read::Done(Outcome::Answered {
      hits, elapsed_ms, ..
    }) => Outcome::Answered {
      hits,
      elapsed_ms,
      remedy: Some(remedy),
      diagnosis: Some(diagnosis),
    },
    Read::Done(outcome) => outcome,
    Read::Refused(again) => Outcome::Refused {
      diagnosis: again,
      tried: Some(remedy),
      after_browser: recipe.route == Route::Browser,
      retry_after_ms,
      elapsed_ms: elapsed(),
    },
  })
}

// ── Reading an engine's page ──

/// What one field of a result is read from: an element's text, or one of its attributes.
pub struct FieldSpec {
  pub selector: &'static str,
  pub attribute: Option<&'static str>,
}

pub const fn text_of(selector: &'static str) -> FieldSpec {
  FieldSpec {
    selector,
    attribute: None,
  }
}

pub const fn attribute_of(selector: &'static str, attribute: &'static str) -> FieldSpec {
  FieldSpec {
    selector,
    attribute: Some(attribute),
  }
}

/// Every element `item` matches, read into the fields named: a field is read from the first element inside
/// the item that matches it, and an item nested in another is read as an item of its own.
pub fn collect_items(
  html: &str,
  item: &str,
  fields: &[(&'static str, FieldSpec)],
) -> Vec<BTreeMap<&'static str, String>> {
  let page = Html::parse_document(html);
  let Ok(items) = Selector::parse(item) else {
    return Vec::new();
  };
  let selectors: Vec<(&'static str, Option<Selector>, Option<&'static str>)> = fields
    .iter()
    .map(|(name, field)| (*name, Selector::parse(field.selector).ok(), field.attribute))
    .collect();

  page
    .select(&items)
    .map(|element| {
      let mut values = BTreeMap::new();

      for (name, selector, attribute) in &selectors {
        let Some(selector) = selector else {
          continue;
        };

        match attribute {
          Some(attribute) => {
            if let Some(value) = element.select(selector).find_map(|found| found.value().attr(attribute)) {
              values.insert(*name, value.trim().to_owned());
            }
          }
          None => {
            if let Some(found) = element.select(selector).next() {
              values.insert(*name, squeeze(&found.text().collect::<String>()));
            }
          }
        }
      }

      values
    })
    .collect()
}

// Text with its runs of whitespace made one space, trimmed, as the search's answer squeezes a snippet.
pub use crate::forms::text::squeeze;

fn web_address(url: &str) -> bool {
  let lower = url.to_lowercase();

  lower.starts_with("http:") || lower.starts_with("https:")
}

/// Results read off a page's items, each needing a title and an address that is a web address, and never
/// the same address twice.
pub fn hits_of(
  records: &[BTreeMap<&'static str, String>],
  base: &str,
  need_snippet: bool,
) -> Vec<EngineHit> {
  let base = Url::parse(base).ok();
  let mut seen = HashSet::new();
  let mut hits = Vec::new();

  for record in records {
    let title = record.get("title").cloned().unwrap_or_default();
    let snippet = record.get("snippet").cloned().unwrap_or_default();
    let raw = record.get("url").cloned().unwrap_or_default();
    let Some(url) = base.as_ref().and_then(|base| base.join(&raw).ok()).map(|url| url.to_string()) else {
      continue;
    };

    if title.is_empty() || !web_address(&url) || seen.contains(&url) || (need_snippet && snippet.is_empty())
    {
      continue;
    }

    seen.insert(url.clone());
    hits.push(EngineHit {
      title,
      url,
      snippet,
    });
  }

  hits
}

/// Results as an engine's API listed them, each needing a title and a web address, and never the same
/// address twice.
pub fn kept_hits(hits: Vec<EngineHit>) -> Vec<EngineHit> {
  let mut seen = HashSet::new();

  hits
    .into_iter()
    .filter(|hit| !hit.title.is_empty() && web_address(&hit.url) && seen.insert(hit.url.clone()))
    .collect()
}

/// A JSON value's text: a string read as the text of the markup it may carry, nothing for anything else.
pub fn text(value: &Value) -> String {
  match value.as_str() {
    Some(markup) => squeeze(&Html::parse_fragment(markup).root_element().text().collect::<String>()),
    None => String::new(),
  }
}

/// A JSON value's string as it stands, nothing for anything else.
pub fn string(value: &Value) -> String {
  value.as_str().unwrap_or_default().to_owned()
}

/// An element's text, its whitespace squeezed.
pub fn element_text(element: ElementRef<'_>) -> String {
  squeeze(&element.text().collect::<String>())
}
