//! The `explorer` entries (docs/api.md, 「The explorer entries」): what a caller of the tool host reads of the
//! web — a raster picture, a page's head and the search engines' states — through the web layer's checks, so
//! every outward request keeps them. Each is for the system alone: never shown to a session, and reached by the
//! tool host's `POST /tool-calls`.

use base64::Engine;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{ImageSearchOutput, Licence, Picture, engine_standing, is_markup, page_head};
use crate::store;
use crate::tool::{Answer, Content, Hints, Served, Session, Tool};
use crate::web::address::Check;
use crate::web::client::{decode, network};
use crate::web::jar::StoredCookie;
use crate::web::pacing::{Boxed, EngineHealth, Pacing, Recipe, StorePacing};
use crate::web::picture::fetch_raster;
use crate::web::search::{
  DEFAULT_SEARCH_COUNT, MAX_SEARCH_COUNT, Search, SearchRoads, image_engines_named, search_images,
};

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&FetchRaster, &FetchPage, &SearchEngines, &SearchImages];

// Each reads the web and changes nothing on this machine.
const READS: Hints = Hints {
  read_only: true,
  destructive: false,
  idempotent: None,
  open_world: true,
};

/// The store's pacing with its keys under one name, so the requests of one run's reading are paced together and
/// apart from every other's, across the processes its calls run in.
pub struct Paced {
  under: String,
  store: StorePacing,
}

impl Paced {
  pub fn new(under: &str, store: StorePacing) -> Self {
    Self {
      under: under.to_owned(),
      store,
    }
  }

  fn key(&self, key: &str) -> String {
    format!("{}/{key}", self.under)
  }
}

impl Pacing for Paced {
  fn reserve(&self, key: &str, interval_ms: u64, now: u64) -> Boxed<'_, u64> {
    let key = self.key(key);

    Box::pin(async move { self.store.reserve(&key, interval_ms, now).await })
  }

  fn defer(&self, key: &str, until_ms: u64, now: u64) -> Boxed<'_, ()> {
    let key = self.key(key);

    Box::pin(async move { self.store.defer(&key, until_ms, now).await })
  }

  fn health(&self, engine: &str) -> Boxed<'_, Option<EngineHealth>> {
    self.store.health(engine)
  }

  fn set_health(&self, health: EngineHealth) -> Boxed<'_, ()> {
    self.store.set_health(health)
  }

  fn recipe(&self, engine: &str) -> Boxed<'_, Option<Recipe>> {
    self.store.recipe(engine)
  }

  fn set_recipe(&self, recipe: Recipe) -> Boxed<'_, ()> {
    self.store.set_recipe(recipe)
  }

  fn cookies(&self, engine: &str, now: u64) -> Boxed<'_, Vec<StoredCookie>> {
    self.store.cookies(engine, now)
  }

  fn set_cookies(&self, engine: &str, cookies: Vec<StoredCookie>, now: u64) -> Boxed<'_, ()> {
    self.store.set_cookies(engine, cookies, now)
  }
}

// The store under the record's data root, or the refusal a call answers where the record names none.
fn store_of(session: &Session) -> Result<StorePacing, Answer> {
  session
    .data_root()
    .map(|root| StorePacing::new(store::place(&root)))
    .map_err(|why| Answer::refusal(format!("ERROR: {why}")))
}

fn paced(session: &Session, pacing: &str) -> Result<Paced, Answer> {
  store_of(session).map(|store| Paced::new(pacing, store))
}

// An answer's schema as the protocol wants it, the object's schema alone, as an input's is written: the draft it
// was written to and the Rust type's own name say nothing a caller reads.
fn schema<T: JsonSchema>() -> Value {
  let mut schema = serde_json::to_value(schemars::schema_for!(T)).expect("a schema is JSON");

  if let Value::Object(fields) = &mut schema {
    fields.remove("$schema");
    fields.remove("title");
  }

  schema
}

fn structured(object: &impl Serialize) -> Answer {
  match serde_json::to_value(object) {
    Ok(object) => Answer::structured(object),
    Err(why) => Answer::failure(format!("the answer could not be written: {why}")),
  }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PacedInput {
  /// The address.
  address: String,
  /// Whose requests are paced together: one name for each run's reading.
  pacing: String,
}

/// The raster picture behind an address.
pub struct FetchRaster;

impl Tool for FetchRaster {
  const NAME: &'static str = "fetch_raster";
  const NAMESPACE: &'static str = "explorer";
  const DESCRIPTION: &'static str = "Fetches the raster picture behind an address by the checked direct road, paced per host under the pacing name given, and answers it as one image.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = PacedInput;

  async fn call(&self, session: &Session, input: PacedInput) -> Answer {
    let pacing = match paced(session, &input.pacing) {
      Ok(pacing) => pacing,
      Err(refused) => return refused,
    };

    match fetch_raster(&input.address, &pacing, &Check::default(), &*network()).await {
      Ok(image) => Answer {
        content: vec![Content::Image {
          data: base64::engine::general_purpose::STANDARD.encode(&image.bytes),
          mime_type: image.kind.to_owned(),
        }],
        is_error: false,
        structured: None,
      },
      Err(why) => Answer::refusal(format!("ERROR: {}", why.message)),
    }
  }
}

/// A page's head as it was fetched.
#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageHeadOutput {
  /// The address after every redirect.
  pub url: String,
  /// The content type as the server sent it.
  #[serde(rename = "type")]
  pub kind: String,
  /// The page decoded as its charset says, where it is markup; null otherwise.
  pub html: Option<String>,
}

/// The head of a page, for the preview of a document that cites it.
pub struct FetchPage;

impl Tool for FetchPage {
  const NAME: &'static str = "fetch_page";
  const NAMESPACE: &'static str = "explorer";
  const DESCRIPTION: &'static str = "Fetches a page directly, checking the address and each redirect, and reads at most 512 KiB of it: the address it ended at, its content type, and its markup decoded as its charset says.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = PacedInput;

  fn output_schema() -> Option<Value> {
    Some(schema::<PageHeadOutput>())
  }

  async fn call(&self, session: &Session, input: PacedInput) -> Answer {
    let pacing = match paced(session, &input.pacing) {
      Ok(pacing) => pacing,
      Err(refused) => return refused,
    };
    let read = match page_head(&input.address, &pacing, &Check::default(), &*network()).await {
      Ok(read) => read,
      Err(why) => return Answer::refusal(format!("ERROR: {}", why.message)),
    };

    if !(200..300).contains(&read.status) {
      return Answer::refusal(format!("ERROR: HTTP {} for {}", read.status, input.address));
    }

    // A page that is not markup has no head to read, and that is an answer rather than a failure.
    structured(&PageHeadOutput {
      html: is_markup(&read).then(|| decode(&read.bytes, &read.raw_content_type)),
      url: read.url,
      kind: read.raw_content_type,
    })
  }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

/// The search engines a search may query, with their states, and the rest each refusal costs.
#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnginesOutput {
  pub engines: Vec<EngineOutput>,
  pub cooldowns: Vec<CooldownOutput>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineOutput {
  pub name: String,
  pub category: String,
  pub korean_native: bool,
  /// `healthy`, `degraded`, `suspended`, or `unknown` where it has not been read.
  pub state: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observed_at: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub suspended_until: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub elapsed_ms: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub result_count: Option<u64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CooldownOutput {
  pub reason: String,
  pub ms: i64,
}

/// The search engines' states, in the roster's order, as the store last saw them.
pub struct SearchEngines;

impl Tool for SearchEngines {
  const NAME: &'static str = "search_engines";
  const NAMESPACE: &'static str = "explorer";
  const DESCRIPTION: &'static str = "Answers each search engine a search may query, in the roster's order, with its state as it was last seen, and how long an engine rests for each kind of refusal.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = NoInput;

  fn output_schema() -> Option<Value> {
    Some(schema::<EnginesOutput>())
  }

  async fn call(&self, session: &Session, _input: NoInput) -> Answer {
    let store = match store_of(session) {
      Ok(store) => store,
      Err(refused) => return refused,
    };
    let standing = match engine_standing(&store).await {
      Ok(standing) => standing,
      Err(why) => return Answer::refusal(format!("ERROR: {}", why.error)),
    };
    let engines = standing
      .engines
      .into_iter()
      .map(|engine| EngineOutput {
        name: engine.name,
        category: engine.category,
        korean_native: engine.korean_native,
        state: engine.state,
        reason: engine.reason,
        observed_at: engine.observed_at,
        suspended_until: engine.suspended_until,
        elapsed_ms: engine.elapsed_ms,
        result_count: engine.result_count,
      })
      .collect();
    let cooldowns = standing
      .cooldowns
      .into_iter()
      .map(|cooldown| CooldownOutput {
        reason: cooldown.reason,
        ms: cooldown.rest_ms,
      })
      .collect();

    structured(&EnginesOutput { engines, cooldowns })
  }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageSearchAsk {
  /// The query.
  q: String,
  /// Whose requests are paced together: one name for each run's reading.
  pacing: String,
  /// The image engines to ask: each an engine's roster name or a category standing for its image engines;
  /// none for every image engine.
  #[serde(default)]
  engines: Vec<String>,
  /// How many pictures to answer, 1 to 30; 10 where absent.
  #[serde(default)]
  limit: Option<u32>,
  /// The language the query is written in, such as "ko" or "en"; any where absent.
  #[serde(default)]
  language: Option<String>,
}

/// A picture search over the image engines, each hit's fields its source's own statement.
pub struct SearchImages;

impl Tool for SearchImages {
  const NAME: &'static str = "search_images";
  const NAMESPACE: &'static str = "explorer";
  const DESCRIPTION: &'static str = "Asks the image search engines directly and answers each picture as fields rather than a snippet: its direct address, the page it stands on, title, creator, publisher, pixel dimensions, media type and licence, each as the source states it and null where it states none.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = ImageSearchAsk;

  fn output_schema() -> Option<Value> {
    Some(schema::<ImageSearchOutput>())
  }

  async fn call(&self, session: &Session, input: ImageSearchAsk) -> Answer {
    let pacing = match paced(session, &input.pacing) {
      Ok(pacing) => pacing,
      Err(refused) => return refused,
    };
    let query = input.q.trim();

    if query.is_empty() {
      return Answer::refusal("ERROR: The query is empty.");
    }

    let count = match input.limit {
      None => DEFAULT_SEARCH_COUNT,
      Some(limit @ 1..=30) => limit as usize,
      Some(limit) => {
        return Answer::refusal(format!("ERROR: The limit is {limit}; it is 1 to {MAX_SEARCH_COUNT}."));
      }
    };
    let engines = match image_engines_named(&input.engines) {
      Ok(engines) => engines,
      Err(why) => return Answer::refusal(format!("ERROR: {why}")),
    };
    let found = match search_images(
      &Search {
        query,
        count: Some(count),
        language: input.language.as_deref(),
        category: None,
        engine_interval_ms: None,
        engines: &engines,
      },
      &SearchRoads {
        store: &pacing,
        check: &Check::default(),
        transport: &*network(),
        render: None,
      },
    )
    .await
    {
      Ok(found) => found,
      Err(why) => return Answer::refusal(format!("ERROR: {}", why.message)),
    };

    structured(&ImageSearchOutput {
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
}
