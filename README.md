# explorer

`bmo-explorer`: web reading as an upstream. It holds the client of its six operations — a search of the
engines, a page read as words, a link's preview, a page's markup, the picture at an address and the engines'
standing — the MCP server a harness starts as `bmo-explorer mcp` with its `bmo-explorer` skill, the web tools
a tool host places, the shared web helper that keeps one investigation's reading, and the site contract site
tools stand on. It serves no HTTP of its own and calls no other service.

## Layout

| Folder | Holds |
|---|---|
| `.graph/` | the code dependency graph: one node per feature, with its files, symbols, edges and the owner's words |
| `docs/` | the canonical documents: the boundary every other unit matches (`api.md`), the web reading (`web-access.md`), the site contract (`site-contract.md`) and the store (`store.md`); the contract itself, `openapi.json` |
| `plugin/` | the harness plugin's skill, `skills/bmo-explorer/` |
| `src/` | the repository's one crate, `bmo-explorer`, split into its modules: the client, the MCP server and the contract document, the forms a web read takes, the web layer, the site contract, the tool host's call protocol, the web helper's reading and the store |
| `tests/` | every test, the checks that span the repository among them, and under `tests/site-placed/` the placed site tools held against an installed tool host |
| `tools/` | the executables the tool host places, each with its manifest and schemas: `web_search`, `web_fetch`, `explorer`, and the shared web helper under `tools/_shared/web` |

## Build and test

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
(cd tests/site-placed && bun install && BMO_LIVE_TOOL_HOST=<the installed mcp-host> bun test)
```

The placed-site test runs only where `BMO_LIVE_TOOL_HOST` names an installed `mcp-host`, with `bmo-pocket` beside
it; it builds the web helper, `web_search` and `web_fetch`, places them with two site tools of its own and holds the
host's claims and calls to them.

## Version

The workspace's version, in semver, is the repository's, kept in `VERSION` and the Cargo manifest: `0.1.0` and
then one revision a version bump, `0.1.0-rev.<n>`, until the first release (docs/api.md, 「Versioning」).
