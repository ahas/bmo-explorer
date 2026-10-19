# The boundary

bmo-explorer is the web reading of every agent session: the web tools a tool host serves, the shared web helper
that holds one investigation's reading, the browse and remote-browser runs, and the unit command that starts them.
It runs no service of its own. The tool host starts its tools and its helper, bmo-agency starts its runs, and
`bmo` hands its commands to its unit command. This document is the whole of what it serves and what it calls;
another repository reaches it through nothing else, and it reaches another repository through nothing else.

## Versioning

- **The contract carries the repository's one version, in semver,** the version in `VERSION` and the Cargo
  manifest.
- **The major moves on a change a caller reads:** a tool entry's name, input or answer, the helper socket's
  requests and answers, a command's words, an exit code, or a setting. The minor moves on an addition: a tool
  entry, a request kind on the socket, a command, a setting. The patch moves on anything else.

## Settings and data

- **The data root** is the folder `BMO_DATA_DIR` names, the one the tool host hands every call. This repository
  keeps its own folder in it, `explorer/`, and reads no other (docs/store.md).
- **The settings** stand in `explorer/settings.toml` under the data root:
  - `[units]`: the base address and the bearer token of each service it calls, `agency` and `agency_token`,
    `receiver` and `receiver_token`, `artifact` and `artifact_token`, `gate` and `gate_token`, `node` and
    `node_token`. A call whose
    address is unset is refused with an English sentence naming the setting; a token unset goes with none, and
    the service refuses it as it refuses any call without its token.
  - `[remote_browser]`: the debugging port and the profile a remote browser lands on where its steps name
    neither (docs/remote-browser.md, "The steps").
- **What the web layer is built with stands beside the code that reads it,** not in the settings: the engine
  roster with each engine's category and the rests after a refusal (docs/web-access.md, "Rosters"), and the
  arguments every Chrome it starts carries with the session variables none of them is handed (docs/web-access.md,
  "How Chrome is started").
- **Every answer this repository gives is English,** a sentence for a person or a status code for a program. A
  room's words are its agent's: a run hands the agent facts, and the agent says them in the room's language.

## What this repository serves

### The tools

Each tool is a folder of `tools/` with its own `tool.toml`, placed with the tool host and registered as this
repository's own (`testbed.toml`, its deploy steps). The host's call protocol is the tool host's
(its `docs/mcp-host.md`, "A call"); every tool here speaks version 1 of it.

| Folder | Entry | Capability | Grant | What it does |
|---|---|---|---|---|
| `tools/web_search` | `web/web_search` | `web.read` | | Asks the search engines directly and answers their hits as they came (docs/web-access.md) |
| `tools/web_fetch` | `web/web_fetch` | `web.read` | | Reads one page, answered by the site tool that claims its address where the record holds one (docs/site-contract.md, "A page a tool claims") |
| `tools/research_branch` | `web/research_branch` | `web.branches` | | Opens one research branch for an angle and answers with its notes |
| `tools/browse` | `bmo/browse` | `room.tools` | `search` | Launches a browse run (docs/browse-runs.md) |
| `tools/remote_browser` | `bmo/remote_browser` | `room.tools` | `launch` | Launches a remote-browser run (docs/remote-browser.md) |
| `tools/explorer` | `explorer/import_page`, `explorer/fetch_raster`, `explorer/fetch_page`, `explorer/search_engines` | none: for the system alone | | A page, a picture, a page's head and the engines' states, for a service's own runs (below) |
| `tools/_shared/web` | the shared helper `web` | | | One record's web reading, which every tool above but the launching ones asks on its socket |

- **`web_search`, `web_fetch` and `research_branch` name the helper `web`** among their shared helpers, and so
  does every site tool a site-tool repository places; the host starts one helper a record and hands its socket
  to every call naming it.
- **The launching tools** take the inputs their schemas state and hand the run to the unit command (below),
  `bmo-explorer` as the `PATH` they run under finds it, under the turn's variables (`BMO_TURN_CHAT`,
  `BMO_TURN_AGENT`, `BMO_TURN_VIA` and the run's own): a call in `sync` waits for the run's ending and answers
  with it, however long the run takes; a call in `async` answers once the run is launched, and the ending
  reaches the agent's next turn as a quiet instruction.
  - The room is the one the call names, or the turn's own; a call naming none outside a room's turn is refused.
    A run is set off by the turn's room, `chat:<chatId>`, and runs in the mode the call asks, or its tool's own,
    in the turn's own room, and async in another, whose ending is that room's session's to answer.
  - `browse` goes async where the call names no mode, and `remote_browser` sync.
  - `browse` takes `task` (required), what to do in one self-contained text; `secrets`, the holder's login, kept
    out of the task; `mode` (`sync` or `async`); `chat` and `message`, the room and the message it answers.
  - `remote_browser` takes `node` (required), the node by its name in the node list; `plan` (required), the
    steps as a steps file holds them; `secrets`, `NAME=value` lines; `mode`, `chat` and `message` as `browse`
    takes them.
  - An async launch answers one English line for the agent naming the run's unit and the room it reports to,
    never a sentence for the room; a sync launch answers the run's ending.

### The explorer entries

What a service's own run reads of the web goes through these, so every outward request keeps this repository's
checks. A caller asks the tool host, `POST /calls` at the tool host's address, with a JSON body naming the entry
under `tool` and what it is handed under `arguments`.

- **`explorer/import_page`,** handed `address`, draws the page in a browser and reads it whole. Its structured
  content carries:
  - `page`: `url`, the address after every redirect; `title`; `keywords`, a list; `body`, the page's text;
    `references`, each with its `text` and `url`; and `figures`, each figure the page draws with its number
    `n`, its `caption` and, where the page drew it as one, its `svg`;
  - `captured`: each figure captured as a picture, by its number `n`, with the PNG's bytes in base64 under
    `png`. A figure that would not capture is not among them.
- **`explorer/fetch_raster`,** handed `address` and `pacing`, fetches the raster picture behind an address. Its
  content is one image block: the bytes in base64 under `data`, and their type under `mimeType`.
- **`explorer/fetch_page`,** handed `address` and `pacing`, fetches the page directly, checking the address and
  each redirect, and reads at most 512 KiB of it. Its structured content carries `url`, the address after every
  redirect; `type`, the content type as the server sent it; and `html`, the page decoded as its charset says,
  where it is markup, null otherwise.
- **`explorer/search_engines`,** handed nothing. Its structured content carries:
  - `engines`, each engine a research run may query, in this repository's order: `name`, `category`,
    `korean_native`, `state` (`unknown` where it has not been read), and `reason?`, `observed_at?`,
    `suspended_until?`, `elapsed_ms?` and `result_count?`;
  - `cooldowns`, each a `reason` and the `ms` an engine rests for it.
- **Every address is checked before it is connected to,** and one that resolves inside this network is refused
  (docs/web-access.md, "Address checks"). Requests are paced per host; `pacing` names whose requests are paced
  together, one name for each run's reading.
- **A `2xx` answer is the entry's tool result.** A result marked `isError` is a refusal, and its text is the
  sentence. The waits a caller holds to are 150 seconds for a page, 60 for a picture, 10 for a page's head and
  30 for the engines.

### The web helper's socket

- **One JSON object on one line each way,** on the socket the tool host hands each call naming the helper under
  `shared.web`. The requests, their kinds and their answers are docs/site-contract.md's ("The web helper's
  socket"): the core tools' requests (`search`, `fetch`, `picture`, `branch`), a site tool's roads (`page`,
  `rendered`, `engine`, `document`, `head`) and the research guard round a site tool's call (`opening`,
  `ended`).
- **The helper is started by the tool host** with the kind `helper` and the socket it is to listen on, and ends
  with its record (the tool host's `docs/mcp-host.md`, "Shared helpers").

### The unit command

`bmo-explorer`, which `bmo` finds as a unit command (bmo-cli's `docs/boundary.md`, "Unit commands"). Asked
`nouns`, it prints the nouns below, one a line. It prints what it did on standard output and why it refused on
standard error, and ends 0 where it did what was asked, 1 where it refused, and 2 where its words do not read.

| Noun | Also run as | Words |
|---|---|---|
| `browse` | | `browse --chat <chatId> [--by <who>] [--via <road>] [--mode <sync\|async>]`, the task on standard input, with the login its holder offered after it on a line of its own that names it as such |
| `runner browse` | `browse-runner` | `runner browse <chatId> <taskFile> [runId] [unit] [profile] [--kept-on <registration>] [--via <road>] [--ending <file>]` |
| `remote-browser` | | `remote-browser --chat <chatId> --by chat:<chatId> [--via <road>] [--mode <sync\|async>] --node <name> --steps <file>`, the secrets the steps name on standard input as `NAME=value` lines |
| `runner remote-browser` | `remote-browser-runner` | `runner remote-browser <chatId> <taskFile> [runId] [unit] [--via <road>] [--ending <file>]` |
| `tool unread-pages` | `unread-pages` | `tool unread-pages`, the pages no road read, a site at a time (docs/site-contract.md, "A page the searcher could not read") |

- **`browse` and `remote-browser` launch:** each refuses before anything is opened where its words or its input
  do not hold, opens the run's row in bmo-agency's run ledger, writes the task for the runner to a file only
  its owner reads, and has bmo-agency launch the runner, under the title `browse for chat <chatId>` or
  `remote browser for chat <chatId> on <node label>`. With
  `--mode async`, or none, it answers once the runner is launched; with `--mode sync` it hands the runner a
  file only its owner reads, `--ending`, waits until bmo-agency's ledger holds the run closed, and prints the
  ending the runner wrote there, however long the run takes. A launch whose runner did not come up takes its
  files away and has its row closed as `error`.
- **The runners are what bmo-agency launches,** never a person. Each reads its task file and deletes it, carries
  the run, closes the run's row with how it ended, and hands the ending to the agent that asked as a quiet
  instruction, or writes it to the `--ending` file a sync launch waits on. A stop signal (`SIGTERM` or
  `SIGINT`) stops the run, and the stop is handed back as its ending.
- **What a runner is handed by its launcher,** in the environment bmo-agency starts it with: `BMO_DATA_DIR`,
  the data root its settings stand under, and `BMO_TURN_AGENT`, the agent whose turn asked, which its ending
  goes back to.
- **The outcomes a run is closed with:** `done`, `failed`, `stopped`, `cut-short` for a browse its clock ended,
  `live-tasks` for a browse that ended with its own work still running, and `error` for a run that never came
  up.

## What this repository calls

### bmo-agency

- **The run ledger.** `POST /runs` opens a run's row (`agent`, `chat_id?`, `unit`, `depth`, `cap_ms?`,
  `started_at`, `started_by?`), answering `201` with its `id`; `POST /runs/{id}/launch` launches the runner
  (`title`, `agent`, `command`: `program`, `args`, `cwd`, `environment`); `POST /runs/{id}/close` closes it
  (`finished_at`, `outcome`, `context_tokens?`), a run already closed keeping its first closing. A browse run is
  opened under the agent `browse` and a remote browser under the agent `remote-browser`, with their times in
  epoch seconds as the ledger keeps them.
- **A run's state.** `GET /runs/{id}` reads a run, for a sync launch waiting on its close; `POST /runs/alive`
  (`names`) answers which units still run, so a launch reclaims only the pool places of a run closed and gone.
- **The road a browse goes out on.** `GET /providers` (the first enabled one carrying the `browse` line),
  `GET /tiers/assignments` and `GET /tiers` (the `browse` key's tier), `POST /routes/resolve` (`work_key`,
  `provider`, `tier`: the model and effort) and `POST /routes/check-effort`; or, for a launch made under a
  registration that keeps its research, `GET /registrations/by-home?home=<home>`, the home read from the
  launching process's `CLAUDE_CONFIG_DIR`.
- **The run apart.** A browse run's model session is a run apart: `POST /apart` starts it (`agent` and `line`
  `browse`, `title`, `work_key` `browse`, `road`, `instructions` named `browse`, `environment`, `prompt`,
  `log?`, and `bound_ms`, the profile's total time), `GET /apart/{id}` reads it every half second until it is no
  longer `running`, and `POST /apart/{id}/stop` stops it the moment the run is stopped. The environment carries
  what the web tools are told (`BMO_RESEARCH_*` and `BMO_WEB_*`, docs/web-access.md, "Settings from the research
  run") and, where the run has a display of its own, `BMO_BROWSE_DISPLAY`, `BMO_BROWSE_XAUTHORITY` and
  `BMO_BROWSE_RUNTIME_DIR`, which the chrome-devtools server is shown on; headless where none is set.
- **The run's clock** is bmo-agency's to keep: past `bound_ms` it asks before it cuts (bmo-agency's
  `docs/agent-control.md`, "A clock asks before it cuts"), and a run apart it stopped past the ceiling ends the
  browse as `cut-short`.
- **A run counted.** `POST /agents/browse/runs`, as the run starts and again as it answers.
- **The branches' seats.** A browse run whose profile opens branches reserves their seats in the co-worker pool
  before its clock starts, `POST /pool/reservations` (`reservation_key`, `owner`, `source`, `role`, `provider`,
  `model`, `effort?`, `tier`, `wanted`), and releases them however it ends,
  `POST /pool/reservations/{token}/release` (`reason`), with every request of its own,
  `POST /pool/requests/{token}/finish` (`outcome`, `cancel`). Its owner is `browse-run:<run>`. Each branch takes
  its seat through the branch command (bmo-artifact, below). A launch first gives back what dead research runs
  left: `GET /pool/reservations` and `GET /pool/requests`, each released or finished where its run is closed and
  its unit no longer runs.
- **A note in the run's log.** `POST /log/notes` (`agent`, `run`, `chat_id?`, `text`).
- **A call that does not reach bmo-agency,** or an answer out of its shape, fails the run with the sentence why.

### bmo-receiver

- **A run's ending** is queued as a quiet instruction for the agent that asked, in the room it asked in:
  `POST /instructions` with an instruction as bmo-receiver's `docs/boundary-api.md` states it ("For bmo-job"):
  `agent`, the agent whose turn asked; `chat`; `said` holding the ending as an order; `sort`, `browse` or
  `remote_browser`; `via`,
  the road the question came in on; and `run` naming the run. It answers `201` with the instruction's `id`.

### bmo-gate

- **The knowledge a browse is handed.** `POST /knowledge/work` (`chat_id`, `lines`: the task as a quiet
  instruction, without the login offered apart from it and with every credential shape masked) answers
  `pieces`, each `path` and `text`, which stand after the run's request in the shape a turn's reference stands
  in. A pick that fails costs the run its reference and nothing else.

### bmo-artifact

- **The browse profile.** `GET /research-settings`, of which this repository reads the `browse` key's profile:
  its total time, its wrap-up time, its branch role and count, its wait limit, its strategy and its host and
  engine intervals.
- **A branch of a browse run** is run by bmo-artifact's branch command, which holds the branch's place in the
  co-worker pool: `bmo research-branch --owner <owner> --source browse --role <role> --chat <chatId> --index <n>
  [--closes-at <ms>] [--strategy <strategy>] [--host-interval-ms <ms>] [--engine-interval-ms <ms>]`, the angle
  on standard input. Its answer is one block on standard output, handed to the run as it stands. The web helper
  runs it for a `branch` request (docs/site-contract.md).

### bmo-node

- **A remote browser's node.** `GET /nodes` answers the list, of which the launch reads the node's `label` and
  `kind`; `GET /nodes/{name}/target` answers its connection: for a remote node, `hubUrl`, `tokenFile` and `agent`.
  A node not in the list, or one that is not a remote node, refuses the launch, and a runner whose node has left
  the list fails, telling the agent so.
- **The remote hub,** an outside program the node service lists, is asked over its HTTP control address with the
  token its token file holds: `POST /exec?agent=<agent>` runs one command on the node's machine, and an exec
  still running is cancelled on the same address (docs/remote-browser.md, "On the node").

### The tool host

- **The call protocol** the tools speak, version 1 (its `docs/mcp-host.md`, "The call protocol").
- **A claim.** `web_fetch` asks the call's call-back socket which entry the record holds claims an address, and
  calls that entry through the same socket (docs/site-contract.md, "A page a tool claims").
- **The site catalogue.** `tool unread-pages` reads the `web` namespace's facts on the host's control socket,
  `GET /facts/web`, to mark a site with the tool that reads it.

## Missing in siblings

What another repository's own boundary must gain or settle for this one to stand on it, each for that
repository's worker:

- **bmo-artifact and bmo-job:** each documents the research-branch command as its own. bmo-artifact's
  `docs/api.md` states `bmo search branch`, also run as `research-branch`, among its commands, and its inventory
  carries the code that runs it; bmo-job's `docs/cli.md` lists `research branch` with the same words among its
  nouns. `bmo` refuses a noun two unit commands declare, so one of the two must give it up. This repository
  calls `bmo research-branch` and reads nothing of which unit answers it.
- **bmo-cli:** `tool unread-pages` (`unread-pages`) is this repository's noun, read out of its own store; bmo-cli's
  `docs/boundary.md` lists it as not served yet.
- **bmo-agency:** its `docs/api.md` states no run apart, though bmo-artifact's search runner and this
  repository's browse runner both start one. It must serve `POST /apart`, `GET /apart/{id}` and
  `POST /apart/{id}/stop` in the shapes 「bmo-agency」 above states, and for the `browse` line: compose the
  instructions it keeps for it (`prompts/agents/browse/task.md`) with the chrome-devtools server, shown on the
  display the run's environment names or headless, beside this repository's web tools; and hold the run to
  `bound_ms`, asking before it cuts. The browse request, `prompts/agents/browse/request.md` in its tree, is this
  repository's own prompt and stands here; its copy there is read by nothing.
