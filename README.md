# explorer

`bmo-explorer`: the web reading of every agent session. It holds the web tools a tool host serves — the web
search, the page reading, research branches and the entries other units read the web through — the shared
web helper that keeps one investigation's reading, the site contract site tools stand on, the browse and
remote-browser runs, and the unit command that starts them. It runs no service of its own: the tool host
starts its tools, bmo-agency starts its runs, and `bmo` hands its commands to `bmo-explorer`.

## Layout

| Folder | Holds |
|---|---|
| `docs/` | the canonical documents: the boundary every other unit matches (`api.md`), the web reading (`web-access.md`), the site contract (`site-contract.md`), the browse runs (`browse-runs.md`), the remote browser (`remote-browser.md`) and the store (`store.md`) |
| `graph/` | the code dependency graph: one node per feature with its files and what it reads or calls, written only by `bmo graph` |
| `src/` | the repository's one crate, `bmo-explorer`, split into its modules: the forms a web read takes, the web layer, the site contract, the tool host's call protocol, the web helper's reading and the store |
| `tests/` | every test, the checks that span the repository among them, and under `tests/site-placed/` the placed site tools held against an installed tool host |
| `tools/` | the executables the tool host places, each with its manifest and schemas: `web_search`, `web_fetch`, `research_branch`, `explorer`, `browse`, `remote_browser`, and the shared web helper under `tools/_shared/web` |

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

The workspace's version, in semver, is the repository's, kept in `VERSION` and the Cargo manifest: a change to a
contract another repository reads (a tool entry, the helper socket, a command's words) raises the major version
(docs/api.md, 「Versioning」).
