//! A PDF's text, for the web reading that would otherwise hand an agent the file's raw bytes
//! (docs/web-access.md, 「A PDF」). PDF is a general format and poppler's `pdftotext` is the maintained
//! reader the machine already carries, so the document is handed to it; it runs as a process of its own,
//! so a hostile file costs that process and not the host. It is waited for until it ends, and ended with the
//! read where the read's caller lets go of it.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;

use crate::web::error::{Failure, WebError, WebResult};

const SIGNATURE: &[u8] = b"%PDF-";

/// Whether a body is a PDF: by the type it was served as, or — a server often sends one as
/// `application/octet-stream` — by the signature it opens with.
pub fn is_pdf(content_type: &str, bytes: &[u8]) -> bool {
  let kind = content_type.trim().to_lowercase();

  kind.starts_with("application/pdf")
    || kind.starts_with("application/x-pdf")
    || bytes.starts_with(SIGNATURE)
}

/// The text `pdftotext` read, each page headed by its number.
pub fn paged_text(read: &str) -> String {
  let mut pages: Vec<String> = read
    .split('\u{c}')
    .map(|page| {
      let trimmed: Vec<&str> = page.lines().map(str::trim_end).collect();

      crate::web::classify::tidy_blank_runs(&trimmed.join("\n")).trim().to_owned()
    })
    .collect();

  // pdftotext ends every page, the last included, with a form feed.
  while pages.last().is_some_and(String::is_empty) {
    pages.pop();
  }

  if pages.iter().all(String::is_empty) {
    return String::new();
  }

  pages
    .iter()
    .enumerate()
    .map(|(index, page)| format!("[page {}]\n{page}", index + 1))
    .collect::<Vec<_>>()
    .join("\n\n")
}

// What a PDF with no text layer reads as stands in the web's forms crate, which a site tool reads it from.
pub use crate::forms::fetch::NO_PDF_TEXT;

/// The PDF's text: `cut` says the body stopped at the transport's byte limit, which makes a file the reader
/// cannot open this machine's limit rather than the site's fault. `layout` keeps each line of a page on one
/// line with its columns spaced apart.
pub async fn pdf_text(bytes: &[u8], url: &str, cut: bool, layout: bool) -> WebResult<String> {
  let reader = std::env::var("BMO_PDFTOTEXT_PATH").unwrap_or_else(|_| "pdftotext".into());
  let mut command = tokio::process::Command::new(&reader);

  command.args(["-enc", "UTF-8"]);
  if layout {
    command.arg("-layout");
  }
  command
    .args(["-", "-"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

  // A machine without the reader is this machine's own failure, never the page's.
  let mut child = command.spawn().map_err(|why| {
    WebError::new(
      format!("{url} is a PDF, and this machine cannot read one: {why} (pdftotext, of poppler-utils)"),
      Failure::BrowserUnavailable,
    )
    .with_url(url)
  })?;
  let mut stdin = child.stdin.take().expect("the reader's input was asked for");
  let handed = bytes.to_vec();
  let feeding = tokio::spawn(async move {
    let _ = stdin.write_all(&handed).await;
  });
  let finished = child.wait_with_output().await;

  feeding.abort();

  let output = finished.map_err(|why| {
    WebError::new(format!("The PDF at {url} could not be read: {why}"), Failure::Transport).with_url(url)
  })?;

  if !output.status.success() {
    let complaint = String::from_utf8_lossy(&output.stderr);
    let reason = complaint.trim().lines().last().unwrap_or("").to_owned();
    let why = if reason.is_empty() { String::new() } else { format!(": {reason}") };

    return Err(
      if cut {
        WebError::new(
          format!("The PDF at {url} was cut at the transport safety limit and could not be read{why}"),
          Failure::TooLarge,
        )
      } else {
        WebError::new(format!("The PDF at {url} could not be read{why}"), Failure::Transport)
      }
      .with_url(url),
    );
  }

  let text = paged_text(&String::from_utf8_lossy(&output.stdout));

  Ok(if text.is_empty() { NO_PDF_TEXT.to_owned() } else { text })
}
