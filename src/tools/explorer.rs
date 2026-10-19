//! The `explorer` entries (docs/api.md, 「The explorer entries」): what a service's own run reads of the web —
//! a page drawn and read whole, a raster picture, a page's head and the search engines' states — through the
//! web layer's checks, so every outward request keeps them. Each is for the system alone: never shown to a
//! session, and reached by the tool host's `POST /calls`.

use base64::Engine;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store;
use crate::tool::{Answer, Content, Hints, Served, Session, Tool};
use crate::web::address::Check;
use crate::web::client::{Direct, decode, network};
use crate::web::import::import_page;
use crate::web::jar::{Jar, StoredCookie};
use crate::web::pacing::{Boxed, EngineHealth, Pacing, Recipe, StorePacing};
use crate::web::picture::fetch_raster;
use crate::web::search::roster::ROSTER;

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&ImportPage, &FetchRaster, &FetchPage, &SearchEngines];

/// How much of a page is read for its head: the tags a preview needs sit in the head, and a head larger than
/// this is a page that is not going to be previewed well anyway.
pub const PAGE_HEAD_BYTES: usize = 512 * 1024;

/// How long a page's head may take to come.
pub const PAGE_HEAD_TIMEOUT_MS: u64 = 10_000;

/// Between two requests for a page's head to one host.
pub const PAGE_HEAD_HOST_INTERVAL_MS: u64 = 250;

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
  fn reserve(&self, key: &str, interval_ms: u64, deadline_ms: u64, now: u64) -> Boxed<'_, Option<u64>> {
    let key = self.key(key);

    Box::pin(async move { self.store.reserve(&key, interval_ms, deadline_ms, now).await })
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

fn paced(session: &Session, pacing: &str) -> Paced {
  Paced::new(pacing, StorePacing::new(store::place(&session.data_root)))
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
pub struct ImportPageInput {
  /// The page's address.
  address: String,
}

/// A page drawn in a browser and read whole.
#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedPageOutput {
  pub page: PageOutput,
  /// Each figure captured as a picture; a figure that would not capture is not among them.
  pub captured: Vec<CapturedOutput>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageOutput {
  /// The address after every redirect.
  pub url: String,
  pub title: String,
  pub keywords: Vec<String>,
  /// The page's text.
  pub body: String,
  pub references: Vec<ReferenceOutput>,
  /// Each figure the page draws.
  pub figures: Vec<FigureOutput>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReferenceOutput {
  pub text: String,
  pub url: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FigureOutput {
  pub n: i64,
  pub caption: String,
  /// The figure as the page drew it, where it drew it as SVG.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub svg: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapturedOutput {
  pub n: i64,
  /// The PNG's bytes, base64 encoded.
  pub png: String,
}

/// A page drawn in a browser and read whole, with each figure it draws captured.
pub struct ImportPage;

impl Tool for ImportPage {
  const NAME: &'static str = "import_page";
  const NAMESPACE: &'static str = "explorer";
  const DESCRIPTION: &'static str = "Draws a page in a browser behind the address check and reads it whole: its address after redirects, title, keywords, text, references and figures, with each figure it draws captured as a PNG.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = ImportPageInput;

  fn output_schema() -> Option<Value> {
    Some(schema::<ImportedPageOutput>())
  }

  async fn call(&self, _session: &Session, input: ImportPageInput) -> Answer {
    let reading = match import_page(&input.address, Check::default().guard()).await {
      Ok(reading) => reading,
      Err(why) => return Answer::refusal(format!("ERROR: {}", why.message)),
    };
    let page = reading.page;
    let engine = base64::engine::general_purpose::STANDARD;

    structured(&ImportedPageOutput {
      page: PageOutput {
        url: page.url,
        title: page.title,
        keywords: page.keywords,
        body: page.body,
        references: page
          .references
          .into_iter()
          .map(|reference| ReferenceOutput {
            text: reference.text,
            url: reference.url,
          })
          .collect(),
        figures: page
          .figures
          .into_iter()
          .map(|figure| FigureOutput {
            n: figure.n,
            caption: figure.caption,
            svg: figure.svg,
          })
          .collect(),
      },
      captured: reading
        .figures
        .into_iter()
        .map(|captured| CapturedOutput {
          n: captured.figure.n,
          png: engine.encode(&captured.png),
        })
        .collect(),
    })
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
    let pacing = paced(session, &input.pacing);

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
    let pacing = paced(session, &input.pacing);
    let check = Check::default();
    let transport = network();
    let jar = tokio::sync::Mutex::new(Jar::default());
    let mut direct =
      Direct::new(&pacing, &jar, check.clock.now() + PAGE_HEAD_TIMEOUT_MS, &check, &*transport);

    direct.interval_ms = PAGE_HEAD_HOST_INTERVAL_MS;
    direct.byte_cap = PAGE_HEAD_BYTES;

    let read = match direct.bytes(&input.address).await {
      Ok(read) => read,
      Err(why) => return Answer::refusal(format!("ERROR: {}", why.message)),
    };

    if !(200..300).contains(&read.status) {
      return Answer::refusal(format!("ERROR: HTTP {} for {}", read.status, input.address));
    }

    // A page that is not markup — a PDF, a raw source file — has no head to read, and that is an answer
    // rather than a failure.
    let markup = matches!(read.content_type.as_str(), "text/html" | "application/xhtml+xml");

    structured(&PageHeadOutput {
      html: markup.then(|| decode(&read.bytes, &read.raw_content_type)),
      url: read.url,
      kind: read.raw_content_type,
    })
  }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

/// The search engines a research run may query, with their states, and the rest each refusal costs.
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
  const DESCRIPTION: &'static str = "Answers each search engine a research run may query, in the roster's order, with its state as it was last seen, and how long an engine rests for each kind of refusal.";
  const HINTS: Hints = READS;
  const CAPABILITIES: &'static [&'static str] = &[];
  const SYSTEM: bool = true;

  type Input = NoInput;

  fn output_schema() -> Option<Value> {
    Some(schema::<EnginesOutput>())
  }

  async fn call(&self, session: &Session, _input: NoInput) -> Answer {
    let store = StorePacing::new(store::place(&session.data_root));
    let mut engines = Vec::new();

    for engine in &ROSTER.engine {
      let health = match store.health(&engine.name).await {
        Ok(health) => health,
        Err(why) => return Answer::refusal(format!("ERROR: the engines' states could not be read: {why}")),
      };
      let (state, reason, observed_at, suspended_until, elapsed_ms, result_count) = match health {
        Some(seen) => (
          seen.state.name().to_owned(),
          seen.reason,
          seen.observed_at,
          seen.suspended_until,
          seen.elapsed_ms,
          seen.result_count,
        ),
        None => ("unknown".to_owned(), None, None, None, None, None),
      };

      engines.push(EngineOutput {
        name: engine.name.clone(),
        category: engine.category.clone(),
        korean_native: engine.korean_native,
        state,
        reason,
        observed_at,
        suspended_until,
        elapsed_ms,
        result_count,
      });
    }

    let cooldowns = ROSTER
      .cooldown
      .iter()
      .map(|line| CooldownOutput {
        reason: line.reason.clone(),
        ms: ROSTER.rests.named(&line.rest).unwrap_or(0),
      })
      .collect();

    structured(&EnginesOutput { engines, cooldowns })
  }
}
