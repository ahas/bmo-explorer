//! The services a browse calls, stood in by one server answering their documented routes and keeping every call
//! it was asked, so a test reads what the run sent each of them.

use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

/// One call the stand-in was asked: its method, its path with its query, and its body.
#[derive(Clone, Debug)]
pub struct Asked {
  pub method: String,
  pub path: String,
  pub body: Value,
}

/// What the stand-in answers a call with: a status and a body.
pub type Answers = Arc<dyn Fn(&Asked) -> (StatusCode, Value) + Send + Sync>;

/// The stand-in running: its address, and every call it was asked, in order.
pub struct StandIn {
  pub address: String,
  pub asked: Arc<Mutex<Vec<Asked>>>,
}

impl StandIn {
  /// The calls asked of a path, in order.
  pub fn calls(&self, method: &str, path: &str) -> Vec<Asked> {
    self
      .asked
      .lock()
      .unwrap()
      .iter()
      .filter(|asked| asked.method == method && asked.path == path)
      .cloned()
      .collect()
  }
}

/// A stand-in answering every call by `answers`.
pub async fn serve(answers: Answers) -> StandIn {
  let asked = Arc::new(Mutex::new(Vec::new()));
  let kept = asked.clone();
  let app = axum::Router::new().fallback(move |method: Method, uri: Uri, body: Bytes| {
    let kept = kept.clone();
    let answers = answers.clone();

    async move {
      let call = Asked {
        method: method.to_string(),
        path: uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
      };
      let (status, said) = answers(&call);

      kept.lock().unwrap().push(call);

      let answer: Response = if status == StatusCode::NO_CONTENT {
        status.into_response()
      } else {
        (status, axum::Json(said)).into_response()
      };

      answer
    }
  });
  let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
  let address = format!("http://{}", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

  StandIn { address, asked }
}

/// A data root whose settings point every unit at the stand-in.
pub fn data_root(address: &str) -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  let folder = root.path().join("explorer");

  std::fs::create_dir_all(&folder).unwrap();
  std::fs::write(
    folder.join("settings.toml"),
    format!(
      "[units]\nagency = \"{address}\"\nreceiver = \"{address}\"\nartifact = \"{address}\"\ngate = \"{address}\"\nnode = \"{address}\"\n"
    ),
  )
  .unwrap();
  root
}
