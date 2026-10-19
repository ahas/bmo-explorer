//! One record's web reading (docs/web-access.md, 「One session, one investigation」): its research close and
//! the search refusals it has met, its session — the direct client's jar, the pacing, the real Chrome it falls
//! back to — and the branches it has opened. A record holds one, in its web helper, which opens it as it starts
//! and closes it as it ends.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::tool::Session;
use crate::web::address::Check;
use crate::web::clock::{Clock, SystemClock};
use crate::web::pacing::StorePacing;
use crate::web::session::{FetchPage, FetchSlice, SessionParts, WebSession};
use crate::web::{Failure, WebError, WebResult};

use crate::site::unread::{Unread, fetch_error_text, fetch_failure_shape, record, site_of};

pub const RESEARCH_CLOSED: &str = "RESEARCH CLOSED: this run's research time is over, so no new search, page or branch is started. Write the result now from what you already have, citing only pages that were actually read, and say plainly what could not be confirmed.";

pub const SEARCH_STOPPED: &str = "SEARCH UNAVAILABLE (not asked again in this run): the search engines refused twice in a row, so another query would meet the same refusal. Work from the pages already in hand and say plainly in the result that the search itself could not run.";

/// What a research run's web tools are held to, as its session record says (docs/web-access.md, 「Settings from the research run」).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Research {
  /// Epoch milliseconds after which the web tools refuse new work; none outside a research run.
  pub closes_at: Option<u64>,
  /// The pool owner its branches are held under.
  pub owner: Option<String>,
  /// `search` or `browse`: the kind of run, under which a page it could not read is recorded.
  pub source: Option<String>,
  pub role: Option<String>,
  pub max_children: u32,
  pub chat_id: i64,
  /// How pages are read: `direct-then-browser` where none is named, `browser-first` or `direct-only`.
  pub strategy: Option<String>,
  pub host_interval_ms: Option<u64>,
  pub engine_interval_ms: Option<u64>,
  /// The rulebooks the run reads whole by name.
  pub rulebooks: Vec<String>,
}

impl Research {
  /// The settings a record's web tools are held to.
  pub fn of(session: &Session) -> Self {
    let said = session.research.clone().unwrap_or_default();

    Self {
      closes_at: said.closes_at,
      owner: said.owner,
      source: said.source.filter(|source| source == "search" || source == "browse"),
      role: said.role,
      max_children: said.branches.unwrap_or(0),
      chat_id: said.chat.unwrap_or(0),
      strategy: said.strategy,
      host_interval_ms: said.host_interval_ms,
      engine_interval_ms: said.engine_interval_ms,
      rulebooks: said.rulebooks,
    }
  }

  pub fn direct_only(&self) -> bool {
    self.strategy.as_deref() == Some("direct-only")
  }

  pub fn browser_first(&self) -> bool {
    self.strategy.as_deref() == Some("browser-first")
  }
}

/// The research close a run's tools are held to, and the search refusals met in a row.
pub struct ResearchGuard {
  closes_at: Option<u64>,
  unavailable_in_a_row: AtomicU32,
  clock: Arc<dyn Clock>,
}

impl ResearchGuard {
  pub fn new(closes_at: Option<u64>, clock: Arc<dyn Clock>) -> Self {
    Self {
      closes_at,
      unavailable_in_a_row: AtomicU32::new(0),
      clock,
    }
  }

  /// The refusal a tool answers with once research has closed, or none while it is open.
  pub fn closed(&self) -> Option<&'static str> {
    self.closes_at.filter(|at| self.clock.now() >= *at).map(|_| RESEARCH_CLOSED)
  }

  /// The deadline a request started now must finish inside.
  pub fn deadline(&self) -> Option<u64> {
    self.closes_at
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
  /// The store a page no road read is written into (docs/store.md).
  pub store: PathBuf,
  /// How many branches this run has opened, and the processes of those still running.
  pub opened: AtomicU64,
  pub branches: Mutex<Vec<u32>>,
  pub closing: std::sync::atomic::AtomicBool,
}

impl WebRun {
  /// The reading a record's session opens: on the store's pacing, by the real network and resolver, its
  /// research settings out of the record.
  pub fn open(session: &Session) -> Self {
    let research = Research::of(session);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    Self {
      guard: ResearchGuard::new(research.closes_at, clock.clone()),
      session: Arc::new(WebSession::new(SessionParts {
        store: Arc::new(StorePacing::new(crate::store::place(&session.data_root))),
        transport: crate::web::client::network(),
        check: Check {
          clock,
          ..Check::default()
        },
        host_interval_ms: research.host_interval_ms.unwrap_or(crate::web::client::DIRECT_HOST_INTERVAL_MS),
        browsers: None,
      })),
      store: crate::store::place(&session.data_root),
      research,
      opened: AtomicU64::new(0),
      branches: Mutex::new(Vec::new()),
      closing: std::sync::atomic::AtomicBool::new(false),
    }
  }

  /// A reading over parts a test hands in.
  pub fn with(research: Research, session: WebSession, store: PathBuf, clock: Arc<dyn Clock>) -> Self {
    Self {
      guard: ResearchGuard::new(research.closes_at, clock),
      session: Arc::new(session),
      store,
      research,
      opened: AtomicU64::new(0),
      branches: Mutex::new(Vec::new()),
      closing: std::sync::atomic::AtomicBool::new(false),
    }
  }

  /// One page as `web_fetch` reads it, by the run's strategy, under its deadline.
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
          deadline_ms: self.guard.deadline(),
          browser_fallback: !self.research.direct_only(),
          browser_first: self.research.browser_first(),
          pdf_layout: layout,
        },
      )
      .await
  }

  /// A page this run could not read by the direct read nor the real Chrome after it, written down; a failure
  /// that is this machine's own is no page's, and writing it down never costs the fetch its answer.
  pub async fn note_unread(&self, url: &str, error: &WebError) {
    let Some(run) = self.research.source.as_deref() else { return };

    if self.guard.closed().is_some() {
      return;
    }

    let (Some((shape, detail)), Some(site)) = (fetch_failure_shape(error), site_of(url)) else {
      return;
    };
    let written = record(
      &self.store,
      &Unread {
        site: &site,
        url,
        shape,
        detail: &detail,
        run,
        recorded_at: crate::web::clock::now_ms(),
      },
    )
    .await;

    if let Err(why) = written {
      eprintln!("unread page not recorded: {why}");
    }
  }

  /// What a failed request answers: the research close where that is what it ran into, the error otherwise.
  pub fn closed_or(&self, error: &WebError) -> crate::tool::Answer {
    if error.kind == Failure::Deadline && self.guard.closed().is_some() {
      crate::tool::Answer::text(RESEARCH_CLOSED)
    } else {
      crate::tool::Answer::refusal(fetch_error_text(error))
    }
  }

  /// The run's branches stopped and its browser and profile ended, as its record goes.
  pub async fn close(&self) {
    self.closing.store(true, Ordering::SeqCst);

    let live: Vec<u32> =
      std::mem::take(&mut *self.branches.lock().unwrap_or_else(std::sync::PoisonError::into_inner));

    for pid in live {
      // SAFETY: a signal to a branch process this run started and still counts as running; the branch's own
      // verb gives its pool place back as it stops.
      unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
      }
    }

    self.session.close().await;
  }
}
