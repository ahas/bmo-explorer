# Remote browser

A browser on one of the owner's own machines, driven from here through the machine's remote agent: a login, a
menu clicked, a form filled, a value read, a screenshot taken. The work it serves is a form the owner fills in
on a site of their own, such as a leave request, done without the owner at the keyboard.

## Why it runs outside the chat turn

- Every command sent through a remote agent is a round trip over the hub, and a round trip is seconds
  before anything on the machine has run. A page driven one command per step spends a minute on a
  handful of steps, and the room waits on the chat turn the whole while, so a login followed by
  two sites holds it for minutes.
- So the whole list of steps is handed over at once and run as a unit of its own, like a search or
  a browse. The run drives the node and hands what it saw back as a quiet instruction to the agent
  whose turn launched it, where that agent still answers the room, and to the room's voice otherwise.
- The chat turn either waits for that instruction inside its call, however long it takes, or launches
  the run and ends: `sync`, the tool's default since a list of steps is mostly done in well under a
  minute, or `async` (docs/api.md, 「The tools」).

## The launch

- The chat agent launches it with the tool `remote_browser`, which takes the node, the plan as the
  JSON a steps file holds and the secrets as `NAME=value` lines, and fills in the chat it answers to,
  who launched it and the road from its turn. The command, `bmo remote-browser`, takes the chat, who
  launched it, the road, the node by its name in the node list and the path of a steps file, with the
  secrets on standard input, and launches as the tool does (docs/api.md, 「The unit command」).
- It refuses before any row or run exists: a node that is not in the list, a node that is not a
  remote node, steps that do not parse, and a secret the steps name that it was not given. The node is read
  from bmo-node (docs/api.md, 「bmo-node」).
- It opens a row in bmo-agency's run ledger for the agent `remote-browser`, writes the task for the
  runner to a file only its owner can read, and has bmo-agency launch the runner, `bmo runner
  remote-browser`. A launch that never came up closes the row and takes the file with it.

## The steps

- A steps file is JSON: the list of steps alone, or an object carrying the list under `steps`
  beside `port`, the browser's debugging port, and `profile`, the name of the browser profile the
  run uses. Both have defaults in the `[remote_browser]` settings, `port` and `profile` — 9222 and
  `remote-browser` where a deployment names neither — so a run keeps landing on the same browser, with the
  logins it already holds.
- Each step is an object with its kind under `do`, and it may carry only the fields its kind takes;
  anything else refuses the whole file, naming the step.
  - `page` — `match` makes the page whose address or title holds those words the one the next steps
    act on, waiting for it to appear; `new` opens a new page at an address and waits for it to load.
  - `goto` — the current page goes to an address and the step waits for that page's own load.
  - `click` — a real mouse press at the element's centre: named by `selector`, or by `text`, the
    words it shows, where the smallest element showing them is the one meant; or at a point, `x`
    and `y`. The element is scrolled into view at once rather than smoothly, and it is pressed only
    once its centre is its own — nothing else lies over it — so a click never lands where the
    element was a moment before. The point is the one on the screen whichever frame holds it.
  - `type` — focuses the field `selector` names, empties it unless `clear` is false, and inserts
    `text` or the value of the secret `secret` names, the way a paste arrives. With no selector it
    types into whatever holds the focus.
  - `key` — one key pressed and let go, by its name (Enter, Tab, Escape, the arrows).
  - `wait` — for `ms`, for something visible matching `selector`, for `text` in the page, for an
    address holding `url`; with none of them, for the page to finish loading.
  - `read` — a JavaScript expression evaluated in the page, awaited, and its value kept, under `as`
    where it is named.
  - `shot` — a screenshot of the page, JPEG unless `format` says png.
  - `close` closes the current page; `quit` shuts the browser down and waits until its port stops
    answering.
- `frame`, on the steps that act inside the page, names a frame by its name or a piece of its
  address; the step then runs in that frame's own document. Legacy sites that open each screen in
  an iframe are driven this way.
- A step that waits gives up after `timeoutMs`, thirty seconds unless it says, two minutes at most.

## Secrets

- A password never travels as an argument, which the process list shows, and never sits in a steps
  file. A step names a secret by a name of capitals, digits and underscores, and the launcher reads
  the values off standard input as `NAME=value` lines — so an environment file holding them can be
  handed in whole — keeping only the names the steps use. Standard input is read only when the
  steps name a secret.
- The values cross to the node in the environment of the command and nowhere else: the hub's traffic
  log records a command's text and never its environment, and the node's helper types a value into
  the page without printing it.
- Everything the run writes or hands back — its log, the step details, a value a page echoed, a
  dialog's message — has every secret's value replaced by its name before it goes anywhere.

## On the node

- The runner reaches the node through its remote hub: the hub's address, its token file and the node's
  agent are the node's target as bmo-node answers it, and each exec is `POST /exec?agent=<agent>` on the
  hub with the token the token file holds.
- The node runs one helper for the whole list. It is kept on the node under a name carrying its
  content's hash, in the agent user's own data directory, and a run whose node lacks that file sends
  it once, in the command's environment, before running it; a changed helper is a new file, so a node
  is never left running an old one. It runs under bun, or node 22 and later.
- The helper is TypeScript in this repository, built into the `bmo-explorer` program, and what is
  shipped is its code with the types stripped by bun's own transpiler, which the runner asks on the
  machine it runs on (bun on `PATH`, or in the home's own `.bun/bin`); the hash is taken of the stripped
  code.
- The helper finds the browser on its debugging port and, where nothing answers there, starts it on
  its own profile: on a WSL node, Windows Edge through PowerShell, with the profile under the
  Windows user's local application data; on a Linux node, the first Edge or Chromium browser
  installed, headless where there is no display. A browser already running on that profile without
  the port keeps the port shut, and the run fails saying so.
- A dialog a page raises — an alert that would otherwise hold every later step — is accepted at
  once and written down with the step that raised it.
- The helper prints one JSON object a line: the browser it attached to or started, each step with
  what it did and the page it ended on, each screenshot's bytes, each dialog, and the end.
- A step that fails ends the run there, with its reason and a screenshot of what the screen showed.

## Longer than one command

- The agent stops any command that runs past five minutes. The helper therefore pauses itself
  before a step once its exec has run for a little over three minutes, or once it has printed five
  megabytes, and says where it stopped and on which page; the runner sends the rest in the next
  exec, on the same page. Each exec takes at least one step, so a run always moves on.
- A whole run is held to twenty minutes.

## What comes back

- The runner keeps each screenshot under this repository's folder of the data root, in
  `explorer/remote-browser/`, in a folder named after the run's unit, readable by its owner alone.
- A sync launch is handed the ending instead, in a file only its owner reads, which the launch prints once the
  run's row is closed (docs/api.md, 「The unit command」).
- It hands the agent that launched it one quiet instruction through bmo-receiver: the node, whether the
  browser was started or already running, every step with what it did and the address it ended on,
  every value read, the failure and its reason where there was one, the dialogs, the screenshot paths,
  and the page it ended on. The session tells the room in its own voice and opens a screenshot where
  the screen matters.
- A run stopped from outside cancels its command on the node, closes its row as stopped and tells
  the session so.
