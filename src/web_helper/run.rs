//! One record's web reading (docs/web-access.md, 「One session, one investigation」): its page budget and
//! the search refusals it has met, its session — the direct client's jar, the pacing, the real Chrome it falls
//! back to — and the research branches it asks for. A record holds one, in its web helper, which opens it as
//! it starts and closes it as it ends.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::tool::Session;
use crate::web::address::Check;
use crate::web::clock::{Clock, SystemClock};
use crate::web::pacing::StorePacing;
use crate::web::session::{FetchPage, FetchSlice, SessionParts, WebSession};
use crate::web::{Failure, WebError, WebResult};
use crate::web_helper::branch::Branches;

pub const SEARCH_STOPPED: &str = "SEARCH UNAVAILABLE (not asked again in this run): the search engines refused twice in a row, so another query would meet the same refusal. Work from the pages already in hand and say plainly in the result that the search itself could not run.";

pub const BUDGET_SPENT: &str = "BUDGET SPENT: this run has read every page its budget allows, so no further page is read. Write the result now from what you already have, citing only pages that were actually read, and put what stays open under UNCONFIRMED.";

/// The issuer's label a record's research settings stand under.
pub const RESEARCH_LABEL: &str = "research";

/// What a record's web tools are held to, and the run its research branches are asked for under, as its
/// issuer's label `research` says (docs/api.md, 「What a placed tool reads of its record」). Any other key of
/// the label is the issuer's and left unread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Research {
  /// How pages are read: `direct-then-browser` where none is named, `browser-first` or `direct-only`.
  pub strategy: Option<String>,
  pub host_interval_ms: Option<u64>,
  pub engine_interval_ms: Option<u64>,
  /// How many pages the record's web tools read before every further page is refused; none where it names no
  /// budget.
  pub pages: Option<u32>,
  /// The research run's id, which a branch is asked for under.
  pub run: Option<i64>,
  /// The run this record is itself a branch of, where it is one.
  pub branch_of: Option<i64>,
  /// The base address of the research unit a branch is asked at.
  pub branches_at: Option<String>,
  /// The bearer token the research unit is asked with.
  pub branches_token: Option<String>,
}

impl Research {
  /// The settings a record's web tools are held to: the defaults where it carries no label `research`, or
  /// why the label does not read.
  pub fn of(session: &Session) -> Result<Self, String> {
    session.label(RESEARCH_LABEL).map_or(Ok(Self::default()), |said| {
      Self::deserialize(said).map_err(|why| format!("the label `{RESEARCH_LABEL}` does not read: {why}"))
    })
  }

  pub fn direct_only(&self) -> bool {
    self.strategy.as_deref() == Some("direct-only")
  }

  pub fn browser_first(&self) -> bool {
    self.strategy.as_deref() == Some("browser-first")
  }
}

/// The search refusals a run's tools have met in a row, and the pages read against the record's budget.
pub struct ResearchGuard {
  unavailable_in_a_row: AtomicU32,
  pages: Option<u32>,
  read: Mutex<Read>,
}

/// The pages a record has read: the addresses `web_fetch` read, and how many pages in all.
#[derive(Default)]
struct Read {
  addresses: HashSet<String>,
  count: u32,
}

impl ResearchGuard {
  /// A guard holding the record's page budget, none where it names none.
  pub fn new(pages: Option<u32>) -> Self {
    Self {
      unavailable_in_a_row: AtomicU32::new(0),
      pages,
      read: Mutex::new(Read::default()),
    }
  }

  /// One page taken out of the budget, or the refusal once it is spent. An address the record has read already is
  /// that same page read on and takes nothing; a site tool's page names no address and is a page each call.
  pub fn take_page(&self, address: Option<&str>) -> Option<&'static str> {
    let pages = self.pages?;
    let mut read = self.read.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if address.is_some_and(|address| read.addresses.contains(address)) {
      return None;
    }

    if read.count >= pages {
      return Some(BUDGET_SPENT);
    }

    read.count += 1;

    if let Some(address) = address {
      read.addresses.insert(address.to_owned());
    }

    None
  }

  /// The answer a search gets without asking the engines, once they have refused twice in a row.
  pub fn search_stopped(&self) -> Option<&'static str> {
    (self.unavailable_in_a_row.load(Ordering::SeqCst) >= 2).then_some(SEARCH_STOPPED)
  }

  pub fn note_search(&self, rendered: &str) {
    if rendered.contains("SEARCH UNAVAILABLE:") {
      self.unavailable_in_a_row.fetch_add(1, Ordering::SeqCst);
    } else {
      self.unavailable_in_a_row.store(0, Ordering::SeqCst);
    }
  }
}

/// One record's web reading.
pub struct WebRun {
  pub research: Research,
  pub guard: ResearchGuard,
  pub session: Arc<WebSession>,
  pub branches: Branches,
}

impl WebRun {
  /// The reading a record's session opens: on the store's pacing under the record's data root, by the real
  /// network and resolver, its research settings out of the record; or why the record gives it none.
  pub fn open(session: &Session) -> Result<Self, String> {
    let research = Research::of(session)?;
    let data_root = session.data_root()?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    Ok(Self {
      guard: ResearchGuard::new(research.pages),
      session: Arc::new(WebSession::new(SessionParts {
        store: Arc::new(StorePacing::new(crate::store::place(&data_root))),
        transport: crate::web::client::network(),
        check: Check {
          clock,
          ..Check::default()
        },
        host_interval_ms: research.host_interval_ms.unwrap_or(crate::web::client::DIRECT_HOST_INTERVAL_MS),
        browsers: None,
      })),
      branches: Branches::new(),
      research,
    })
  }

  /// A reading over parts a test hands in.
  pub fn with(research: Research, session: WebSession) -> Self {
    Self {
      guard: ResearchGuard::new(research.pages),
      session: Arc::new(session),
      branches: Branches::new(),
      research,
    }
  }

  /// One page as `web_fetch` reads it, by the run's strategy.
  pub async fn read_page(
    &self,
    url: &str,
    offset: Option<usize>,
    max_chars: Option<usize>,
    raw: bool,
    layout: bool,
  ) -> WebResult<FetchSlice> {
    self
      .session
      .fetch_page(
        url,
        &FetchPage {
          offset,
          max_chars,
          raw,
          browser_fallback: !self.research.direct_only(),
          browser_first: self.research.browser_first(),
          pdf_layout: layout,
        },
      )
      .await
  }

  /// What a failed request answers: the error, said as a refusal.
  pub fn failed(&self, error: &WebError) -> crate::tool::Answer {
    crate::tool::Answer::refusal(fetch_error_text(error))
  }

  /// The run's branch requests still waiting dropped, and its browser and profile ended, as its record goes.
  pub async fn close(&self) {
    self.branches.end();
    self.session.close().await;
  }
}

/// The ERROR a web tool answers with. An address this machine's own policy refused is said to be the
/// machine's, so the caller does not take the page for one its site would not serve.
pub fn fetch_error_text(error: &WebError) -> String {
  let text = format!("ERROR: {}", error.message);

  if error.kind == Failure::UnsafeAddress {
    format!("{text}\nThis machine's own address policy refused it before the site was asked.")
  } else {
    text
  }
}
