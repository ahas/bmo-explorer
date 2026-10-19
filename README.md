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
| `src/` | the repository's one crate, `bmo-explorer`, split into its modules |
| `tests/` | every test, the checks that span the repository among them |

## Build and test

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Version

The workspace's version, in semver, is the repository's, kept in `VERSION` and the Cargo manifest: a change to a
contract another repository reads (a tool entry, the helper socket, a command's words) raises the major version
(docs/api.md, 「Versioning」).
