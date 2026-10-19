//! What a record's web tools ask its web helper (the tool host's docs/mcp-host.md, 「What a record's tools keep」): one JSON
//! object on one line of the helper's socket, answered on one line. The core tools' requests are answered with
//! the tool result, a placed site tool's roads with what the road gave or the failure it met, and the research
//! guard round a site tool's call with its verdict. The tools ask through this client, and the helper,
//! `tools/_shared/web/`, answers out of the record's one web reading.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use crate::forms::{Failure, SearchResults, WebError, WebResult};
use crate::tool::protocol::{Sockets, ask_line};
use crate::tool::{Answer, Session};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::site::contract::{Arrival, PageHead, PageRequest, Reach, Rendered, Slice};

/// The name the tools' manifests give the helper among their shared helpers.
pub const HELPER: &str = "web";

/// One request of the record's web reading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebAsked {
  /// The search engines asked, for `count` hits held to the search's bounds.
  Search {
    query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
  },
  /// One page read and sliced as `web_fetch` reads it.
  Fetch {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_chars: Option<usize>,
    #[serde(default)]
    raw: bool,
  },
  /// One picture by the address of its image file, answered as a picture.
  Picture { image: String },
  /// One research branch opened for the angle, answered with its notes.
  Branch { angle: String },
  /// A site tool's `page` road, answered with the road's result.
  Page {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request: Option<PageRequest>,
  },
  /// A site tool's `rendered` road, answered with the road's result.
  Rendered {
    url: String,
    #[serde(default)]
    fresh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arrival: Option<Arrival>,
  },
  /// A site tool's `engine` road, answered with the road's result.
  Engine { query: String, count: usize },
  /// A site tool's `document` road, answered with the road's result.
  Document { url: String, slice: Slice },
  /// A site tool's `head` road, answered with the road's result.
  Head { url: String },
  /// Whether a site tool's call may read at all, answered with the refusal's text or null: research closed, or,
  /// for a tool reading through the engines, searching stopped.
  Opening { engine: bool },
  /// How a site tool's call came out, answered with the tool result: an engine tool's answer counted as a
  /// search, and a failure answered as the reading answers a failed page.
  Ended {
    engine: bool,
    outcome: WebResult<String>,
  },
}

/// What the record's web helper answers to this, or why it could not be asked.
pub async fn ask(session: &Session, asked: &WebAsked) -> Answer {
  let sockets = Sockets::of(session);

  match sockets.helper(HELPER) {
    Ok(socket) => ask_line(socket, asked).await.unwrap_or_else(Answer::failure),
    Err(why) => Answer::failure(why),
  }
}

/// The record's web reading as a placed site tool is given it: each road asked of the record's web helper on its
/// socket. A helper out of reach fails the road as a program this machine needs that could not be had.
pub struct HelperReach {
  socket: Result<PathBuf, String>,
}

impl HelperReach {
  /// The reach of the helper the call under this record was handed.
  pub fn of(session: &Session) -> Self {
    Self {
      socket: Sockets::of(session).helper(HELPER).map(PathBuf::from),
    }
  }

  async fn asked<R: DeserializeOwned>(&self, asked: &WebAsked) -> Result<R, String> {
    ask_line(self.socket.as_ref().map_err(Clone::clone)?, asked).await
  }

  async fn reached<T: DeserializeOwned>(&self, asked: WebAsked) -> WebResult<T> {
    self
      .asked::<WebResult<T>>(&asked)
      .await
      .unwrap_or_else(|why| Err(WebError::new(why, Failure::BrowserUnavailable)))
  }

  /// The research guard's refusal of a call before it reads anything, where it refuses one.
  pub async fn opening(&self, engine: bool) -> Result<Option<String>, String> {
    self.asked(&WebAsked::Opening { engine }).await
  }

  /// A call's outcome as the record's reading answers it.
  pub async fn ended(&self, engine: bool, outcome: WebResult<String>) -> Answer {
    self.asked(&WebAsked::Ended { engine, outcome }).await.unwrap_or_else(Answer::failure)
  }
}

impl Reach for HelperReach {
  fn page<'a>(
    &'a self,
    url: &'a str,
    request: Option<PageRequest>,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    Box::pin(self.reached(WebAsked::Page {
      url: url.to_owned(),
      request,
    }))
  }

  fn rendered<'a>(
    &'a self,
    url: &'a str,
    fresh: bool,
    arrival: Option<Arrival>,
  ) -> Pin<Box<dyn Future<Output = WebResult<Rendered>> + Send + 'a>> {
    Box::pin(self.reached(WebAsked::Rendered {
      url: url.to_owned(),
      fresh,
      arrival,
    }))
  }

  fn engine<'a>(
    &'a self,
    query: &'a str,
    count: usize,
  ) -> Pin<Box<dyn Future<Output = WebResult<SearchResults>> + Send + 'a>> {
    Box::pin(self.reached(WebAsked::Engine {
      query: query.to_owned(),
      count,
    }))
  }

  fn document<'a>(
    &'a self,
    url: &'a str,
    slice: Slice,
  ) -> Pin<Box<dyn Future<Output = WebResult<String>> + Send + 'a>> {
    Box::pin(self.reached(WebAsked::Document {
      url: url.to_owned(),
      slice,
    }))
  }

  fn head<'a>(&'a self, url: &'a str) -> Pin<Box<dyn Future<Output = WebResult<PageHead>> + Send + 'a>> {
    Box::pin(self.reached(WebAsked::Head {
      url: url.to_owned(),
    }))
  }
}
