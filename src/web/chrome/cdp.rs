//! One DevTools connection to a Chrome's page: commands answered by id, and the events the page sends, over
//! the WebSocket Chrome serves on loopback. The protocol is JSON messages, read and written with serde.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::web::clock::{left, now_ms};
use crate::web::error::{Failure, WebError, WebResult};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// What sends a command: cheap to clone, so an event's handler can answer the page.
#[derive(Clone)]
pub struct Sender {
  out: mpsc::UnboundedSender<Message>,
  pending: Pending,
  next: Arc<AtomicU64>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn deadline_error(action: &str) -> WebError {
  WebError::new(format!("Chrome {action} did not finish before the web deadline"), Failure::Deadline)
}

impl Sender {
  /// One command, answered by the deadline.
  pub async fn command(&self, method: &str, params: Value, deadline_ms: u64) -> WebResult<Value> {
    if now_ms() >= deadline_ms {
      return Err(deadline_error(method));
    }

    let id = self.next.fetch_add(1, Ordering::SeqCst);
    let (answer, answered) = oneshot::channel();

    locked(&self.pending).insert(id, answer);

    let message = json!({ "id": id, "method": method, "params": params }).to_string();

    if self.out.send(Message::text(message)).is_err() {
      locked(&self.pending).remove(&id);
      return Err(WebError::new("Chrome DevTools is not connected", Failure::Transport));
    }

    match tokio::time::timeout(left(deadline_ms, now_ms()), answered).await {
      Err(_) => {
        locked(&self.pending).remove(&id);
        Err(deadline_error(method))
      }
      Ok(Err(_)) => Err(WebError::new("Chrome DevTools connection closed", Failure::Transport)),
      Ok(Ok(Err(why))) => Err(WebError::new(why, Failure::Transport)),
      Ok(Ok(Ok(result))) => Ok(result),
    }
  }

  /// A command sent and not waited on.
  pub fn tell(&self, method: &str, params: Value) {
    let id = self.next.fetch_add(1, Ordering::SeqCst);
    let _ = self
      .out
      .send(Message::text(json!({ "id": id, "method": method, "params": params }).to_string()));
  }
}

/// The connection: its sender, its events, and the tasks that read and write it.
pub struct Connection {
  pub sender: Sender,
  pub events: broadcast::Sender<(String, Value)>,
  reading: JoinHandle<()>,
  writing: JoinHandle<()>,
}

/// What a connection tells every event it reads, before any waiter hears of it.
pub type EventHandler = Arc<dyn Fn(&Sender, &str, &Value) + Send + Sync>;

impl Drop for Connection {
  fn drop(&mut self) {
    self.reading.abort();
    self.writing.abort();
  }
}

impl Connection {
  /// The connection to a page's DevTools address, open by the deadline. `handle` is told every event before
  /// any waiter is, so what it records is there when a waiter reads it.
  pub async fn open(address: &str, deadline_ms: u64, handle: EventHandler) -> WebResult<Self> {
    let opened =
      tokio::time::timeout(left(deadline_ms, now_ms()), tokio_tungstenite::connect_async(address)).await;
    let socket = match opened {
      Err(_) => return Err(deadline_error("DevTools connection")),
      Ok(Err(_)) => {
        return Err(WebError::new("Chrome DevTools connection failed", Failure::BrowserUnavailable));
      }
      Ok(Ok((socket, _))) => socket,
    };
    let (mut write, mut read) = socket.split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let (events, _) = broadcast::channel(256);
    let sender = Sender {
      out,
      pending: pending.clone(),
      next: Arc::new(AtomicU64::new(1)),
    };
    let writing = tokio::spawn(async move {
      while let Some(message) = outgoing.recv().await {
        if write.send(message).await.is_err() {
          break;
        }
      }
    });
    let reader_sender = sender.clone();
    let reader_events = events.clone();
    let reading = tokio::spawn(async move {
      while let Some(Ok(message)) = read.next().await {
        let Message::Text(text) = message else {
          continue;
        };
        let Ok(reply) = serde_json::from_str::<Value>(&text) else {
          continue;
        };

        if let Some(id) = reply.get("id").and_then(Value::as_u64) {
          if let Some(answer) = locked(&pending).remove(&id) {
            let result = match reply.get("error") {
              Some(error) => {
                Err(error["message"].as_str().unwrap_or("Chrome DevTools command failed").to_owned())
              }
              None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = answer.send(result);
          }

          continue;
        }

        if let Some(method) = reply.get("method").and_then(Value::as_str) {
          let params = reply.get("params").cloned().unwrap_or_else(|| json!({}));

          handle(&reader_sender, method, &params);
          let _ = reader_events.send((method.to_owned(), params));
        }
      }

      // The connection ended: every command still waiting fails now rather than at its deadline.
      locked(&pending).clear();
    });

    Ok(Self {
      sender,
      events,
      reading,
      writing,
    })
  }

  pub fn is_open(&self) -> bool {
    !self.reading.is_finished() && !self.writing.is_finished()
  }
}

/// Waits for an event of one method, from a subscription taken before the command that causes it.
pub async fn event(
  events: &mut broadcast::Receiver<(String, Value)>,
  method: &str,
  deadline_ms: u64,
) -> WebResult<Value> {
  let waiting = async {
    loop {
      match events.recv().await {
        Ok((name, params)) if name == method => return Ok(params),
        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
        Err(broadcast::error::RecvError::Closed) => {
          return Err(WebError::new("Chrome DevTools connection closed", Failure::Transport));
        }
      }
    }
  };

  tokio::time::timeout(left(deadline_ms, now_ms()), waiting)
    .await
    .unwrap_or_else(|_| Err(deadline_error(method)))
}
