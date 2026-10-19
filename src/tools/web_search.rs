//! `web_search`, the web namespace's search (docs/web-access.md): the search engines asked directly, through
//! the record's web helper, which holds the run's research close and the refusals it has met.

use crate::site::helper::{WebAsked, ask};
use crate::tool::{Answer, Hints, NonEmpty, Served, Session, Tool};
use schemars::JsonSchema;
use serde::Deserialize;

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&WebSearch];

/// Which engines a search asks.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Category {
  General,
  News,
  Images,
  It,
}

impl Category {
  fn name(self) -> &'static str {
    match self {
      Self::General => "general",
      Self::News => "news",
      Self::Images => "images",
      Self::It => "it",
    }
  }
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchInput {
  /// The search query.
  query: NonEmpty,
  /// How many hits to return (default 10).
  #[schemars(range(min = 1, max = 30))]
  count: Option<u32>,
  /// Result language such as "ko" or "en"; omit for any.
  language: Option<String>,
  /// Which engines to ask: general web, news, images or IT; omit for "general".
  category: Option<Category>,
}

pub struct WebSearch;

impl Tool for WebSearch {
  const NAME: &'static str = "web_search";
  const NAMESPACE: &'static str = "web";
  const TITLE: Option<&'static str> = Some("Web search");
  const DESCRIPTION: &'static str = "Searches the web by asking the search engines directly and returns their results as they are: title, URL and snippet per hit, nothing summarized. Run more than one query when the first does not settle the question, then open the pages that matter with web_fetch.";
  // It changes nothing on this machine and says so, which lets Claude Code send a turn's searches side by
  // side.
  const HINTS: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: None,
    open_world: true,
  };
  const CAPABILITIES: &'static [&'static str] = &["web.read"];

  type Input = SearchInput;

  async fn call(&self, session: &Session, input: SearchInput) -> Answer {
    let asked = WebAsked::Search {
      query: input.query.to_string(),
      count: input.count.map(|count| count as usize),
      language: input.language,
      category: input.category.map(|category| category.name().to_owned()),
    };

    ask(session, &asked).await
  }
}
