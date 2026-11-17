//! The reading of a picture, asked of the lab's own Qwen vision pool over the model gateway
//! (docs/web-access.md, 「When the browser meets a challenge」): one OpenAI-shaped chat completion with the
//! picture as a data URL and the question asking for one JSON object back. This is the owner's allowance of
//! 2026-11-17 — the tool may call his Qwen model from inside itself — and no paid solving service: what is
//! spent here is a call to an engine of this deployment, on a picture this layer took itself.
//!
//! The address, the key and the model are read beside this code, not from a settings file, in the way every
//! other built-in argument of the web layer is: `BMO_VISION_ADDRESS` over the gateway address, `BMO_VISION_KEY_FILE`
//! over the key file, `BMO_VISION_MODEL` over the model name, each falling back to the default below. The key
//! is a file of the data root's `env/`, read whole and never printed.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{Value, json};

use crate::web::browser::Answer;
use crate::web::error::{Failure, WebError, WebResult};

/// The gateway in front of the deployment's own engines.
pub const DEFAULT_ADDRESS: &str = "http://127.0.0.1:18030";
/// The vision-capable Qwen pool, the same one the eye's first rungs stand on.
pub const DEFAULT_MODEL: &str = "flash-next-coder";
/// The saved secret holding the gateway key, under the data root's `env/`.
pub const KEY_FILE: &str = "llm-gateway.key";

/// What reads a picture. The gateway client answers below; a test hands in its own reading.
pub trait Vision: Send + Sync {
  /// The picture (PNG bytes) and the question, answered with the JSON object the model returned.
  fn read<'a>(&'a self, png: &'a [u8], ask: &'a str) -> Answer<'a, WebResult<Value>>;
  /// The name the attempt log reads this road by.
  fn road(&self) -> &'static str;
}

/// The reading asked at the deployment's model gateway over the OpenAI-shaped route.
pub struct Gateway {
  address: String,
  key: String,
  model: String,
}

impl Gateway {
  /// The reader of this data root, or none where the key file is not there to read.
  pub fn open(data_root: &Path) -> Option<Self> {
    let address = std::env::var("BMO_VISION_ADDRESS").unwrap_or_else(|_| DEFAULT_ADDRESS.to_owned());
    let model = std::env::var("BMO_VISION_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
    let key_file = std::env::var("BMO_VISION_KEY_FILE")
      .map(PathBuf::from)
      .unwrap_or_else(|_| data_root.join("env").join(KEY_FILE));
    let key = std::fs::read_to_string(key_file).ok()?;

    Some(Self {
      address: address.trim_end_matches('/').to_owned(),
      key: key.trim().to_owned(),
      model,
    })
  }
}

impl Vision for Gateway {
  fn read<'a>(&'a self, png: &'a [u8], ask: &'a str) -> Answer<'a, WebResult<Value>> {
    Box::pin(async move {
      let url = format!("{}/v1/chat/completions", self.address);
      let body = json!({
        "model": self.model,
        "temperature": 0,
        "max_tokens": 512,
        "messages": [{
          "role": "user",
          "content": [
            {
              "type": "image_url",
              "image_url": {
                "url": format!(
                  "data:image/png;base64,{}",
                  base64::engine::general_purpose::STANDARD.encode(png)
                ),
              },
            },
            { "type": "text", "text": ask },
          ],
        }],
      });

      let answer = reqwest::Client::new()
        .post(url)
        .header("authorization", format!("Bearer {}", self.key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|why| {
          WebError::new(format!("the vision road could not be asked: {why}"), Failure::Transport)
        })?;
      let said = answer.json::<Value>().await.map_err(|why| {
        WebError::new(format!("the vision road answered what does not read: {why}"), Failure::Transport)
      })?;
      let text = said["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| WebError::new("the vision road answered no text", Failure::Transport))?;

      json_of(text)
        .ok_or_else(|| WebError::new("the vision road answered no JSON object", Failure::Transport))
    })
  }

  fn road(&self) -> &'static str {
    "the vision road"
  }
}

/// The first JSON object of an answer, spoken through ``` fences or plain prose around it included; none
/// where the answer holds no whole object.
pub fn json_of(text: &str) -> Option<Value> {
  let bytes = text.as_bytes();
  let mut open = 0_usize;

  loop {
    let from = text[open..].find('{')? + open;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for (at, one) in bytes[from..].iter().enumerate() {
      if in_string {
        if escaped {
          escaped = false;
        } else if *one == b'\\' {
          escaped = true;
        } else if *one == b'"' {
          in_string = false;
        }
        continue;
      }

      match one {
        b'"' => in_string = true,
        b'{' => depth += 1,
        b'}' => {
          depth -= 1;
          if depth == 0 {
            if let Ok(found) = serde_json::from_str::<Value>(&text[from..=from + at]) {
              return Some(found);
            }
            break;
          }
        }
        _ => {}
      }
    }

    open = from + 1;
  }
}
