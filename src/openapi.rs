//! The repository's OpenAPI 3.1 document (docs/api.md, 「The contract」): no paths, since the tools are the
//! contract; the error shape written from the client's own type; the harness plugin; and the MCP server's tools
//! from the table it lists, each input and output written from the client's types. The repository keeps it as
//! `docs/openapi.json`, and a test holds the kept document to this one.

use serde_json::{Map, Value, json};

use crate::client::ClientError;
use crate::mcp::{ENTRY, MCP, PLUGIN, generator, tools};

/// Where the schemas the tools refer to stand in the document.
const SCHEMAS: &str = "/components/schemas";

const DESCRIPTION: &str = "Web reading: the search engines asked directly, a page read as words, a link's \
  preview, a page's markup as served, the picture at an address, and the engines that answer with their \
  standing. No HTTP of its own: the tools are the contract, served by its MCP server and placed with a tool \
  host, each a thin call of one function of the client this repository publishes as its Rust crate. It calls no \
  other service. Bodies are JSON with camelCase keys; times are integers of epoch milliseconds; a failure is the \
  error shape, its code one of bad-request, unsafe-address, not-markup, too-large, upstream-failed and failed.";

/// The document as JSON.
pub fn openapi() -> Value {
  let mut generator = generator(SCHEMAS);

  // Every failure is answered in this shape, so it stands among the schemas whether a tool names it or not.
  generator.subschema_for::<ClientError>();

  let tools: Vec<Value> = tools()
    .iter()
    .map(|tool| {
      json!({
        "name": tool.name,
        "description": tool.description,
        "input": tool.input(&mut generator).to_value(),
        "output": tool.output(&mut generator).to_value(),
        "route": null,
        "function": format!("bmo_explorer::client::Explorer::{}", tool.function),
      })
    })
    .collect();
  let schemas: Map<String, Value> = generator.take_definitions(true);

  json!({
    "openapi": "3.1.0",
    "info": {
      "title": "bmo-explorer",
      "version": crate::VERSION,
      "description": DESCRIPTION,
    },
    "x-layer": "upstream",
    "x-calls": [],
    "x-harness": {
      "plugin": PLUGIN,
      "mcp": MCP,
      "entry": ENTRY,
    },
    "paths": {},
    "components": {
      "schemas": schemas,
    },
    "x-tools": tools,
  })
}

/// The document as `docs/openapi.json` keeps it: pretty, ending in a line break.
pub fn openapi_json() -> String {
  let mut written = serde_json::to_string_pretty(&openapi()).expect("the document is JSON");

  written.push('\n');
  written
}
