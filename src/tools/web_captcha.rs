//! `web_captcha`, the web namespace's forcing read (docs/api.md, 「The captcha read」): one page opened in the
//! real browser from the start, a CAPTCHA or browser challenge met on the way worked as a bounded loop of
//! rounds, and the answer headed with what the loop came to — all of it asked of the record's web helper.

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
  const DESCRIPTION: &'static str = "Reads one page on the forcing road: the real browser from the start, no direct attempt first, and a CAPTCHA or browser challenge met in the page worked as a bounded loop of rounds — the site's own check handed through to its end, then its box clicked, its slider pulled to its gap or its words typed, through the browser's own input on a walked human-like path, with the screen read by this deployment's own vision model where a round needs it read — until the challenge ends, the site ends it or the loop's bound of rounds and time is spent, and then the page opened once more. The answer is the page as web_fetch returns it, headed CHALLENGE CLEARED where a challenge stood and the retry came through, or NO CHALLENGE where the browser met none, with an ATTEMPT line saying what the loop came to: its rounds, what each acting round did and where it stopped. A challenge that stands is refused with its own words and that account, never with a claim of a solve, and nothing goes to a paid solving service.";
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
