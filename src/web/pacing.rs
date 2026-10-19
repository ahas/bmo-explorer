//! What the web layer keeps across processes in this repository's store (docs/web-access.md, 「Pacing」 and
//! 「A block: diagnosis, remedy, one retry」; docs/store.md): each host's and each engine's next slot, each
//! engine's health and recipe, and each engine's own cookie jar. One trait over the store's tables, and a
//! store in memory for a caller with no data root and for tests.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;

use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::{Connection, Row};

use crate::web::clock::Clock;
use crate::web::error::{Failure, WebError, WebResult};
use crate::web::jar::StoredCookie;

// What an engine was last seen to do is one of the web's forms, kept here and read from the store.
pub use crate::forms::engine::{EngineHealth, EngineState};

pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// The road an engine is asked by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
  Direct,
  Alternate,
  Browser,
}

impl Route {
  pub fn name(self) -> &'static str {
    match self {
      Self::Direct => "direct",
      Self::Alternate => "alternate",
      Self::Browser => "browser",
    }
  }

  pub fn named(name: &str) -> Option<Self> {
    [Self::Direct, Self::Alternate, Self::Browser]
      .into_iter()
      .find(|one| one.name() == name)
  }
}

/// How an engine is asked, as the last block that was cleared left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
  pub engine: String,
  pub identity: String,
  pub interval_ms: u64,
  pub route: Route,
  pub warm_up: bool,
  pub cause: Option<String>,
  pub evidence: Option<String>,
  pub set_at: u64,
}

/// However long a cookie asks to live, an engine's jar keeps it no longer than this.
pub const ENGINE_COOKIE_LIFETIME_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

pub trait Pacing: Send + Sync {
  /// The next slot of `key` at least `interval_ms` after the last one handed out, taken; none where it
  /// would fall after the deadline, and then nothing is taken.
  fn reserve(&self, key: &str, interval_ms: u64, deadline_ms: u64, now: u64) -> Boxed<'_, Option<u64>>;
  /// `key`'s next slot pushed back to `until_ms`, where it stood earlier.
  fn defer(&self, key: &str, until_ms: u64, now: u64) -> Boxed<'_, ()>;
  fn health(&self, engine: &str) -> Boxed<'_, Option<EngineHealth>>;
  fn set_health(&self, health: EngineHealth) -> Boxed<'_, ()>;
  fn recipe(&self, engine: &str) -> Boxed<'_, Option<Recipe>>;
  fn set_recipe(&self, recipe: Recipe) -> Boxed<'_, ()>;
  /// The engine's cookies still alive at `now`.
  fn cookies(&self, engine: &str, now: u64) -> Boxed<'_, Vec<StoredCookie>>;
  /// The engine's whole jar, replaced: each cookie kept to its own expiry or the jar's lifetime.
  fn set_cookies(&self, engine: &str, cookies: Vec<StoredCookie>, now: u64) -> Boxed<'_, ()>;
}

// The cookies a jar may keep, each held to the jar's lifetime; a malformed or spent one is left out, and
// of one host, path and name the later stands.
fn kept_cookies(cookies: Vec<StoredCookie>, now: u64) -> Vec<StoredCookie> {
  let limit = now + ENGINE_COOKIE_LIFETIME_MS;
  let mut kept: Vec<StoredCookie> = Vec::new();

  for mut cookie in cookies {
    let host_fits = !cookie.host.is_empty()
      && cookie.host.len() <= 253
      && cookie.host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let value_fits = cookie.value.len() <= 4_096 && !cookie.value.contains(['\0', '\r', '\n', ';']);

    if !host_fits
      || !cookie.path.starts_with('/')
      || cookie.path.len() > 1_000
      || !value_fits
      || cookie.name.len() > 256
    {
      continue;
    }

    cookie.host = cookie.host.to_lowercase();
    cookie.expires_at = Some(cookie.expires_at.unwrap_or(limit).min(limit));
    kept.retain(|one| !(one.host == cookie.host && one.path == cookie.path && one.name == cookie.name));

    if cookie.expires_at.is_some_and(|at| at > now) {
      kept.push(cookie);
    }
  }

  kept
}

/// The store in memory.
#[derive(Default)]
pub struct MemoryPacing {
  next: Mutex<HashMap<String, u64>>,
  health: Mutex<HashMap<String, EngineHealth>>,
  recipes: Mutex<HashMap<String, Recipe>>,
  jars: Mutex<HashMap<String, Vec<StoredCookie>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Pacing for MemoryPacing {
  fn reserve(&self, key: &str, interval_ms: u64, deadline_ms: u64, now: u64) -> Boxed<'_, Option<u64>> {
    let mut next = locked(&self.next);
    let at = next.get(key).copied().unwrap_or(now).max(now);
    let answer = (at <= deadline_ms).then(|| {
      next.insert(key.to_owned(), at + interval_ms);
      at
    });

    Box::pin(std::future::ready(Ok(answer)))
  }

  fn defer(&self, key: &str, until_ms: u64, now: u64) -> Boxed<'_, ()> {
    let mut next = locked(&self.next);
    let at = next.get(key).copied().unwrap_or(now).max(until_ms);

    next.insert(key.to_owned(), at);
    Box::pin(std::future::ready(Ok(())))
  }

  fn health(&self, engine: &str) -> Boxed<'_, Option<EngineHealth>> {
    let found = locked(&self.health).get(engine).cloned();

    Box::pin(std::future::ready(Ok(found)))
  }

  fn set_health(&self, health: EngineHealth) -> Boxed<'_, ()> {
    locked(&self.health).insert(health.engine.clone(), health);
    Box::pin(std::future::ready(Ok(())))
  }

  fn recipe(&self, engine: &str) -> Boxed<'_, Option<Recipe>> {
    let found = locked(&self.recipes).get(engine).cloned();

    Box::pin(std::future::ready(Ok(found)))
  }

  fn set_recipe(&self, recipe: Recipe) -> Boxed<'_, ()> {
    locked(&self.recipes).insert(recipe.engine.clone(), recipe);
    Box::pin(std::future::ready(Ok(())))
  }

  fn cookies(&self, engine: &str, now: u64) -> Boxed<'_, Vec<StoredCookie>> {
    let alive = locked(&self.jars)
      .get(engine)
      .map(|jar| jar.iter().filter(|one| one.expires_at.is_none_or(|at| at > now)).cloned().collect())
      .unwrap_or_default();

    Box::pin(std::future::ready(Ok(alive)))
  }

  fn set_cookies(&self, engine: &str, cookies: Vec<StoredCookie>, now: u64) -> Boxed<'_, ()> {
    locked(&self.jars).insert(engine.to_owned(), kept_cookies(cookies, now));
    Box::pin(std::future::ready(Ok(())))
  }
}

/// The store over the tables of this repository's store at its place under the data root (docs/store.md):
/// each call opens its own connection, so a store held for a session's whole life holds none between calls.
pub struct StorePacing {
  place: PathBuf,
}

impl StorePacing {
  pub fn new(place: impl Into<PathBuf>) -> Self {
    Self {
      place: place.into(),
    }
  }

  async fn open(&self) -> Result<SqliteConnection, String> {
    crate::store::open(&self.place).await
  }
}

fn whole(value: u64) -> i64 {
  i64::try_from(value).unwrap_or(i64::MAX)
}

fn stored(row: &SqliteRow, column: &str) -> Option<u64> {
  row.try_get::<Option<i64>, _>(column).ok().flatten().map(|value| value.max(0) as u64)
}

fn failed(why: sqlx::Error) -> String {
  why.to_string()
}

impl Pacing for StorePacing {
  fn reserve(&self, key: &str, interval_ms: u64, deadline_ms: u64, now: u64) -> Boxed<'_, Option<u64>> {
    let key = key.to_owned();

    Box::pin(async move {
      let mut store = self.open().await?;

      // The row stands first, so the one statement after it takes the slot or leaves it: two processes
      // asking at once are handed two slots, never the same one.
      sqlx::query(
        "INSERT INTO web_host_pacing (key, next_at, updated_at) VALUES ($1, 0, $2) ON CONFLICT (key) DO NOTHING",
      )
      .bind(&key)
      .bind(whole(now))
      .execute(&mut store)
      .await
      .map_err(failed)?;

      let taken = sqlx::query(
        "UPDATE web_host_pacing \
         SET next_at = (CASE WHEN next_at > $2 THEN next_at ELSE $2 END) + $3, updated_at = $2 \
         WHERE key = $1 AND (CASE WHEN next_at > $2 THEN next_at ELSE $2 END) <= $4 \
         RETURNING next_at",
      )
      .bind(&key)
      .bind(whole(now))
      .bind(whole(interval_ms))
      .bind(whole(deadline_ms))
      .fetch_optional(&mut store)
      .await
      .map_err(failed)?;

      Ok(
        taken
          .and_then(|row| stored(&row, "next_at"))
          .map(|next| next.saturating_sub(interval_ms)),
      )
    })
  }

  fn defer(&self, key: &str, until_ms: u64, now: u64) -> Boxed<'_, ()> {
    let key = key.to_owned();

    Box::pin(async move {
      let mut store = self.open().await?;

      sqlx::query(
        "INSERT INTO web_host_pacing (key, next_at, updated_at) VALUES ($1, $2, $3) \
         ON CONFLICT (key) DO UPDATE SET \
           next_at = CASE WHEN web_host_pacing.next_at > excluded.next_at THEN web_host_pacing.next_at ELSE excluded.next_at END, \
           updated_at = excluded.updated_at",
      )
      .bind(&key)
      .bind(whole(until_ms))
      .bind(whole(now))
      .execute(&mut store)
      .await
      .map_err(failed)?;

      Ok(())
    })
  }

  fn health(&self, engine: &str) -> Boxed<'_, Option<EngineHealth>> {
    let engine = engine.to_owned();

    Box::pin(async move {
      let mut store = self.open().await?;
      let row = sqlx::query(
        "SELECT state, reason, observed_at, suspended_until, elapsed_ms, result_count FROM web_engine_health WHERE engine = $1",
      )
      .bind(&engine)
      .fetch_optional(&mut store)
      .await
      .map_err(failed)?;

      row
        .map(|row| {
          let state: String = row.try_get("state").map_err(failed)?;

          Ok(EngineHealth {
            engine: engine.clone(),
            state: EngineState::named(&state)
              .ok_or_else(|| format!("Invalid stored web engine state: {state}"))?,
            reason: row.try_get("reason").map_err(failed)?,
            observed_at: stored(&row, "observed_at"),
            suspended_until: stored(&row, "suspended_until"),
            elapsed_ms: stored(&row, "elapsed_ms"),
            result_count: stored(&row, "result_count"),
          })
        })
        .transpose()
    })
  }

  fn set_health(&self, health: EngineHealth) -> Boxed<'_, ()> {
    Box::pin(async move {
      let mut store = self.open().await?;

      sqlx::query(
        "INSERT INTO web_engine_health (engine, state, reason, observed_at, suspended_until, elapsed_ms, result_count) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (engine) DO UPDATE SET state = excluded.state, reason = excluded.reason, \
           observed_at = excluded.observed_at, suspended_until = excluded.suspended_until, \
           elapsed_ms = excluded.elapsed_ms, result_count = excluded.result_count",
      )
      .bind(&health.engine)
      .bind(health.state.name())
      .bind(health.reason.as_ref().map(|reason| reason.chars().take(500).collect::<String>()))
      .bind(health.observed_at.map(whole))
      .bind(health.suspended_until.map(whole))
      .bind(health.elapsed_ms.map(whole))
      .bind(health.result_count.map(whole))
      .execute(&mut store)
      .await
      .map_err(failed)?;

      Ok(())
    })
  }

  fn recipe(&self, engine: &str) -> Boxed<'_, Option<Recipe>> {
    let engine = engine.to_owned();

    Box::pin(async move {
      let mut store = self.open().await?;
      let row = sqlx::query(
        "SELECT identity, interval_ms, route, warm_up, cause, evidence, set_at FROM web_engine_recipe WHERE engine = $1",
      )
      .bind(&engine)
      .fetch_optional(&mut store)
      .await
      .map_err(failed)?;

      row
        .map(|row| {
          let route: String = row.try_get("route").map_err(failed)?;

          Ok(Recipe {
            engine: engine.clone(),
            identity: row.try_get("identity").map_err(failed)?,
            interval_ms: stored(&row, "interval_ms").unwrap_or(0),
            route: Route::named(&route)
              .ok_or_else(|| format!("Invalid stored web engine route: {route}"))?,
            warm_up: stored(&row, "warm_up") == Some(1),
            cause: row.try_get("cause").map_err(failed)?,
            evidence: row.try_get("evidence").map_err(failed)?,
            set_at: stored(&row, "set_at").unwrap_or(0),
          })
        })
        .transpose()
    })
  }

  fn set_recipe(&self, recipe: Recipe) -> Boxed<'_, ()> {
    Box::pin(async move {
      let mut store = self.open().await?;

      sqlx::query(
        "INSERT INTO web_engine_recipe (engine, identity, interval_ms, route, warm_up, cause, evidence, set_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (engine) DO UPDATE SET identity = excluded.identity, interval_ms = excluded.interval_ms, \
           route = excluded.route, warm_up = excluded.warm_up, cause = excluded.cause, \
           evidence = excluded.evidence, set_at = excluded.set_at",
      )
      .bind(&recipe.engine)
      .bind(&recipe.identity)
      .bind(whole(recipe.interval_ms))
      .bind(recipe.route.name())
      .bind(i64::from(recipe.warm_up))
      .bind(&recipe.cause)
      .bind(recipe.evidence.as_ref().map(|evidence| evidence.chars().take(1_000).collect::<String>()))
      .bind(whole(recipe.set_at))
      .execute(&mut store)
      .await
      .map_err(failed)?;

      Ok(())
    })
  }

  fn cookies(&self, engine: &str, now: u64) -> Boxed<'_, Vec<StoredCookie>> {
    let engine = engine.to_owned();

    Box::pin(async move {
      let mut store = self.open().await?;
      let rows = sqlx::query(
        "SELECT host, path, name, value, secure, expires_at FROM web_engine_cookie \
         WHERE engine = $1 AND expires_at > $2 ORDER BY host, path, name",
      )
      .bind(&engine)
      .bind(whole(now))
      .fetch_all(&mut store)
      .await
      .map_err(failed)?;

      rows
        .iter()
        .map(|row| {
          Ok(StoredCookie {
            host: row.try_get("host").map_err(failed)?,
            path: row.try_get("path").map_err(failed)?,
            name: row.try_get("name").map_err(failed)?,
            value: row.try_get("value").map_err(failed)?,
            secure: stored(row, "secure") == Some(1),
            expires_at: stored(row, "expires_at"),
          })
        })
        .collect()
    })
  }

  fn set_cookies(&self, engine: &str, cookies: Vec<StoredCookie>, now: u64) -> Boxed<'_, ()> {
    let engine = engine.to_owned();
    let kept = kept_cookies(cookies, now);

    Box::pin(async move {
      let mut store = self.open().await?;
      let mut writing = store.begin().await.map_err(failed)?;

      sqlx::query("DELETE FROM web_engine_cookie WHERE engine = $1")
        .bind(&engine)
        .execute(&mut *writing)
        .await
        .map_err(failed)?;

      for cookie in &kept {
        sqlx::query(
          "INSERT INTO web_engine_cookie (engine, host, path, name, value, secure, expires_at) \
           VALUES ($1, $2, $3, $4, $5, $6, $7) \
           ON CONFLICT (engine, host, path, name) DO UPDATE SET value = excluded.value, \
             secure = excluded.secure, expires_at = excluded.expires_at",
        )
        .bind(&engine)
        .bind(&cookie.host)
        .bind(&cookie.path)
        .bind(&cookie.name)
        .bind(&cookie.value)
        .bind(i64::from(cookie.secure))
        .bind(cookie.expires_at.map_or(0, whole))
        .execute(&mut *writing)
        .await
        .map_err(failed)?;
      }

      writing.commit().await.map_err(failed)
    })
  }
}

/// A store's failure, as the read it stood under fails with.
pub fn store_failure(why: String) -> WebError {
  WebError::new(format!("the web pacing store failed: {why}"), Failure::Transport)
}

/// Waits for `key`'s next slot under the deadline, and answers when it came; a slot that would come after
/// the deadline is not waited for (docs/web-access.md, 「Pacing」).
pub async fn wait_for_host(
  store: &dyn Pacing,
  key: &str,
  interval_ms: u64,
  deadline_ms: u64,
  clock: &dyn Clock,
) -> WebResult<u64> {
  let late = || {
    WebError::new(format!("Host pacing for {key} would start after the web deadline"), Failure::Deadline)
  };
  let at = store
    .reserve(key, interval_ms, deadline_ms, clock.now())
    .await
    .map_err(store_failure)?
    .ok_or_else(late)?;
  let now = clock.now();

  if at > now {
    if at > deadline_ms {
      return Err(late());
    }

    clock.sleep(at - now).await;
  }

  Ok(at)
}
