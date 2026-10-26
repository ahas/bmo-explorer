# The boundary

bmo-explorer is web reading as an upstream: the search engines asked directly, a page read as words, a link's
preview, a page's markup as served, the picture at an address, and the engines' standing. It serves no HTTP of
its own: its tools are its contract, served by its MCP server and placed with a tool host, and the client behind
them is the Rust library of this repository. It calls no other service. This document is the whole of what it
serves; another repository reaches it through nothing else.

```mermaid
flowchart TD
  H[A harness] -->|stdio| M[bmo-explorer mcp]
  T[A tool host] -->|call protocol| P[The placed tools and the web helper]
  M --> C[The client]
  P --> W[The web layer]
  C --> W
  W --> S[The store]
```

## Versioning

- **The repository has one version,** in `VERSION` and the Cargo manifest, which the MCP server's
  `serverInfo`, the OpenAPI document and the harness plugin all answer.
- **It is numbered and moved as bmo-chat's `docs/conventions.md` says (「Versions」):** only at a release the
  owner declares — before the first release `0.1.0`, then `0.1.0-rev.<n>`, one revision a release — and a commit,
  a deploy or a contract change moves none; a deployment is told apart by its commit.

## Settings and data

- **The data root** is the folder `--data-root` names, or `BMO_DATA_DIR` where none is named; a placed tool
  reads it as `BMO_DATA_DIR` of its session record (「What a placed tool reads of its record」). This repository
  keeps its own folder in it, `explorer/`, and reads no other (docs/store.md).
- **It reads no settings file.** What the web layer is built with stands beside the code that reads it: the
  engine roster with each engine's category and the rests after a refusal (docs/web-access.md, 「Rosters」), and
  the arguments every Chrome it starts carries (docs/web-access.md, 「How Chrome is started」).
- **Every answer this repository gives is English,** a sentence or a status code.

## The contract

- **`docs/openapi.json` is the contract,** an OpenAPI 3.1 document written from the client's own types: no
  paths, the common schemas (`Error`, `Health`, `Event`, `EventPage`), the plugin under `x-harness`, the six
  tools under `x-tools` with their input and output schemas and the client function each calls, and
  `x-calls` empty. A test fails where the committed document differs from what the code writes;
  `bmo-explorer openapi` prints it.
- **The wire format is bmo-chat's** (`docs/conventions.md`, 「The common wire format」): camelCase keys, times
  in epoch milliseconds, a failure as `code` and `error`.

## The client

- **The client is this crate's `client` module,** the one place the six operations are done. It is opened over
  a data root and holds one web reading, its own direct client, Chrome and cookie stores, until it is closed
  (docs/web-access.md, 「One session, one investigation」).
- **A failure is the wire format's error:**

| `code` | When |
|---|---|
| `bad-request` | the input is not the shape the operation takes: an empty query, an engine no roster names, a width of 0 |
| `unsafe-address` | the address check refused the address before the site was asked (docs/web-access.md, 「Address checks」) |
| `not-markup` | `fetchPage` was answered with something other than markup |
| `too-large` | the answer was more than the operation reads |
| `upstream-failed` | the site or every engine asked failed: an HTTP error, a block, no answer in time; the sentence says which |
| `failed` | this machine failed: the store, or a Chrome that could not start |

- **No TypeScript package is published:** the contract has no route a package could call, and no browser
  runs any of it.

## The MCP server

- **`bmo-explorer mcp`** serves the six tools on standard input and output to a harness that starts it, over
  the data root the command names. Its instructions say what a harness does first: search with
  `explorer_web_search` and read a hit with `explorer_web_fetch`.
- **Each tool is a thin call of one client function,** answered as structured content. A failure is a tool
  result marked `isError` whose text is the error as JSON, `code` and `error`.

| Tool | Client function | Takes | Answers |
|---|---|---|---|
| `explorer_web_search` | `web_search` | `q`; `engines`, each an engine's roster name or a category standing for its engines, none for the general ones; `limit?`, 1 to 30, 10 where absent; `language?` | `hits`, each `title`, `url`, `snippet` and `engine`, mixed one from each engine in turn; `unresponsive` and `offTopic`, the engines that did not answer or answered off the query, each with why |
| `explorer_web_fetch` | `web_fetch` | `url` | `url`, the address the read ended at; `title`, null where the page has none; `text`, what a reader sees, at most 100,000 characters; `links`, every address the page links to once, absolute; `truncated`, true where the text was cut |
| `explorer_link_preview` | `link_preview` | `url` | `url`; `title`, `description` and `image` as the page's head declares them for a preview, each null where it declares none |
| `explorer_fetch_page` | `fetch_page` | `url` | `url`; `status`; `contentType`, as the server sent it; `html`, at most 512 KiB of the markup decoded as its charset says |
| `explorer_fetch_raster` | `fetch_raster` | `url`; `width?` | `image`, the picture's bytes in base64, and `mimeType`: the picture as served, or where `width` is named and the picture is wider, redrawn as JPEG that wide |
| `explorer_search_engines` | `search_engines` | nothing | `engines`, in the roster's order, each `name`, `category`, `koreanNative`, `state` (`unknown` where it has not been read) and `reason?`, `observedAt?`, `suspendedUntil?`, `elapsedMs?` and `resultCount?`; `cooldowns`, each a `reason` and the `restMs` an engine rests for it |

- **`explorer_web_fetch` reads by the plain road:** the direct client, then the real Chrome after a block
  (docs/web-access.md, 「Strategies」); a PDF comes back as its text. A site tool's own reading of a page it
  claims is a tool host's (docs/site-contract.md, 「A page a tool claims」), and the MCP server has none.
- **`explorer_link_preview` and `explorer_fetch_page` read the page directly,** checking the address and each
  redirect, within 10 seconds, at most 512 KiB of it, paced 250 ms apart per host.
- **`explorer_fetch_raster`** reads a JPEG, PNG, GIF, WebP, AVIF or TIFF picture of at most 5 MiB within
  10 seconds.

## The harness plugin

- **`plugin/skills/bmo-explorer/SKILL.md`** is the `bmo-explorer` skill: when a harness uses the server, how
  it starts it, what it does first and what each failure means.

## The tools a tool host places

Each tool is a folder of `tools/` with its own `tool.toml`, placed with the tool host and registered as this
repository's own (`testbed.toml`, its deploy steps). The host's call protocol is the tool host's, as its API
0.1.0-rev.2 states it (its `docs/api.md`, 「The call protocol」); every tool here speaks version 1 of it.

| Folder | Entry | Capability | What it does |
|---|---|---|---|
| `tools/web_search` | `web/web_search` | `web.read` | Asks the search engines directly and answers their hits as they came (docs/web-access.md) |
| `tools/web_fetch` | `web/web_fetch` | `web.read` | Reads one page, answered by the site tool that claims its address where the record holds one (docs/site-contract.md, 「A page a tool claims」) |
| `tools/explorer` | `explorer/fetch_raster`, `explorer/fetch_page`, `explorer/search_engines` | none: for the system alone | A picture, a page's head and the engines' states (below) |
| `tools/_shared/web` | the shared helper `web` | | One record's web reading, which `web_search`, `web_fetch` and every placed site tool ask on its socket |

- **`web_search` and `web_fetch` name the helper `web`** among their shared helpers, and so does every site
  tool a site-tool repository places; the host starts one helper a record and hands its socket to every call
  naming it.
- **`web_search` and `web_fetch` declare their line in the `web` listing** (the tool host's `docs/api.md`,
  「The manifest」, `[entry.listing]`), so a searcher's instructions, composed with that listing, name the two
  reading tools ahead of every site tool: `web_search` first and `web_fetch` second, each line headed with its
  name and saying what it answers, and a site tool's lines after them in the order of the date it was made.
- **No entry here declares a grant,** so a call of one is checked against the record's capabilities alone.

### What a placed tool reads of its record

Every call and the helper are handed the session record the tool host's `docs/api.md` states (「The session
record」): `holderName`, `namespaces`, `capabilities`, `grants`, `shown`, `variables`, `variablesFile`, `labels`,
`expiresAt` and `createdAt`. A record naming a field outside these does not read, and the call fails with why.
Of it the tools read:

- **`capabilities`,** which decide the form a session holds an entry in, whole or its bundle's.
- **`BMO_DATA_DIR`,** the data root: from the record's `variables`, or from the call's environment where they
  name none, which the host lays from its `--pass-env` and the record's `variablesFile`. A call of
  `explorer/fetch_raster`, `explorer/fetch_page` or `explorer/search_engines` that finds none is refused naming
  the variable, and the helper's start ends unclean naming it, which fails the call that started it.
- **`labels.research`,** the research settings a record's web reading is held to (docs/web-access.md,
  「Settings from the session record」), an object of six keys, each optional; any other key of it, and any
  other label, is the issuer's and left unread. One whose keys do not read as these types ends the helper's
  start unclean in the same way:

| Key | Type | Meaning |
|---|---|---|
| `closesAt` | epoch milliseconds | When research closes: no search or page starts after it, and each request before it finishes inside it |
| `strategy` | `direct-then-browser`, `browser-first` or `direct-only` | How a page is read; `direct-then-browser` where absent |
| `hostIntervalMs` | integer | The gap between two direct requests to one host |
| `engineIntervalMs` | integer | The gap between two requests to one engine |
| `pages` | integer, at least 1 | The record's page budget: how many pages its web tools read before every further page is refused |
| `branchPages` | integer, at least 1 | The budget of each branch the run opens, which a branch's own record carries as its `pages`; read and kept, and no fetch of this record's is held to it |

### The explorer entries

A caller asks the tool host's control socket, `POST /tool-calls` with a tool session's token whose record names
the namespace `explorer`, with a JSON body naming the entry by `namespace` and `name` and what it is handed under
`input` (the tool host's `docs/api.md`, 「Tool calls」).

- **`explorer/fetch_raster`,** handed `address` and `pacing`, fetches the raster picture behind an address. Its
  content is one image block: the bytes in base64 under `data`, and their type under `mimeType`.
- **`explorer/fetch_page`,** handed `address` and `pacing`, fetches the page directly, checking the address and
  each redirect, and reads at most 512 KiB of it. Its structured content carries `url`, the address after every
  redirect; `type`, the content type as the server sent it; and `html`, the page decoded as its charset says,
  where it is markup, null otherwise.
- **`explorer/search_engines`,** handed nothing. Its structured content carries:
  - `engines`, each engine in the roster's order: `name`, `category`, `korean_native`, `state` (`unknown` where
    it has not been read), and `reason?`, `observed_at?`, `suspended_until?`, `elapsed_ms?` and
    `result_count?`;
  - `cooldowns`, each a `reason` and the `ms` an engine rests for it.
- **Every address is checked before it is connected to,** and one that resolves inside this network is refused
  (docs/web-access.md, 「Address checks」). Requests are paced per host; `pacing` names whose requests are paced
  together.
- **A 200 answer carries the entry's tool result under `result`.** A result marked `isError` is a refusal, and
  its text is the sentence. The waits a caller holds to are 60 seconds for a picture, 10 for a page's head and 30 for the
  engines.

### The web helper's socket

- **One JSON object on one line each way,** on the socket the tool host hands each call naming the helper under
  `shared.web`. The requests, their kinds and their answers are docs/site-contract.md's (「The web helper's
  socket」): the core tools' requests (`search`, `fetch`, `picture`), a site tool's roads (`page`,
  `rendered`, `engine`, `document`, `head`) and the research guard round a site tool's call (`opening`,
  `ended`).
- **The helper is started by the tool host** with the kind `helper` and the socket it is to listen on, and ends
  with its record (the tool host's `docs/mcp-host.md`, 「Shared helpers」).

## The unit command

`bmo-explorer`, on the `PATH` a deployment links it into. It prints what it did on standard output and why it
refused on standard error, and ends 0 where it did what was asked, 1 where it refused, and 2 where its words do
not read.

| Verb | Words | Does |
|---|---|---|
| `mcp` | `mcp [--data-root <folder>]` | Serves the MCP server on standard input and output until the harness closes it |
| `openapi` | `openapi` | Prints the OpenAPI document, as `docs/openapi.json` holds it |

## What this repository calls

- **No service.** The placed tools speak the tool host's call protocol, version 1, and `web_fetch` asks the
  call's own call-back socket which entry the record holds claims an address and calls that entry through the
  same socket (docs/site-contract.md, 「A page a tool claims」); both are the host's side of a call it made.

## Missing in siblings

What another repository's own boundary must gain or settle for this one to stand on it, each for that
repository's worker:

- **bmo-markup:** its Rust crate holds no answer ceiling; its TypeScript package alone serves `answer-cap`. A tool
  answer here is held under the ceiling by this repository's own Rust twin, tested against its own copy of the
  cases, until the crate serves the ceiling; bmo-markup's `docs/copies.md` does not yet list that copy among the
  copies other repositories hold.
- **bmo-artifact:** a page's import is its own (`POST /imports`), drawing the page and capturing its figures;
  this repository serves no import, and its client is what such an import reads a page through.
- **The issuer of a research session,** whoever posts its record to the tool host's `POST /tool-sessions`: names
  the data root as `BMO_DATA_DIR` among the record's `variables`, or has the host pass it on, and puts the
  research settings under `labels.research` in the shape 「What a placed tool reads of its record」 states.
- **The tool host's deployment:** the `web/research_branch`, `bmo/browse` and `bmo/remote_browser` tools this
  repository no longer places stand registered in a host they were put into until they are taken out of it.
