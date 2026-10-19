//! `bmo-explorer tool unread-pages` (docs/site-contract.md, 「The list of pages no road read」): the one list of
//! pages the research runs could read neither directly nor in the real Chrome, a site at a time, out of this
//! repository's store, each beside the site tool that reads the site where there is one.

use std::path::Path;

use chrono::DateTime;
use serde_json::Value;
use sqlx::Row;

use crate::clients::tool_host::ToolHost;
use crate::site::unread::BOTH_ROADS;
use crate::store;

/// One site the research runs could not read, with the last page of it they met.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadSite {
  pub site: String,
  pub failures: i64,
  pub first_at: i64,
  pub last_at: i64,
  pub url: Option<String>,
  pub shape: String,
  pub detail: String,
  pub run: Option<String>,
}

// A moment in epoch milliseconds, to the minute in Korean time, as the list has always been read.
fn kst(ms: i64) -> String {
  DateTime::from_timestamp_millis(ms + 9 * 60 * 60 * 1_000)
    .map_or_else(|| ms.to_string(), |at| format!("{} KST", at.format("%Y-%m-%d %H:%M")))
}

/// Every site the runs could read neither directly nor in Chrome, the most recently failed first, out of the
/// store at `place`.
pub async fn unread_sites(place: &Path) -> Result<Vec<UnreadSite>, String> {
  let mut connection = store::open(place).await?;
  let rows = sqlx::query(
    "SELECT grouped.site, grouped.failures, grouped.first_at, grouped.last_at, last.url, last.shape, last.detail, last.run
     FROM (
       SELECT site, COUNT(*) AS failures, MIN(recorded_at) AS first_at, MAX(recorded_at) AS last_at, MAX(id) AS last_id
       FROM page_read_failures
       WHERE via = $1
       GROUP BY site
     ) AS grouped
     JOIN page_read_failures AS last ON last.id = grouped.last_id
     ORDER BY grouped.last_at DESC, grouped.site",
  )
  .bind(BOTH_ROADS)
  .fetch_all(&mut connection)
  .await
  .map_err(|why| format!("the pages no road read could not be read: {why}"))?;

  rows
    .iter()
    .map(|row| {
      let read = |why: sqlx::Error| format!("a page no road read does not read: {why}");

      Ok(UnreadSite {
        site: row.try_get("site").map_err(read)?,
        failures: row.try_get("failures").map_err(read)?,
        first_at: row.try_get("first_at").map_err(read)?,
        last_at: row.try_get("last_at").map_err(read)?,
        url: row.try_get("url").map_err(read)?,
        shape: row.try_get("shape").map_err(read)?,
        detail: row.try_get("detail").map_err(read)?,
        run: row.try_get("run").map_err(read)?,
      })
    })
    .collect()
}

/// The unread sites as the command prints them, a site a paragraph, each beside the tool that reads it where there
/// is one.
pub fn unread_pages_lines(sites: &[(UnreadSite, Option<String>)]) -> Vec<String> {
  let mut lines = vec![format!(
    "{} site{} read neither directly nor in Chrome",
    sites.len(),
    if sites.len() == 1 { "" } else { "s" }
  )];

  for (one, reading) in sites {
    lines.push(String::new());
    lines.push(format!(
      "{} — {} failure{}, first {}, last {}",
      one.site,
      one.failures,
      if one.failures == 1 { "" } else { "s" },
      kst(one.first_at),
      kst(one.last_at)
    ));
    lines.push(format!(
      "  last: {} — {}{}",
      one.url.as_deref().unwrap_or("(no address)"),
      one.shape,
      one.run.as_ref().map(|run| format!(" ({run})")).unwrap_or_default()
    ));

    if !one.detail.is_empty() {
      lines.push(format!("  showed: {}", one.detail));
    }

    if let Some(tool) = reading {
      lines.push(format!("  tool: {tool} reads this site, so the failure is that tool's to repair"));
    }
  }

  lines
}

/// The catalogue's first tool whose read sites name a site.
pub fn catalogue_reading(catalogue: &[Value], site: &str) -> Option<String> {
  catalogue
    .iter()
    .find(|tool| tool["reads"].as_array().into_iter().flatten().any(|named| named == site))
    .and_then(|tool| tool["name"].as_str().map(str::to_owned))
}

/// The site tool that reads an unread site: the one the tool host names as claiming its last address, else the
/// catalogue's first whose read sites name the site.
pub async fn site_tool_reading(
  host: &ToolHost,
  catalogue: &[Value],
  site: &UnreadSite,
  data_root: &Path,
) -> Result<Option<String>, String> {
  if let Some(url) = &site.url
    && let Some(entry) = host.claimed_by(url, data_root).await?
  {
    return Ok(Some(entry));
  }

  Ok(catalogue_reading(catalogue, &site.site))
}

/// The list as the command prints it, every site beside the tool that reads it, read off the tool host.
pub async fn unread_pages(data_root: &Path, host: &ToolHost) -> Result<Vec<String>, String> {
  let sites = unread_sites(&store::place(data_root)).await?;

  if sites.is_empty() {
    return Ok(unread_pages_lines(&[]));
  }

  let catalogue = host.site_catalogue().await?;
  let mut read = Vec::with_capacity(sites.len());

  for site in sites {
    let reading = site_tool_reading(host, &catalogue, &site, data_root).await?;

    read.push((site, reading));
  }

  Ok(unread_pages_lines(&read))
}
