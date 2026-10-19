//! The web search (docs/web-access.md, 「Search engines」): every engine of the asked category asked through
//! its own road, the Korean-native ones first for a Korean query, the answers measured against the query and
//! mixed one from each engine in turn, and every engine's health written down.

pub mod engine;
pub mod engines;
pub mod roster;

use std::collections::HashSet;
use std::sync::Arc;

use crate::web::address::Check;
use crate::web::client::Transport;
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::pacing::{EngineHealth, EngineState, Pacing, store_failure};
use engine::{Engine, EngineQuery, EngineRoads, Outcome, Render, ask_engine, rest_ms};

// The answer a search hands on, its counts and its rendering stand in the web's forms crate, below this layer
// and the site contract, so a site tool reading through the engines answers as the search does.
pub use crate::forms::search::{
  DEFAULT_SEARCH_COUNT, MAX_SEARCH_COUNT, SearchHit, SearchResults, render_search_results,
};
pub const SEARCH_ENGINE_INTERVAL_MS: u64 = 2_000;

/// Every engine, in the order the roster of the shared policy names them. Adding an engine is writing its
/// module and naming it here; nothing else anywhere is told about it.
pub const ENGINES: [&dyn Engine; 13] = [
  &engines::NAVER,
  &engines::DAUM,
  &engines::DuckDuckGo,
  &engines::ZapMeta,
  &engines::Yandex,
  &engines::NAVER_NEWS,
  &engines::DAUM_NEWS,
  &engines::BingNews,
  &engines::Openverse,
  &engines::WikimediaCommons,
  &engines::Unsplash,
  &engines::GitHub,
  &engines::StackOverflow,
];

/// The engines of one category, in roster order.
pub fn engines_for(category: &str) -> Vec<&'static dyn Engine> {
  ENGINES
    .iter()
    .copied()
    .filter(|one| engine::declared(one.name()).category == category)
    .collect()
}

/// One search asked.
pub struct Search<'a> {
  pub query: &'a str,
  pub count: Option<usize>,
  pub language: Option<&'a str>,
  pub category: Option<&'a str>,
  pub deadline_ms: Option<u64>,
  pub engine_interval_ms: Option<u64>,
}

/// What a search is asked through.
pub struct SearchRoads<'a> {
  pub store: &'a dyn Pacing,
  pub check: &'a Check,
  pub transport: &'a dyn Transport,
  pub render: Option<Render>,
}

fn cjk_or_hangul(c: char) -> bool {
  matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}')
}

/// The terms of the query an answer would have to carry: the query cut at everything that is not a letter or
/// a digit, each term looked for as it stands — Latin runs of three characters and more, Korean, Japanese and
/// Chinese words of two and more.
pub fn search_terms(query: &str) -> Vec<String> {
  let mut seen = HashSet::new();

  query
    .to_lowercase()
    .split(|c: char| !c.is_alphanumeric())
    .filter(|term| !term.is_empty())
    .filter(|term| {
      if term.chars().any(cjk_or_hangul) {
        term.chars().count() >= 2
      } else {
        term.chars().count() >= 3
      }
    })
    .filter(|term| seen.insert(term.to_string()))
    .map(str::to_owned)
    .collect()
}

/// Whether an engine answered something other than what it was asked: fewer than half the query's terms
/// carried anywhere in its answer, over a query of two terms and more. The words that decided it, or nothing.
pub fn off_topic_answer(query: &str, hits: &[SearchHit]) -> Option<String> {
  let terms = search_terms(query);

  if terms.len() < 2 || hits.is_empty() {
    return None;
  }

  let haystacks: Vec<String> = hits
    .iter()
    .map(|hit| format!("{} {} {}", hit.title, hit.url, hit.snippet).to_lowercase())
    .collect();
  let carried = terms
    .iter()
    .filter(|term| haystacks.iter().any(|text| text.contains(term.as_str())))
    .count();

  (carried * 2 < terms.len()).then(|| {
    format!(
      "answered {} results carrying {carried} of the query's {} terms",
      hits.len(),
      terms.len()
    )
  })
}

fn search_language(query: &str, asked: Option<&str>) -> String {
  match asked {
    Some(asked) if !asked.trim().is_empty() => asked.to_owned(),
    _ if query.chars().any(|c| matches!(c, '가'..='힣' | 'ㄱ'..='ㅎ' | 'ㅏ'..='ㅣ')) => {
      "ko-KR".into()
    }
    _ => "en".into(),
  }
}

/// Korean-native engines are asked first for a Korean query and last for any other, so the engine best
/// placed to answer spends the search's time first.
pub fn asking_order<T: Copy>(engines: &[T], korean_native: impl Fn(T) -> bool, language: &str) -> Vec<T> {
  let korean = language.to_lowercase().starts_with("ko");
  let native: Vec<T> = engines.iter().copied().filter(|one| korean_native(*one)).collect();
  let rest: Vec<T> = engines.iter().copied().filter(|one| !korean_native(*one)).collect();

  if korean { [native, rest].concat() } else { [rest, native].concat() }
}

/// One hit from each answering engine in turn, so one engine's first page never pushes every other engine's
/// answer past the count.
pub fn interleave_hits(answered: &[Vec<SearchHit>], count: usize) -> Vec<SearchHit> {
  let mut hits = Vec::new();
  let mut seen = HashSet::new();
  let mut rank = 0;

  while hits.len() < count && answered.iter().any(|list| rank < list.len()) {
    for list in answered {
      let Some(one) = list.get(rank) else {
        continue;
      };

      if !seen.insert(one.url.clone()) {
        continue;
      }

      hits.push(one.clone());

      if hits.len() == count {
        break;
      }
    }

    rank += 1;
  }

  hits
}

async fn observed(store: &dyn Pacing, health: EngineHealth) -> WebResult<EngineHealth> {
  let health = EngineHealth {
    reason: health.reason.map(|reason| reason.chars().take(500).collect()),
    ..health
  };

  store.set_health(health.clone()).await.map_err(store_failure)?;
  Ok(health)
}

// An instant written as ISO 8601 in UTC, to the millisecond, as a resting engine's end is named.
fn iso(ms: u64) -> String {
  chrono::DateTime::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX))
    .map_or_else(String::new, |at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// One search: every engine of the category asked, and what came of each.
pub async fn search_web(search: &Search<'_>, roads: &SearchRoads<'_>) -> WebResult<SearchResults> {
  let clock = &*roads.check.clock;
  let count = search.count.unwrap_or(DEFAULT_SEARCH_COUNT).clamp(1, MAX_SEARCH_COUNT);
  let category = search
    .category
    .map(str::trim)
    .filter(|category| !category.is_empty())
    .unwrap_or("general");
  let language = search_language(search.query, search.language);
  let roster = engines_for(category);

  if roster.is_empty() {
    return Err(WebError::new(format!("Unsupported search category: {category}"), Failure::Transport));
  }

  let deadline_ms = search
    .deadline_ms
    .unwrap_or(u64::MAX)
    .min(clock.now() + crate::web::session::FETCH_TIMEOUT_MS);
  let engine_roads = EngineRoads {
    store: roads.store,
    interval_ms: search.engine_interval_ms.unwrap_or(SEARCH_ENGINE_INTERVAL_MS),
    deadline_ms,
    check: roads.check,
    transport: roads.transport,
    render: roads.render.clone(),
  };
  let mut answered: Vec<Vec<SearchHit>> = Vec::new();
  let mut unresponsive: Vec<String> = Vec::new();
  let mut off_topic: Vec<String> = Vec::new();
  let mut health: Vec<EngineHealth> = Vec::new();
  let health_of =
    |engine: &str, state, reason: Option<String>, elapsed_ms, result_count, suspended_until| EngineHealth {
      engine: engine.to_owned(),
      state,
      reason,
      observed_at: Some(clock.now()),
      suspended_until,
      elapsed_ms,
      result_count,
    };

  for engine in asking_order(&roster, |one| engine::declared(one.name()).korean_native, &language) {
    let name = engine.name();
    let prior = roads
      .store
      .health(name)
      .await
      .map_err(store_failure)?
      .unwrap_or_else(|| EngineHealth::unknown(name));

    if prior.state == EngineState::Suspended
      && let Some(until) = prior.suspended_until.filter(|until| *until > clock.now())
    {
      unresponsive.push(format!(
        "{name} (resting until {} after {})",
        iso(until),
        prior.reason.as_deref().unwrap_or("a failure")
      ));
      health.push(prior);
      continue;
    }

    let query = EngineQuery {
      query: search.query,
      language: &language,
      count: MAX_SEARCH_COUNT,
    };

    match ask_engine(engine, &query, &engine_roads).await? {
      Outcome::Late => {
        let reason = "not started before the search deadline";

        health.push(
          observed(
            roads.store,
            health_of(name, EngineState::Unknown, Some(reason.into()), None, None, None),
          )
          .await?,
        );
        unresponsive.push(format!("{name} ({reason})"));
      }
      Outcome::Failed {
        reason,
        timed_out,
        elapsed_ms,
      } => {
        // The caller's own deadline is no evidence against the engine; anything else rests it briefly.
        let seen = if timed_out {
          health_of(name, EngineState::Unknown, Some(reason.clone()), Some(elapsed_ms), None, None)
        } else {
          health_of(
            name,
            EngineState::Suspended,
            Some(reason.clone()),
            Some(elapsed_ms),
            None,
            Some(clock.now() + rest_ms(None, false)),
          )
        };

        health.push(observed(roads.store, seen).await?);
        unresponsive.push(format!("{name} ({reason})"));

        if timed_out {
          break;
        }
      }
      Outcome::Refused {
        diagnosis,
        tried,
        after_browser,
        retry_after_ms,
        elapsed_ms,
      } => {
        let reason = format!(
          "{}: {}; tried {}",
          diagnosis.cause.name(),
          diagnosis.evidence,
          tried.map_or("nothing — no remedy on this road", |remedy| remedy.name())
        );
        let until =
          clock.now() + rest_ms(Some(diagnosis.cause), after_browser).max(retry_after_ms.unwrap_or(0));

        health.push(
          observed(
            roads.store,
            health_of(
              name,
              EngineState::Suspended,
              Some(reason.clone()),
              Some(elapsed_ms),
              Some(0),
              Some(until),
            ),
          )
          .await?,
        );
        unresponsive.push(format!("{name} ({reason})"));
      }
      Outcome::Answered {
        hits,
        elapsed_ms,
        remedy,
        diagnosis,
      } => {
        let found: Vec<SearchHit> = hits
          .into_iter()
          .map(|hit| SearchHit {
            title: hit.title,
            url: hit.url,
            snippet: hit.snippet,
            engine: name.to_owned(),
          })
          .collect();

        // An engine answering something other than what it was asked is kept out of the merged list: its pages
        // are not partial results, they are another query's.
        if let Some(wandered) = off_topic_answer(search.query, &found) {
          health.push(
            observed(
              roads.store,
              health_of(
                name,
                EngineState::Degraded,
                Some(wandered.clone()),
                Some(elapsed_ms),
                Some(found.len() as u64),
                None,
              ),
            )
            .await?,
          );
          off_topic.push(format!("{name} ({wandered})"));
          continue;
        }

        let cleared = match (remedy, diagnosis) {
          (Some(remedy), Some(diagnosis)) => {
            Some(format!("answered after {} for {}", remedy.name(), diagnosis.cause.name()))
          }
          _ => None,
        };

        health.push(
          observed(
            roads.store,
            health_of(
              name,
              EngineState::Healthy,
              cleared,
              Some(elapsed_ms),
              Some(found.len() as u64),
              None,
            ),
          )
          .await?,
        );
        answered.push(found);
      }
    }
  }

  let mut seen = HashSet::new();

  unresponsive.retain(|one| seen.insert(one.clone()));
  seen.clear();
  off_topic.retain(|one| seen.insert(one.clone()));

  Ok(SearchResults {
    hits: interleave_hits(&answered, count),
    unresponsive,
    off_topic,
    health,
  })
}

/// The render road a search lends its engines, out of a session.
pub fn render_through(session: Arc<crate::web::session::WebSession>) -> Render {
  Arc::new(move |url: String, deadline_ms: u64| {
    let session = session.clone();

    Box::pin(async move {
      session
        .render(&url, deadline_ms, false, crate::web::chrome::FetchOptions::default())
        .await
    })
  })
}
