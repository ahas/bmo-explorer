# Working in this repository

## Principles

(a) each service runs on its own, with no context bleeding in from other repositories: it builds, tests and runs
from its own repository, data and settings, and never reads another repository's code, files, data folders or
tables; (b) services talk to each other only through documented APIs (HTTP routes, events, a documented command),
never by importing another service's code; (c) references run one way: lower code uses the base, and the base
never knows what is below it (no per-provider branch such as `if telegram`, no list of the units below, no
dependency on any service); where two repositories must know the same policy value, the lowest service in the
dependency direction owns it and the ones above read it through that service's API; and each repository holds
only what it uses itself, since the same thing kept in several places is the wrong structure (archive document
#301, 「원칙」).

This repository is `bmo-explorer`: the web tools a tool host serves, the shared web helper, the site contract,
the browse and remote-browser runs, and the unit command `bmo-explorer`. `docs/api.md` is everything it serves
and everything it calls.

## Rules

- **`docs/` is canonical.** Read the document covering a change before changing code. A change that would make
  it wrong or incomplete repairs it first, stating what is true now, and the code is then written to match.
- **No dependency from another repository but the base libraries** `bmo-chat`, `bmo-chat-client`, `bmo-ui` and
  `bmo-markup`: no `bmo-*` crate or package of another unit, no path outside the repository, no git source.
  Another unit is reached over the API `docs/api.md` states. `tests/repository.rs` fails on any of these.
- **One crate.** The repository's code is the package `bmo-explorer` under `src/`, split into modules; each
  tool the tool host places is a member of its own under `tools/`, since each is an executable the host starts.
  No `crates/` folder.
- **English only** in code, comments, strings and tests, and in commit messages. What this repository answers
  is an English sentence or a status code; a room's words are its agent's. Where code or a test inherently
  handles Korean text, the Korean is written plainly, never as an escape or a character code, and the file's
  count of lines holding it stands in `korean-ledger.json`.
- **No agent names, owner names, machine names, home paths or private network addresses** in code, documents or
  commits; tests use neutral names. `tests/repository.rs` reads the names from the list `BMO_NAMES_LIST` names,
  since no repository keeps such a list.
- **Tests live in `tests/`.** No test module sits in `src/` or beside a tool's code.
- **No directory named after its parent** (no `x/x`), and **only the Cargo manifest at the root.**
- **Documents state what is true now:** no change history, no quoted source and no file-and-line pins.
- **A general format is read by a maintained library,** never by a parser written here; a reading no library
  gives says why beside its code.
- **`graph/` is the code dependency graph**, written only by `bmo graph` run with `BMO_SOURCE_DIR` naming this
  checkout; a change that moves a feature's files updates its node in the same commit.

## Checks

A change moves through three stages in order:

1. Writing: `cargo check --workspace --all-targets`.
2. Review: read the whole diff against what the change is for and the document governing it, then
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` (108 columns, 2-space
   indentation, as `rustfmt.toml` sets).
3. Verification: the test targets the change touched, then the whole suite once: `cargo test --workspace`, and
   the placed-site test under `tests/site-placed/`, which runs where `BMO_LIVE_TOOL_HOST` names an installed
   `mcp-host`.

The version is the workspace's, in semver, kept in `VERSION`. `testbed.toml` is the testbed's declaration: how
it tests, checks, builds and deploys the repository.
