# Browse runs

A browse is a research run: started once, ended once, on the `browse` research profile bmo-artifact keeps.
What a search can answer from the search engines is a search, which bmo-artifact runs; what has to be seen
is a browse, which this repository runs.

## The browse tool

- **What a search cannot answer because it has to be seen** — a page to open, a members-only site
  whose holder offers their own login — runs through the `browse` tool, which takes what to do and
  the holder's login as two arguments, `task` and `secrets` (`bmo browse` is the same launch from a
  shell and from the console's launch box, the task on its standard input).
- **The launcher sends the room nothing at all;** the calling session announces in its own words
  that it is going to look. The launcher opens the run's row in bmo-agency's run ledger under the agent
  `browse`, has bmo-agency launch `bmo runner browse` in the agent's host, and returns at once
  (docs/api.md, 「The unit command」).
- **The runner is a run apart** (docs/api.md, 「bmo-agency」) holding only the chrome-devtools browser
  tools and this repository's web tools, no persona. bmo-agency composes its program and its
  instructions for the `browse` line; the task, the time it has and the knowledge its words call for
  are its prompt. Every line its record gains is fed to its run log as it is written.
- **It opens the pages itself and hands what it saw back to the agent that asked** as a quiet
  instruction through bmo-receiver; the agent tells the room in its own voice, and until then neither
  waits nor answers from memory. A `browse` called in sync takes that ending inside the call instead.
- **The task reaches the runner through a file** in the temporary directory, created owner-only and
  deleted the moment the runner has read it, never through a command line, because the task may carry
  credentials and arguments are visible in the process list.
- **Credentials are never stored, never logged, and never repeated back into the chat** — the run
  is told to use them only to sign in to their holder's own service and to keep them out of its
  answer — and a request to get into an account the asker does not hold is refused under the agents'
  consent rule.
- **Failures are reported rather than swallowed:** a login that does not work, a page that cannot be
  reached, a site that blocks automation, or a run that dies outright all reach the agent the same way,
  and the agent says so to the room.
- **The browse run is shown no tool that could put work in the background,** so nothing it hands back
  can be a note about work still going; what it needs is a tail that always speaks, and it has one —
  a run whose unit is stopped out from under it reaches the agent as a failure like any other, and its
  ceiling is 「The browse clock」.
- **How it ends:** `done`, `failed`, `stopped`, or `live-tasks` where it ended with its own work still
  running; a launch that never came up is closed as `error`. The runner closes the run's row with it.

## The profile and the branches

- **The profile is read at the launch** from bmo-artifact's research settings, the `browse` key: its total
  time, its wrap-up time, its branch role and count, its wait limit, its fetch strategy and its host and
  engine intervals. A run keeps the profile it started with.
- **A run whose profile opens branches reserves their seats** in bmo-agency's co-worker pool before its
  clock starts, under the owner `browse-run:<run>`, and releases them once however it ends.
- **A branch is opened by the run's `research_branch` tool,** which the web helper answers by running the
  branch command with `--source browse` (docs/api.md, 「bmo-artifact」). A branch is refused past the run's
  allowance and after its research close.

## The browse clock

- **Start:** a browse run's clock starts when the model process is launched. Two times are set:
  - **Research deadline** = start + total time − wrap-up time.
  - **Ceiling** = start + total time.
- **After the research deadline:**
  - The web tools refuse new searches and new pages, and the refusal says to write the result from
    what is at hand.
  - A request started before the deadline does not run past it either: searches, pages and host-gap
    waits take the deadline as their own deadline.
- **At the ceiling** the agent is asked whether to keep the run or cut it, and the run goes on until it
  answers (bmo-agency's `docs/agent-control.md`, 「A clock asks before it cuts」). Research has closed by
  then, so a keep buys the result's writing. A cut — the agent's, or for want of an answer — ends the run
  and salvages the last structured result.
- **The wrap-up time** secures time for that salvaged result to be written; it does not replace the
  ceiling.

### When the search engine is blocked

- **The same for every run.** When `SEARCH UNAVAILABLE` comes twice in a row within one run, that
  process's web search stops asking and returns the same answer.
- **An upstream rest:** when the engine is resting for an upstream reason, the store's record returns
  that fact without a request.
- **The run still writes a result** from the pages already opened and the branches returned, and states
  that search could not run and what remains unconfirmed.

## Browser strategy

### Search

- **Direct request first by default.** It hands over to real Chrome only on a block or an empty
  JavaScript shell.
- **Most pages read fine with a direct request,** and Chrome costs a few seconds and memory to start.
- **`browser-first`,** which reads with Chrome from the start, is the value the owner picks in the
  profile when blocks are frequent.

### Browse

- **A browser run by nature.**
- **With `visible`:**
  - The runner starts a virtual X server (Xvfb) of that run's own.
  - The chrome-devtools server starts standard Chrome with an isolated temporary profile on that screen.
- **Nothing on the person's desktop:** no window and no authentication dialog appears in the person's
  desktop session.
  - Chrome starts with that virtual screen, a disabled session bus and `--password-store=basic`, instead
    of the user session's display, D-Bus and keyring.
  - Xvfb ends with the runner, whether it finishes or dies.
- **No Xvfb:** if the machine has no Xvfb or it does not start, the run starts headless and says so in
  the log.
- **No CAPTCHA solving** either way. If a human-check screen remains, the run reports it was blocked.
