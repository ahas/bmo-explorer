use bmo_explorer::web::clock::now_ms;
use bmo_explorer::web::error::Failure;
use bmo_explorer::web::error::WebError;
use bmo_explorer::web::error::WebResult;
use std::sync::Mutex;

use serde_json::json;

use bmo_explorer::web::import::*;

// A drawn page standing in for Chrome: its reading of itself, and which regions fail to capture.
struct Drawn {
  reading: serde_json::Value,
  failing: Vec<i64>,
  shots: Mutex<Vec<(f64, f64, f64, f64)>>,
}

impl Page for Drawn {
  async fn evaluate(&self, expression: &str, _: u64) -> WebResult<serde_json::Value> {
    Ok(if expression.contains("readImportedPage") {
      self.reading.clone()
    } else {
      serde_json::Value::Null
    })
  }

  async fn screenshot(&self, clip: (f64, f64, f64, f64), _: u64) -> WebResult<Vec<u8>> {
    self.shots.lock().unwrap().push(clip);

    let n = self.shots.lock().unwrap().len() as i64;

    if self.failing.contains(&n) {
      return Err(WebError::new("Chrome returned no screenshot", Failure::Transport));
    }

    Ok(vec![n as u8])
  }
}

fn figure(n: i64, x: f64, y: f64, width: f64, height: f64) -> serde_json::Value {
  json!({ "n": n, "kind": "svg", "rect": { "x": x, "y": y, "width": width, "height": height }, "svg": "<svg></svg>", "caption": format!("Figure {n}") })
}

fn reading(figures: Vec<serde_json::Value>) -> serde_json::Value {
  json!({ "url": "https://page.test/a", "title": "A", "description": "", "keywords": [], "body": "words", "figures": figures, "references": [] })
}

// Carried from packages/web/page-import.test.ts: captures every figure the page drew, cut out where the page drew it
#[tokio::test]
async fn every_figure_the_page_drew_is_captured_where_it_was_drawn() {
  let drawn = Drawn {
    reading: reading(vec![figure(1, 10.4, 20.6, 100.2, 50.0), figure(2, -5.0, 0.0, 30.0, 30.0)]),
    failing: Vec::new(),
    shots: Mutex::new(Vec::new()),
  };
  let read = read_held(&drawn, now_ms() + 10_000).await.unwrap();

  assert_eq!(read.figures.len(), 2);
  assert!(read.missing.is_empty());
  assert_eq!(*drawn.shots.lock().unwrap(), [(10.0, 20.0, 101.0, 51.0), (0.0, 0.0, 25.0, 30.0)]);
}

// Carried from packages/web/page-import.test.ts: a figure that would not capture costs only itself and is handed back as missing
#[tokio::test]
async fn a_figure_that_would_not_capture_costs_only_itself() {
  let drawn = Drawn {
    reading: reading(vec![
      figure(1, 0.0, 0.0, 40.0, 40.0),
      figure(2, 0.0, 50.0, 40.0, 40.0),
      figure(3, -80.0, 0.0, 40.0, 40.0),
    ]),
    failing: vec![1],
    shots: Mutex::new(Vec::new()),
  };
  let read = read_held(&drawn, now_ms() + 10_000).await.unwrap();

  assert_eq!(read.figures.iter().map(|one| one.figure.n).collect::<Vec<_>>(), [2]);
  assert_eq!(read.missing.iter().map(|one| one.n).collect::<Vec<_>>(), [1, 3]);
}

#[test]
fn the_reading_handed_to_the_page_is_its_own_function_called() {
  let expression = import_expression();

  assert!(expression.starts_with("(// The reading a page takes of itself"));
  assert!(expression.contains("function readImportedPage()"));
  assert!(expression.ends_with("})()") || expression.trim_end().ends_with(")()"));
}
