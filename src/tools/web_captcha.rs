//! `web_captcha`, the web namespace's forcing read (docs/api.md, 「The captcha read」): one page opened in the
//! real browser from the start, a CAPTCHA or browser challenge met on the way attempted once, and the answer
//! headed with what the attempt came to — all of it asked of the record's web helper.

use crate::site::helper::{WebAsked, ask};
use crate::tool::{Answer, Hints, Served, Session, Tool};
use schemars::JsonSchema;
use serde::Deserialize;

/// Every entry the executable answers, as its manifest declares them.
pub const TOOLS: &[&dyn Served] = &[&WebCaptcha];

#[derive(Deserialize, JsonSchema)]
pub struct CaptchaInput {
  /// The URL to fetch.
  #[schemars(url)]
  url: String,
  /// Character offset to start from (default 0).
  #[schemars(range(min = 0))]
  offset: Option<u64>,
  /// How many characters to return (default 20000).
  #[schemars(range(min = 1, max = 100000))]
  max_chars: Option<u64>,
}

pub struct WebCaptcha;

impl Tool for WebCaptcha {
  const NAME: &'static str = "web_captcha";
  const NAMESPACE: &'static str = "web";
  const TITLE: Option<&'static str> = Some("Web captcha");
  const DESCRIPTION: &'static str = "Reads one page on the forcing road: the real browser from the start, no direct attempt first, and a CAPTCHA or browser challenge met in the page attempted once — the site's own check handed through to its end and a visible challenge box clicked, then the page opened once more. The answer is the page as web_fetch returns it, headed CHALLENGE CLEARED where a challenge stood and the retry came through, or NO CHALLENGE where the browser met none. A challenge that stands after the one attempt is refused with its own words and never attempted twice; nothing is solved by reading pictures, answering puzzles or a solving service.";
  // Like every reading: nothing on this machine changes, and several calls may go side by side.
  const HINTS: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: None,
    open_world: true,
  };
  const CAPABILITIES: &'static [&'static str] = &["web.read"];

  type Input = CaptchaInput;

  async fn call(&self, session: &Session, input: CaptchaInput) -> Answer {
    let asked = WebAsked::Captcha {
      url: input.url,
      offset: input.offset.map(|offset| offset as usize),
      max_chars: input.max_chars.map(|chars| chars as usize),
    };

    ask(session, &asked).await
  }
}
