//! The contract's tests (docs/api.md, 「The contract」): the kept document is the one the code writes, and it
//! states the MCP server's seven tools and no route.

use bmo_explorer::openapi::{openapi, openapi_json};

#[test]
fn the_kept_document_is_the_one_the_code_writes() {
  let kept = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/openapi.json")).unwrap();

  assert!(
    kept == openapi_json(),
    "docs/openapi.json differs from what the code writes; write it again with \
     cargo run -- openapi > docs/openapi.json"
  );
}

#[test]
fn the_document_states_the_seven_tools_the_plugin_and_no_route() {
  let document = openapi();
  let tools: Vec<&str> = document["x-tools"]
    .as_array()
    .unwrap()
    .iter()
    .map(|tool| tool["name"].as_str().unwrap())
    .collect();

  assert_eq!(
    tools,
    [
      "explorer_web_search",
      "explorer_search_images",
      "explorer_web_fetch",
      "explorer_link_preview",
      "explorer_fetch_page",
      "explorer_fetch_raster",
      "explorer_search_engines",
    ]
  );
  assert_eq!(document["paths"], serde_json::json!({}));
  assert_eq!(document["x-calls"], serde_json::json!([]));
  assert_eq!(document["x-harness"]["mcp"], "bmo-explorer mcp");
  assert_eq!(document["x-harness"]["plugin"], "bmo-explorer");
  assert_eq!(document["info"]["version"], env!("CARGO_PKG_VERSION"));
}
