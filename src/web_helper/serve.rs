//! The web helper's service (the tool host's docs/mcp-host.md, 「Shared helpers」): the record's one web reading opened as
//! the helper starts, every request on its socket answered out of it, and the reading closed as the host
//! asks the helper to end. A core tool's request is answered with its tool result; a placed site tool's road
//! with what the road gave or the failure it met, and the guard round its call with the guard's verdict.

use std::path::Path;
use std::sync::Arc;

use crate::site::contract::Reach;
use crate::site::helper::WebAsked;
use crate::tool::{Answer, Content, Session};
use crate::web::picture::{VIEWED_PICTURE_EDGE, fetch_raster, view};
use crate::web::search::{DEFAULT_SEARCH_COUNT, MAX_SEARCH_COUNT, render_search_results};
use crate::web::session::{DEFAULT_FETCH_CHARS, MAX_FETCH_CHARS, render_slice};
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedReadHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};

use crate::web_helper::branch;
use crate::web_helper::run::WebRun;

/// What the reading answers to one request, as it is written on the answer's line.
pub async fn answer(run: &WebRun, asked: WebAsked) -> Value {
  match asked {
    WebAsked::Search {
      query,
      count,
      language,
      category,
    } => written(search(run, &query, count, language.as_deref(), category.as_deref()).await),
    WebAsked::Fetch {
      url,
      offset,
      max_chars,
      raw,
    } => written(fetch(run, &url, offset, max_chars, raw).await),
    WebAsked::Captcha {
      url,
      offset,
      max_chars,
    } => written(captcha(run, &url, offset, max_chars).await),
    WebAsked::Picture { image } => written(picture(run, &image).await),
    WebAsked::Branch { angle } => written(Answer::text(branch::open(run, &angle).await)),
    WebAsked::Page { url, request } => written(run.page(&url, request).await),
    WebAsked::Rendered {
      url,
      fresh,
      arrival,
    } => written(run.rendered(&url, fresh, arrival).await),
    WebAsked::Engine { query, count } => written(run.engine(&query, count).await),
    WebAsked::Document { url, slice } => written(run.document(&url, slice).await),
    WebAsked::Head { url } => written(run.head(&url).await),
    WebAsked::Opening { engine } => written(opening(run, engine)),
    WebAsked::Ended { engine, outcome } => written(ended(run, engine, outcome)),
  }
}

fn written(said: impl Serialize) -> Value {
  serde_json::to_value(said).unwrap_or_else(|why| {
    serde_json::to_value(Answer::failure(format!("the web helper could not write its answer: {why}")))
      .unwrap_or_default()
  })
}

/// The guard's refusal of a site tool's call before it reads: for a tool reading through the engines the
/// engines' own refusals, the plain web search's included, so asking again through a tool meets the same wall;
/// for any other tool, the page budget spent, the call taking one page of it.
fn opening(run: &WebRun, engine: bool) -> Option<&'static str> {
  if engine { run.guard.search_stopped() } else { run.guard.take_page(None) }
}

/// A site tool's outcome as the reading answers it: an engine tool's answer counted as a search, and a failure
/// answered as a failed page is.
fn ended(run: &WebRun, engine: bool, outcome: crate::web::WebResult<String>) -> Answer {
  match outcome {
    Ok(text) => {
      if engine {
        run.guard.note_search(&text);
      }

      Answer::text(text)
    }
    Err(error) => run.failed(&error),
  }
}

async fn search(
  run: &WebRun,
  query: &str,
  count: Option<usize>,
  language: Option<&str>,
  category: Option<&str>,
) -> Answer {
  if let Some(refused) = run.guard.search_stopped() {
    return Answer::text(refused);
  }

  let count = count.map_or(DEFAULT_SEARCH_COUNT, |count| count.clamp(1, MAX_SEARCH_COUNT));

  match run.search(query, Some(count), language, category).await {
    Ok(found) => {
      let text = render_search_results(query, &found.hits, &found.unresponsive, &found.off_topic);

      run.guard.note_search(&text);
      Answer::text(text)
    }
    Err(error) => run.failed(&error),
  }
}

async fn fetch(
  run: &WebRun,
  url: &str,
  offset: Option<usize>,
  max_chars: Option<usize>,
  raw: bool,
) -> Answer {
  if let Some(refused) = run.guard.take_page(Some(url)) {
    return Answer::text(refused);
  }

  let max_chars = max_chars.map_or(DEFAULT_FETCH_CHARS, |chars| chars.clamp(1, MAX_FETCH_CHARS));

  match run.read_page(url, offset, Some(max_chars), raw, false).await {
    Ok(slice) => Answer::text(render_slice(&slice)),
    Err(error) => run.failed(&error),
  }
}

// The forcing read: the page as `web_fetch` answers it, headed by what the challenge attempt came to
// (docs/api.md, 「The captcha read」).
async fn captcha(run: &WebRun, url: &str, offset: Option<usize>, max_chars: Option<usize>) -> Answer {
  if let Some(refused) = run.guard.take_page(Some(url)) {
    return Answer::text(refused);
  }

  let max_chars = max_chars.map_or(DEFAULT_FETCH_CHARS, |chars| chars.clamp(1, MAX_FETCH_CHARS));

  match run.solve_captcha(url, offset, Some(max_chars)).await {
    Ok(read) => Answer::text(format!("{}\n{}", read.lines(), render_slice(&read.slice))),
    Err(error) => run.failed(&error),
  }
}

async fn picture(run: &WebRun, address: &str) -> Answer {
  let viewed = async {
    let image =
      fetch_raster(address, &**run.session.store(), run.session.check(), &**run.session.transport())
        .await?;

    view(&image)
  };

  match viewed.await {
    Ok(picture) => {
      let size = if picture.source_width == picture.width {
        format!("{}x{}", picture.width, picture.height)
      } else {
        format!(
          "{}x{}, shown at {}x{}",
          picture.source_width, picture.source_height, picture.width, picture.height
        )
      };

      Answer {
        content: vec![
          Content::Text {
            text: format!("IMAGE {address} ({size})"),
          },
          Content::Image {
            data: base64::engine::general_purpose::STANDARD.encode(&picture.jpeg),
            mime_type: picture.kind.to_owned(),
          },
        ],
        is_error: false,
        structured: None,
      }
    }
    Err(error) => run.failed(&error),
  }
}

// The viewer's description names its edge as the picture reading has it.
const _: () = assert!(VIEWED_PICTURE_EDGE == 1568);

/// One connection: one request read off its line, answered on one line; a line that does not read is
/// answered as a failure. The asker holds its side open until it is answered, and an asker that ends it first
/// has let go of its request: the read under way for it is dropped with everything it set off.
pub async fn one(run: Arc<WebRun>, stream: UnixStream) {
  let (reading, mut writing) = stream.into_split();
  let mut reading = BufReader::new(reading);
  let mut line = String::new();

  if reading.read_line(&mut line).await.is_err() {
    return;
  }

  let answer = match serde_json::from_str::<WebAsked>(&line) {
    Ok(asked) => tokio::select! {
      answered = answer(&run, asked) => answered,
      () = let_go(&mut reading) => return,
    },
    Err(why) => written(Answer::failure(format!("the web helper was asked what it does not read: {why}"))),
  };
  let mut said = answer.to_string();

  said.push('\n');
  let _ = writing.write_all(said.as_bytes()).await;
}

// Returns once the asker has ended its side of the connection; anything more it writes is not read as a request.
async fn let_go(reading: &mut BufReader<OwnedReadHalf>) {
  let mut rest = [0_u8; 256];

  while matches!(reading.read(&mut rest).await, Ok(read) if read > 0) {}
}

/// The helper's whole life for the record: its reading opened, the socket bound and said on standard output,
/// every connection answered side by side until `SIGTERM`, and the reading closed.
pub async fn serve(session: &Session, socket: &Path) -> Result<(), String> {
  let run = Arc::new(WebRun::open(session)?);
  let listener =
    UnixListener::bind(socket).map_err(|why| format!("{} could not be bound: {why}", socket.display()))?;
  let mut ending =
    signal(SignalKind::terminate()).map_err(|why| format!("SIGTERM could not be awaited: {why}"))?;

  println!("listening on {}", socket.display());

  loop {
    tokio::select! {
      _ = ending.recv() => break,
      accepted = listener.accept() => match accepted {
        Ok((stream, _)) => {
          tokio::spawn(one(run.clone(), stream));
        }
        Err(why) => eprintln!("web helper: a connection could not be taken: {why}"),
      },
    }
  }

  run.close().await;
  Ok(())
}
