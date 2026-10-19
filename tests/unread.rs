//! The pages no road read, as `bmo-explorer tool unread-pages` reads them out of the store (docs/site-contract.md,
//! 「The list of pages no road read」), each beside the site tool that reads its site.

use std::path::Path;
use std::process::Command;

use axum::extract::Json;
use axum::routing::{get, post};
use bmo_explorer::store;
use bmo_explorer::unread::{UnreadSite, catalogue_reading, unread_pages_lines, unread_sites};
use serde_json::{Value, json};

const HOUR: i64 = 60 * 60 * 1_000;
// 2026-10-08 01:00 UTC.
const NOW: i64 = 1_791_421_200_000;

// One failure written: its site, address, shape, detail, the roads it was written by, the run, and when.
async fn failed(
  place: &Path,
  (site, url, shape, detail, via, run, at): (&str, &str, &str, &str, &str, &str, i64),
) {
  let mut connection = store::open(place).await.unwrap();

  sqlx::query(
    "INSERT INTO page_read_failures (site, url, shape, detail, via, run, recorded_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
  )
  .bind(site)
  .bind(url)
  .bind(shape)
  .bind(detail)
  .bind(via)
  .bind(run)
  .bind(at)
  .execute(&mut connection)
  .await
  .unwrap();
}

async fn written(root: &Path) {
  let place = store::place(root);

  failed(
    &place,
    (
      "getyarn.io",
      "https://getyarn.io/a",
      "script-only",
      "",
      "direct+chrome",
      "search",
      NOW - 48 * HOUR,
    ),
  )
  .await;
  failed(
    &place,
    (
      "example.org",
      "https://example.org/x",
      "refused",
      "",
      "direct+chrome",
      "search",
      NOW - 24 * HOUR,
    ),
  )
  .await;
  failed(
    &place,
    (
      "getyarn.io",
      "https://getyarn.io/b",
      "challenge",
      "Just a moment...",
      "direct+chrome",
      "browse",
      NOW,
    ),
  )
  .await;
  failed(
    &place,
    ("elsewhere.net", "https://elsewhere.net/", "gone", "", "web_fetch", "search", NOW),
  )
  .await;
}

// Are read back a site at a time, its last failure named, and only those written by both roads.
#[tokio::test]
async fn are_read_back_a_site_at_a_time_its_last_failure_named() {
  let root = tempfile::tempdir().unwrap();

  written(root.path()).await;

  assert_eq!(
    unread_sites(&store::place(root.path())).await.unwrap(),
    [
      UnreadSite {
        site: "getyarn.io".into(),
        failures: 2,
        first_at: NOW - 48 * HOUR,
        last_at: NOW,
        url: Some("https://getyarn.io/b".into()),
        shape: "challenge".into(),
        detail: "Just a moment...".into(),
        run: Some("browse".into()),
      },
      UnreadSite {
        site: "example.org".into(),
        failures: 1,
        first_at: NOW - 24 * HOUR,
        last_at: NOW - 24 * HOUR,
        url: Some("https://example.org/x".into()),
        shape: "refused".into(),
        detail: "".into(),
        run: Some("search".into()),
      },
    ]
  );
}

// Two entries as the web namespace's facts carry the catalogue.
fn catalogue() -> Vec<Value> {
  vec![
    json!({ "name": "book_prices", "what": "A book's prices.", "site": "Books (books.example.com)",
      "reads": ["books.example.com"] }),
    json!({ "name": "blog_posts", "what": "A blog post.", "site": "Blogs (blog.example.com)",
      "reads": ["blog.example.com"] }),
  ]
}

#[test]
fn an_unread_site_is_read_by_the_tool_whose_read_sites_name_it() {
  assert_eq!(catalogue_reading(&catalogue(), "blog.example.com").as_deref(), Some("blog_posts"));
  assert_eq!(catalogue_reading(&catalogue(), "example.com"), None);
}

#[test]
fn an_unread_site_is_named_with_its_last_page_and_the_tool_that_reads_it() {
  let lines = unread_pages_lines(&[(
    UnreadSite {
      site: "blog.example.com".into(),
      failures: 2,
      first_at: 0,
      last_at: 60_000,
      url: Some("https://blog.example.com/a/1".into()),
      shape: "script-only".into(),
      detail: "".into(),
      run: Some("search".into()),
    },
    Some("blog_posts".into()),
  )]);

  assert_eq!(
    lines,
    [
      "1 site read neither directly nor in Chrome",
      "",
      "blog.example.com — 2 failures, first 1970-01-01 09:00 KST, last 1970-01-01 09:01 KST",
      "  last: https://blog.example.com/a/1 — script-only (search)",
      "  tool: blog_posts reads this site, so the failure is that tool's to repair",
    ]
  );
}

// The command run against a tool host on its control socket, whose site catalogue names a tool reading one of the
// sites and whose claim names another's tool for the other site's last address.
#[tokio::test(flavor = "multi_thread")]
async fn the_command_prints_the_list_each_site_beside_the_tool_the_tool_host_names() {
  let root = tempfile::tempdir().unwrap();
  let socket = root.path().join("mcp-host.sock");

  written(root.path()).await;

  let app = axum::Router::new()
    .route(
      "/facts/web",
      get(|| async { Json(json!({ "sites": [{ "name": "yarn_clips", "reads": ["getyarn.io"] }] })) }),
    )
    .route(
      "/claims",
      post(|Json(asked): Json<Value>| async move {
        let entry = (asked["address"] == "https://example.org/x").then_some("web/example_pages");

        Json(json!({ "entry": entry }))
      }),
    );
  let listener = tokio::net::UnixListener::bind(&socket).unwrap();

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

  let done = Command::new(env!("CARGO_BIN_EXE_bmo-explorer"))
    .args(["tool", "unread-pages"])
    .env("BMO_DATA_DIR", root.path())
    .env("BMO_MCP_HOST_SOCKET", &socket)
    .output()
    .unwrap();
  let out = String::from_utf8_lossy(&done.stdout);

  assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
  assert!(out.starts_with("2 sites read neither directly nor in Chrome"), "{out}");
  assert!(out.contains("  tool: yarn_clips reads this site"), "{out}");
  assert!(out.contains("  tool: example_pages reads this site"), "{out}");
  assert!(out.contains("  showed: Just a moment..."), "{out}");

  let nouns = Command::new(env!("CARGO_BIN_EXE_bmo-explorer")).arg("nouns").output().unwrap();

  assert!(String::from_utf8_lossy(&nouns.stdout).lines().any(|noun| noun == "tool unread-pages"));
}
