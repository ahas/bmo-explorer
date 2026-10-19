//! A page brought into the record whole, as the browser draws it (docs/api.md, 「The explorer entries」,
//! `explorer/import_page`). Somebody hands over one address and asks for the page as it stands, so nothing is researched
//! and no model runs: the page is opened in the browser, followed to the end of its redirects and drawn, and
//! the reading it takes of itself (`page_import.js`) is handed back with every figure the page draws itself
//! captured as a picture.

use serde::Deserialize;

use crate::web::chrome::{Chrome, RenderedPage, UrlGuard};
use crate::web::clock::now_ms;
use crate::web::error::{Failure, WebError, WebResult};

/// How long the whole reading may take: opening the page, following it, reading it and capturing its
/// figures. The room is waiting, and a page that has not drawn in this long is not going to.
pub const IMPORT_DEADLINE_MS: u64 = 120_000;

/// The most figures one import captures. A page drawing hundreds of icons as inline SVG would otherwise fill
/// the disk with them.
pub const IMPORT_FIGURE_LIMIT: usize = 60;

/// The in-page reading's own source, which the page is handed.
pub const READING: &str = include_str!("page_import.js");

/// The in-page reading as the expression a page evaluates.
pub fn import_expression() -> String {
  format!("({READING})()")
}

// A lazily loaded picture loads when it comes into view, so the page is scrolled through once before it is
// read.
const SCROLL_THROUGH: &str = "(async () => {
  const step = Math.max(400, innerHeight);
  for (let y = 0; y < document.documentElement.scrollHeight && y < 200000; y += step) {
    scrollTo(0, y);
    await new Promise((resolve) => setTimeout(resolve, 60));
  }
  scrollTo(0, 0);
  await new Promise((resolve) => setTimeout(resolve, 200));
})()";

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Rect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

/// One figure the page drew itself.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ImportedFigure {
  pub n: i64,
  pub kind: String,
  pub rect: Rect,
  #[serde(default)]
  pub svg: Option<String>,
  pub caption: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Reference {
  pub text: String,
  pub url: String,
}

/// The page as it read itself.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ImportedPage {
  pub url: String,
  pub title: String,
  #[serde(default)]
  pub description: String,
  #[serde(default)]
  pub keywords: Vec<String>,
  pub body: String,
  #[serde(default)]
  pub figures: Vec<ImportedFigure>,
  #[serde(default)]
  pub references: Vec<Reference>,
}

/// A figure as it was captured out of the drawn page.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedFigure {
  pub figure: ImportedFigure,
  pub png: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedReading {
  pub page: ImportedPage,
  pub figures: Vec<CapturedFigure>,
  /// The figures that could not be captured, which the result names.
  pub missing: Vec<ImportedFigure>,
}

/// The page's own figure, cut out of the drawn page: one standing partly off the page is cut to what is on
/// it, and one with nothing left is not captured.
pub fn clip_of(figure: &ImportedFigure) -> Option<(f64, f64, f64, f64)> {
  let x = figure.rect.x.floor().max(0.0);
  let y = figure.rect.y.floor().max(0.0);
  let width = (figure.rect.x + figure.rect.width).ceil() - x;
  let height = (figure.rect.y + figure.rect.height).ceil() - y;

  (width >= 1.0 && height >= 1.0).then_some((x, y, width, height))
}

/// A page held open for the reading: what it evaluates, and the regions it is drawn in.
pub trait Page: Send + Sync {
  fn evaluate(
    &self,
    expression: &str,
    deadline_ms: u64,
  ) -> impl Future<Output = WebResult<serde_json::Value>> + Send;
  fn screenshot(
    &self,
    clip: (f64, f64, f64, f64),
    deadline_ms: u64,
  ) -> impl Future<Output = WebResult<Vec<u8>>> + Send;
}

impl Page for RenderedPage {
  async fn evaluate(&self, expression: &str, deadline_ms: u64) -> WebResult<serde_json::Value> {
    RenderedPage::evaluate(self, expression, deadline_ms).await
  }

  async fn screenshot(&self, clip: (f64, f64, f64, f64), deadline_ms: u64) -> WebResult<Vec<u8>> {
    RenderedPage::screenshot(self, clip, deadline_ms).await
  }
}

/// The reading of a page already held open: scrolled through, read, and its figures captured.
pub async fn read_held(page: &impl Page, deadline_ms: u64) -> WebResult<ImportedReading> {
  page.evaluate(SCROLL_THROUGH, deadline_ms).await?;

  let read: ImportedPage = serde_json::from_value(page.evaluate(&import_expression(), deadline_ms).await?)
    .map_err(|why| {
      WebError::new(format!("the page's reading of itself could not be read: {why}"), Failure::Transport)
    })?;
  let mut figures = Vec::new();
  let mut missing = Vec::new();

  for figure in &read.figures {
    let clip = clip_of(figure).filter(|_| figures.len() < IMPORT_FIGURE_LIMIT);
    let Some(clip) = clip else {
      missing.push(figure.clone());
      continue;
    };

    match page.screenshot(clip, deadline_ms).await {
      Ok(png) => figures.push(CapturedFigure {
        figure: figure.clone(),
        png,
      }),
      Err(why) => {
        eprintln!("figure {} of {} was not captured: {why}", figure.n, read.url);
        missing.push(figure.clone());
      }
    }
  }

  Ok(ImportedReading {
    page: read,
    figures,
    missing,
  })
}

/// One page, read whole in a Chrome opened for this reading and closed with it, every request it makes
/// checked the way every outward request of the web layer is.
pub async fn import_page(url: &str, guard: UrlGuard) -> WebResult<ImportedReading> {
  let deadline_ms = now_ms() + IMPORT_DEADLINE_MS;
  let browser = Chrome::new(guard);
  let read = browser
    .inspect(url, deadline_ms, async |page| read_held(&page, deadline_ms).await)
    .await;

  browser.shut().await;
  read
}
