# Web search and page collection

- **What the web tools return:** the search engines' answers and the page's original text, not a
  summary a model read and wrote on the agent's behalf.
- **What this document settles:** which road those requests take, when they move to a real browser,
  how a block is recognised, diagnosed and answered, and how private data is handled.

## The web session inside one investigation

### One session, one investigation

- **The one standing store: each search engine's own jar.** A search engine keeps one cookie jar
  across investigations, in this repository's store (docs/store.md).
  - It holds only what that engine's own result pages and its warm-up page set, on the engine's
    own hosts. A search request carries a query and nothing of whoever asked it.
  - A cookie is kept for its own lifetime and never longer than 7 days.
  - The jar is emptied whenever the engine changes identity (「Identities and headers」).
- **Scope of everything else:** one investigation, which is one of two things.
  - A session record of the tool host: the web helper the host starts for a record holds its one
    reading, for the core tools and every site tool alike (the tool host's `docs/mcp-host.md`, 「What a
    record's tools keep」).
  - One run of the MCP server (docs/api.md, 「The MCP server」): its client holds one reading for as
    long as the server runs.
- **Two per-investigation cookie stores:** inside it, the direct HTTP client that reads pages and
  the real browser each keep using their own cookie store, and both are discarded when the
  investigation ends.
  - The real browser on the owner's profile (「Which browser」) starts from a copy of his profile, so its
    store opens with his cookies and site sign-ins. What it gathers stays in that copy and is discarded
    with it; nothing is ever written back into his profile.
- **Nothing moves between the three:**
  - An engine jar is never used for a page fetch and never handed to the browser, and a page's or
    the browser's cookies never enter an engine jar.
  - Cookies and credentials are never moved between the page client and the browser.
  - Nothing of a page fetch is shared with another investigation.
  - So challenge cookies a site hands out never leak into another investigation.

### Identities and headers

- **A small fixed set of identities,** each one real stable Chrome of the installed major version:
  Linux desktop, Windows desktop, macOS desktop and an Android phone.
  - An identity sets `User-Agent`, `sec-ch-ua`, `sec-ch-ua-mobile` and `sec-ch-ua-platform`
    together, so the four never disagree.
  - Sent with them: `Accept`, `Accept-Language` (`ko-KR`, `ko`, `en-US`, `en`), `Accept-Encoding`,
    `Sec-Fetch-*`, and the upgrade and priority headers.
- **Who uses which:**
  - Page fetches use the Linux desktop identity.
  - A search engine keeps one identity, written in its recipe, and moves to the next only when a
    block is diagnosed as the client's shape. Changing identity per request would contradict the
    cookies the engine carries.
- **No product or crawler tag** is added after the product name.
- **Not a fingerprint patch:** an identity is the HTTP presentation an ordinary browser sends. Where a
  site checks more than the request shows, the answer is the real browser road, which presents itself as
  that browser does (「Which browser」).

### Redirects and responses

- **Redirects:** the client follows them one step at a time, a limited number of times.
  - Each step's destination is checked again, and cookie domain, path, secure and expiry are applied.
  - When the origin changes, sensitive headers are not passed on: `Authorization`,
    `Proxy-Authorization`, the caller's `Cookie`, `Origin`, the full `Referer`.
- **Response size:** a response is decompressed and read as a stream up to a byte limit, the rest is
  cancelled, and a cut response is marked as such in the tool result.
- **Charset:** response text prefers HTTP's `charset`, with the HTML meta declaration as a secondary
  source, so a non-UTF-8 Korean page is never taken for broken UTF-8.

### Address checks

- **Refused addresses:** loopback, private, link-local and metadata addresses are refused for URLs an
  agent opens at will.
  - The other special-purpose ranges are refused too: shared, documentation, benchmarking,
    multicast and reserved.
  - Each range is refused by its registered prefix alone, so a public block beside one — a hosting
    provider's addresses next to a protocol block — is read like any other site.
- **The trusted road:** test fixtures use a separate trusted road only.
- **Hostname resolution:**
  - It is waited for until the resolver answers, and a read its caller lets go of drops its lookup with it.
  - While it is slow, the real request is not sent ahead of it.
- **Connecting to the checked address:**
  - The connection is made to exactly the address that passed the check; the request does not resolve
    the hostname again.
  - The original name rides only in the `Host` header and the TLS server name, and the certificate is
    verified against that name.
  - Even with a gap wait between check and connect, a name changed meanwhile to point elsewhere never
    reaches the connection.

### Pacing

- **One queue per host:** every direct request, every redirect of the direct client, and every top-level
  navigation of the real browser first takes a per-host global FIFO interval.
- **One queue per engine:** every request to a search engine first takes that engine's interval — the
  record's engine gap, or the engine's recipe interval where that is longer.
- **Shared between processes:** both queues live in this repository's store, so two runs at once never
  double the rate a host or an engine sees.
- **A slot is waited for, however far off:** a request takes the next slot of its queue and starts when the
  slot comes. No request is turned away for the wait in front of it.
- **`429` and `Retry-After`:**
  - When a page answers with them, that host's global time is pushed back.
  - The read waits until that time and then asks the real browser, where its strategy lets it; the site's own span
    is followed, never cut short and never copied into a bound of this layer's.

### Settings from the session record

- **What a record may carry:** the fetch strategy, the host and engine gaps, a page budget and a branch
  allowance, under its issuer's label `research` (docs/api.md, 「What a placed tool reads of its record」). A
  record that names none of them is read `direct-then-browser`, at the default gaps, with no budget and no
  branch.
- **Nothing a record carries is a clock:** no search, page or branch is refused or cut short because time has
  passed. A read ends with its answer, its own failure, or its caller letting go of it (「A read its caller lets
  go of」).
- **Unavailable search:** two `SEARCH UNAVAILABLE` in a row in one record stop further asking.
- **The page budget is a hard ceiling:** a record that names `pages` reads that many pages and refuses every
  further one with one line, `BUDGET SPENT`, saying the budget is spent and to write from what is in hand. A page
  is a `web_fetch` of an address the record has not read yet — reading on in one already read, by its offset,
  is that same page — and a call of a site tool that reads no engine; a search, a picture and a page's head are
  no pages. A record that names no budget is never refused on it.
- **The branch allowance:** a record naming its run's `run` and the research unit it reaches by `branchesAt`
  asks that unit for one research branch each `web/research` call; how many branches the run may open and each
  branch's own budget are the research unit's to hold. A branch's own record, `branchOf`, opens none. The
  reading's own close drops every branch request still waiting (docs/api.md, 「The research branch」).
- **The MCP server takes no record:** it reads `direct-then-browser`, at the default gaps.

### A read its caller lets go of

- **A read waits for its answer.** No request of the web layer has a timeout of its own: a page, a search
  engine, a picture, a page's head, a name's lookup, the browser's load of a page and the PDF reader each end with
  their answer, a refused or broken connection, or the other end gone. The connection's TCP keep-alive is what
  tells a peer that has gone away.
- **Its caller lets go of it by going away.** A call of a placed tool asks the record's web helper on a
  connection of its own and holds it open until it is answered. When the call ends — it answered, the tool host
  ended it, or its caller went away — that connection closes, and the helper drops the read with everything
  under way for it: the request, its wait in a pacing queue or for a browser place, the browser's navigation, the PDF
  reader, a branch request. A harness that cancels a call of the MCP server drops that call's read the same way.
- **A site that takes the connection and never answers holds the read until its caller lets go of it.** Nothing
  here ends such a read on time.

## Direct collection and the real browser

### Strategies

- **`direct-then-browser` (default):** read with the direct client first.
- **`browser-first`:** read with the real browser from the start.
- **`direct-only`:** never use the browser, and return blocks as they are.
- **A fallback opens the address that was asked for,** not the one the direct client's redirects
  ended on, and the browser follows the redirects itself: a site that sent the direct client to its login
  page may serve a browser the page itself.
- **A failure the fallback meets says so:** the error carries the mark that the browser failed the page
  after the direct read had, so its caller knows both roads were tried.

### How a fetched page is classified

- **Blocked — a browser fallback candidate:**
  - `403`, `418`, `420` and `429`. `418` and `420` are refusals that some sites send in place of
    `403` or `429`; neither is an ordinary error.
    - A `403`, `418` or `420` whose page is a challenge, by any of the marks below or by challenge
      wording alone, is named a challenge rather than a bare status: a site that puts a CAPTCHA in
      front of one network answers it with a refusal status, and the failure is the challenge.
  - A challenge's own markup: `cf-chl-` and the challenge-platform path, wherever it stands.
  - A challenge widget — `g-recaptcha`, `h-captcha`, `cf-turnstile`, `arkose`, `funcaptcha`, Cap's
    `cap-widget` — on a page with under 1,500 characters of readable body text.
    - A widget beside a body of the page's own is that page's form guard, not a challenge. Sites put
      the same widget into their contact, sign-up and comment forms on pages they serve whole to any
      reader, and a Webflow site's contact form carries a reCAPTCHA on every page. A challenge
      stands in place of the body.
    - Each token is matched as a whole token only. A word that merely contains one is not a widget:
      `refresh-captcha` in a page's configuration URL is not `h-captcha`, and every Naver search
      page carries that URL.
    - A form's answer field is not a widget either. `g-recaptcha-response` and `h-captcha-response`
      are the names of the field a solved widget fills in, and Shopify's storefront forms script
      lists both in every page of every store it serves, whether or not a widget stands there.
    - The challenge-platform path is a widget only where it leads to a challenge. Cloudflare puts
      its passive scripts, served under that path's `scripts/` folder — the detection script under
      `scripts/jsd/`, the precursor under `scripts/precursor/` — into ordinary pages of the sites it
      guards, which it serves whole to any reader. A challenge page loads its own scripts from
      elsewhere under that path and carries `cf-chl-` markup.
  - A challenge title, or challenge wording together with a challenge form or a `503`.
  - A redirect chain that started on a public page and ended on a login page — a final path naming
    login or sign-in when the requested one did not. A shop that sends a client it does not trust
    to its login page shows the same page to a real browser without one.
- **Empty shell — a browser fallback candidate:** an HTML document that arrived with no readable
  body, only a script application shell.
- **An error — not a candidate:**
  - An ordinary `404`.
  - `451`: a refusal for legal reasons does not change in a browser.
  - A plain `503` with no challenge wording: a server error does not change in a browser.
  - A short page with real content is content.
- **No content:** a `204` or `205` answers the tool with a line saying the page answered with no
  content, never with a blank text read as success.

### A PDF

- **A PDF is answered with its text.** A body served as `application/pdf`, or opening with the PDF
  signature whatever type it was sent as, is read by poppler's `pdftotext` (the distribution's
  `poppler-utils` package) and sliced like any page; the content type still says `application/pdf`.
  - The text comes in reading order, each page headed by its number. A manufacturer's manual is
    often one poster-sized sheet, and a layout-preserving reading turns it into runs of spacing many
    times the length of its words.
  - A site tool reading a sheet of tables may ask for the layout reading instead, through its
    `document` road: each line of the sheet kept on one line with its columns spaced apart, so a
    table's rows stand as rows rather than one cell to a line. `web_fetch` itself never asks for it.
  - A PDF with no text layer — a scan, pages that are pictures — answers saying so, never with a
    blank text read as success.
  - A PDF the reader cannot open, a cut one among them, is an error naming the reader's complaint.
    `pdftotext` runs as its own process and is waited for until it ends; a read its caller lets go of ends
    the process with it.
- **Only the direct client reads a PDF.** A browser shows a PDF in its viewer, whose page is the
  viewer's and not the document's. Under `browser-first`, a page the browser reports as a PDF is read
  again by the direct client; a PDF served to the browser alone, after the direct client was refused, is
  a block.

### Which browser

- **The owner's own Firefox profile, where this machine has one.** The owner ordered on 2026-11-17 that the
  web tools' browser use the profile he uses himself, with his cookies and sign-ins, so a site sees an
  ordinary signed-in person rather than a fresh browser it takes for a bot. So where Firefox and its
  default profile stand on the machine, every browser read runs Firefox on a copy of that profile.
  - Firefox is `/snap/bin/firefox` and its profiles' folder `snap/firefox/common/.mozilla/firefox` under the
    home folder; `BMO_FIREFOX_PATH` and `BMO_FIREFOX_PROFILES` name others, and an empty
    `BMO_FIREFOX_PROFILES` names none. The profile is the one the folder's `profiles.ini` makes Firefox open
    by default.
- **Chrome after a refusal of Firefox:** a site that refuses the owner's Firefox outright — the page it
  answers with `403`, `418`, `420` or `429` — or a Firefox that cannot be started has the page read once
  more in Chrome on a temporary profile, after the host's own pacing interval, and Chrome's page is taken
  where that site did not refuse it too. Some sites' edges refuse a remote-controlled Firefox at its first
  request whatever its profile, and let the same machine's Chrome through; the owner's profile is for
  being let in, and the fallback keeps every page Chrome alone was let into before.
- **Chrome on a fresh temporary profile everywhere else:** a machine with no such Firefox profile, another
  node among them, reads with standard Chrome on a profile made for it and deleted after it.
- **The owner's profile is only ever read.** It is copied file by file into a new folder at each start, and
  the browser runs on the copy, never on the profile itself. Nothing is written into the profile, no lock of
  it is taken, and its own Firefox open or closed makes no difference:
  - Firefox opens a profile in one process at a time; a copy lets several browser places and the owner's own
    Firefox run side by side.
  - A browser ended without warning leaves nothing changed behind in his profile, since no setting of a
    start is ever written there.
  - Where his Firefox is writing a database as it is copied, the copy takes the database and its journal as
    they stand, which SQLite reads as the last whole state; a sign-in made that very moment may be missed.
- **What the copy leaves out:** saved passwords and their key store, the Firefox account Sync signs in
  with, the browsing history and bookmarks, form history, the saved session, the lock files, his own
  `user.js`, and crash and telemetry reports. Cookies, site storage, permissions, certificates, extensions
  and his preferences are copied.
- **The copy dies with the browser.** Cookies a site renews in that browser stay in the copy and are
  deleted with it, so a site that replaces a sign-in token on every use may sign the owner out of his own
  Firefox; nothing gathered in the copy ever reaches his profile.

### How the browser is started

- **Firefox on the owner's profile copy:** started non-headless with `--no-remote`, so it neither hands
  itself to a Firefox he opens nor takes one over, and with a loopback WebDriver BiDi endpoint on a port it
  chooses and announces.
  - The copy is made in a new folder named `.bmo-web-firefox-` and a random suffix inside the profiles'
    folder, readable by a snap's Firefox, made by this process alone and holding the starting process's id.
  - Its `user.js` is this start's own: the network path below, its cache and every download kept inside the
    copy so they are deleted with it and nothing lands in the owner's own download folder, no restore of a
    session, no default-browser check, and its window behaving as focused, as a person's window does.
  - An address that turns into a download fails the read, saying so: a browser shows no page for it.
  - A snap's Firefox is started in a user scope of the snap's own name made through the user's service
    manager, since its launcher would otherwise ask the session bus for one; the bus is cut only inside it.
  - Firefox's remote agent is told not to lay its recommended automation preferences over the profile's,
    so the copy behaves as the owner's own Firefox does.
  - **`navigator.webdriver` is hidden:** a script run in every page before the page's own scripts makes
    it answer false, as a person's browser answers. The owner's complaint was bot blocking, and a browser
    that announces automation defeats his order of 2026-11-17; the same holds for the browser that tops up
    his DeepSeek balance on the same profile.
- **Chrome on a temporary profile:** `/usr/bin/google-chrome` (`BMO_CHROME_PATH`) stable non-headless,
  with a fresh temporary `--user-data-dir` and a loopback DevTools endpoint. The arguments it adds for a
  detached session and the session variables it clears are built in beside the code that starts it. It
  adds no stealth option: nothing alters its `navigator.webdriver` or client hints.
- **Its own screen:** a dedicated virtual X server (Xvfb) the web layer starts for this browser.
  - Xvfb picks a free display number itself and reports it, so it never collides with another X server.
  - It accepts connections only with a random cookie kept inside the browser's profile folder, and does
    not listen on TCP.
- **Kept off the person's desktop:** the browser opens no window in the person's desktop session, and
  touches neither that session's D-Bus nor its keyring.
  - The user session's `DISPLAY`, `WAYLAND_DISPLAY`, `XAUTHORITY`, `XDG_RUNTIME_DIR` and
    `DBUS_SESSION_BUS_ADDRESS` are removed from the environment.
  - The runtime directory is put inside the profile folder, and the session bus is off or leads nowhere.
  - Chrome starts with `--password-store=basic` and the X11 platform; Firefox with Wayland turned off and
    no crash reporter.
  - So a browser never raises an authentication dialog on the person's screen while trying to open a
    password store.
- **What it never does:**
  - start a browser on the owner's profile itself, or write anything into it
  - open a Chrome profile of a person's
  - sign in to anything, or pretend to be an account the profile is not signed in to
  - solve a CAPTCHA, or pass one to a solving service

### The browser's network path

- **No resolving of its own:** the browser never resolves names to connect by itself.
- **Through a proxy:** every request goes through a proxy the web layer starts on loopback for that browser.
  - The proxy resolves the name.
  - It refuses private, loopback, link-local and metadata addresses.
  - It connects only to the address just checked.
- **https** passes the proxy as a tunnel, so TLS and certificate checks stay between the browser and the
  site.
- **Plain http** ends with one connection per request, so one connection never continues to another
  name.
- **Loopback is not exempt:** loopback addresses are also removed from the proxy bypass list and get the
  same check.
- **No WebRTC UDP,** which would not go through the proxy.
- **Firefox is told the same through its copy's `user.js`:** the proxy for http and https, no host it
  bypasses and loopback sent through it as well, no failing over to a direct connection, no DNS over HTTPS,
  no name prefetching or speculative connection, and WebRTC through the proxy only.
- **Every request a page makes is checked as its address is,** before the browser sends it.
  - A refused address fails the whole read, wherever on the page it stands: the page asked for a
    place this machine never goes.
  - A name with no address fails only its own request, as it does in any browser, unless it is the
    page's own document. An advertisement's host retired from the DNS leaves the page around it
    readable.

### Lifetime and cleanup

- **One context per session:** within the same session record the browser context is kept while it
  is used, preserving cookies.
- **A few browsers on the machine at once.** A browser takes one of the machine's browser places before
  it starts and gives it back as it ends; there are three, and `BMO_CHROME_PLACES` sets another number. A
  place is a lock in the temporary directory, let go with the process holding it however that process
  ends. A start that finds every place taken waits for one to be given back, however long that takes, and
  is never turned away for having waited; a place whose lock file cannot be opened at all fails the start
  as unavailable.
- **A start waits for what it started.** Its private display is waited for until the display names its
  number or ends, Chrome until its DevTools endpoint answers or Chrome exits, and Firefox until it
  announces its WebDriver BiDi endpoint or ends. On a loaded machine a fresh display and browser can stand
  silent for seconds before they answer, where an idle machine answers in about half a second.
- **An idle browser ends.** A browser no read has used or waited for in three minutes is ended as at the
  end below and gives its place back; the next page in that session starts a fresh one, with fresh cookies
  (on Firefox, a fresh copy of the owner's profile). A record stands as long as its session — a resident
  program's for as long as the program stands — and a session that read one page an hour ago would
  otherwise keep a place, and a browser's memory, for hours. It is the release of a browser nothing is
  using; a read under way or waiting is never ended by it.
- **A browser of its own, where a page asks for one:** a page opened through the render road may be
  asked for in a browser started for that page alone and ended as soon as it is read, under the same
  guard, pacing and cleanup. It is for a site that puts a check in front of the second page one browser
  process opens, which no clearing of cookies, cache or storage inside that browser lifts; the
  consumer that knows its site asks for it, and this layer names no site.
- **A page read once it has arrived, where a page asks for that:** a page opened through the render
  road may be handed a test of the address it stands at and the document's title. While they fail it
  — a site's own browser check that runs its script and then moves on to the page by itself, under
  an address of its own or in the page's place under the page's own address — or the document there
  is still loading, the browser keeps watching until the test passes or the page has come to rest, then
  reads the page it came to as any page is read.
  - The page has come to rest when the document standing in it has loaded and the network has gone
    quiet for it. Chrome says so by its own network-idle mark for that document with no navigation
    scheduled; Firefox gives no such mark, so the page has come to rest there once its last load has
    fired, no request of the page is under way, and none has begun or ended for half a second. A check
    that moves on by itself loads another document or asks the network as it works, and either keeps the
    watch going.
  - Nothing is clicked or solved: a check that waits for a person comes to rest and is read as it stands.
    So is a check that waits on a timer of its own with nothing on the network; one that holds the network
    open without moving on is watched until its caller lets go of the read.
  - The consumer that knows its site names the test, and this layer names no site; a site tool names it
    as data the web helper makes the test of (docs/site-contract.md, 「What a tool is given」).
- **At the end:**
  - The browser is asked to close, sent `SIGTERM`, and waited for until it has ended; whatever is left of
    Firefox's process group is then killed.
  - That browser's Xvfb is sent `SIGTERM` and waited for until it has ended.
  - The browser's profile folder — Chrome's temporary profile, or Firefox's copy of the owner's profile —
    and everything under it are deleted. The owner's profile itself is never deleted, emptied or written
    to: a removal refuses any folder that is not one of this layer's own by its name, or that is the owner's
    profile, stands inside it or holds it.
- **A navigation waits for the page's load event,** or for the browser or its connection to end.
  - A navigation the site replaces before its page has loaded — a redirect of its own, or a script check
    that moves on to the next address while the first page is still loading — is waited on through the
    one that took its place. Only a navigation that fails before its document has arrived fails the read.
- **The page is read once it holds still:** two snapshots taken 150 ms apart that agree, or the snapshot
  standing two seconds after the load. A page that keeps changing its own text — a clock, a ticker — never
  agrees with itself, and the two seconds say when its snapshot is taken; they never fail the read. A
  snapshot has a size limit.
- **One navigation at a time** in the same context, so concurrent fallbacks never overwrite each other's
  page, status or cookie observations.
- **If the process that started the browser dies suddenly** — the web helper, or the tool host for a
  site tool's browser — Chrome ends with it through the kernel parent-death signal, and Xvfb through
  the same signal; a helper ends with the tool host the same way.
  - A snap's Firefox is started through the snap's privileged launcher, which drops that signal. So a
    small watcher process stands beside each Firefox, in a process group of its own, holding the reading
    end of a pipe only the starting process writes to; when that process is gone the pipe closes, and the
    watcher kills Firefox's process group.
- **Leftover profiles** are reclaimed by the next start, after it confirms the owner is not alive:
  Chrome's in the temporary directory, Firefox's copies in the profiles' folder.

### How a leftover profile is reclaimed

1. **Stop every process that points at the profile path** by argument or environment variable:
   - the browser opened with `--user-data-dir` or `--profile`
   - helpers given a directory under the profile as an argument
   - Xvfb given the cookie inside the profile as an argument
   - the GPU process using a shader cache through `XDG_CACHE_HOME`
2. **Delete the profile** only after confirming none is left.
3. **Look again after deleting.** If the profile has come back, do the same again.
4. **If that cannot be confirmed within the limit,** do not delete it; leave it to the next start's
   reclaim.

- A browser helper that is shutting down must not recreate the deleted profile and leave it behind,
  which is what the second look is for.
- Only a folder whose name carries this layer's own prefix is ever looked at, and the owner's profile is
  refused by the same check as at the end above.

### What the browser returns

- **Ordinary content:** when the browser produces the ordinary body behind JavaScript, it is returned in
  the same character-slicing format, with the final URL, content type and `browser` provenance.
- **Still blocked:** if an HTTP refusal or a challenge/CAPTCHA remains in the browser too, `blocked` is
  returned.
- **No claims of bypassing:** a real browser having seen a challenge is not success, and nothing is
  recorded as having "bypassed" or "cleared" any protection.
- **Browser failures:** if the browser cannot be started, that is left as a failure as it is.

## Search engines

### Each engine is its own module

- **The web layer asks the search engines itself.** `web_search` sends each engine a request of its own
  and reads the answer with that engine's own parser. No search service stands in between.
- **An engine answers for itself.** Its module declares:
  - its name and its category;
  - whether it is Korean-native, and whether it reaches the English-language web;
  - how it builds its request and how it reads its answer into title, URL and snippet;
  - the block pages it recognises beyond the generic ones;
  - optionally a warm-up page whose cookies a first request should carry, and an alternate
    endpoint — a mobile or lite form of the same search — to try when the usual one is refused.
- **The shared engine layer names no engine and no site.** It holds the request loop every engine
  goes through: recipe, identity, cookies, pacing, diagnosis, one retry, and the record.
- **The roster is one list of engines.** A category's roster is the engines that declare that
  category. Adding an engine is writing its module and putting it in the list; nothing else
  anywhere is told about it.
- **The engines' states and cooldowns are this repository's own,** kept in its store and read by anyone
  else only through `explorer_search_engines` and the client function behind it, or the tool host's
  `explorer/search_engines` (docs/api.md); the roster and the rests are built in beside the engines' code.
- **A search may name its engines:** each by its roster name, or a category standing for every engine of
  it. A search naming none asks the general engines.

### Rosters

| Category | Engines | Asked at |
|---|---|---|
| General search | `naver`, `daum`, `duckduckgo`, `zapmeta`, `yandex` | Naver's and Daum's result pages, DuckDuckGo's lite page, ZapMeta's result page, Yandex's site-search page |
| News | `naver news`, `daum news`, `bing news` | Naver's and Daum's news tabs, Bing News' RSS |
| Images | `openverse`, `wikicommons.images`, `unsplash` | Openverse's API, the Wikimedia Commons API over file pages, Unsplash's public search |
| IT search | `github`, `stackoverflow` | GitHub's repository search API, the Stack Exchange API |

- **Korean-native:** `naver`, `daum`, `naver news` and `daum news`.
- **Two that can find an English document:** the general roster never stands on one engine for
  English. `duckduckgo`, `zapmeta` and `yandex` index the English-language web, and Naver and Daum
  answer English queries as well. A roster that answers English questions through a single engine
  is one refusal away from answering none of them.
- **Before adding an engine** to a roster, ask it the questions the roster is for from this machine
  and read the results, because neither a refusal nor an off-query answer shows in a status code.

### Order and mixing

- **Korean-native engines** are asked first for a Korean query and last for anything else.
- **One engine per request,** each through its own engine queue.
- **The final list alternates** between the engines that answered, so one engine's first page never
  pushes out every other answer.

### Pictures as fields

- **An image engine answers pictures, not snippets.** Openverse's API, the Wikimedia Commons API over file pages
  and Unsplash's public search each name a picture's direct image address, the page it stands on, a title, a
  creator, a publisher, its pixel dimensions, its media type and, where the source states one, its licence. The
  licence carries the source's own id, the licence address it gives and the attribution line it states.
- **Each engine reads its own answer.** An engine that answers pictures declares it and reads its own API's
  fields; the shared layer names no engine and no source. An engine that answers words alone is no image source
  and is left out of a picture search.
- **Nothing is invented.** A field the source does not state is null, and a licence is never inferred from how a
  page looks; a source that states no licence answers none. The bytes are not fetched here: `fetch_raster` is
  the one road to them.
- **Pictures are mixed as text hits are:** one from each answering engine in turn, deduplicated by their direct
  address, and an engine whose answer carries too little of the query is left out and marked `degraded` by the
  same terms.

### What this network cannot reach

Measured from the deployment's network with the engines' own forms of request:

- **`google`:** answers `/sorry` with `429`, in a real Chrome as well.
- **`yandex`'s ordinary search page:** redirects to its CAPTCHA. Its site-search page answers and is
  the one the `yandex` engine asks.
- **`brave`:** answers `429`.
- **`mojeek`:** answers `403`.
- **`startpage`:** returns no results without script.
- **`bing` web search:** answers with a well-formed page about the query's leading term alone — ten
  results, HTTP 200, no CAPTCHA and no error. A real browser on the same network is given the same page,
  so it is the network that is degraded and not the parser, the headers or the cookies. `bing news`
  reads a different endpoint and answers the query it was asked.
- **`html.duckduckgo.com`:** answers that the browser is not supported. DuckDuckGo's lite page
  answers, and it is the one the `duckduckgo` engine asks.

### A block: diagnosis, remedy, one retry

- **What is read:** the status, the page's wording, the redirect's destination, the headers and the
  cookies an answer set.
- **The causes,** read generically and made more precise by an engine's own recognisers:

| Cause | What shows it |
|---|---|
| `rate` | `429`, `420`, a `Retry-After`, or a refusal whose wording names too many requests |
| `network` | a refusal whose wording names the network or the IP address |
| `client-shape` | `403` or `418` without rate wording, a redirect from a public page to a login page, a challenge page |
| `cookie` | an answer that sets a cookie and sends the request back to itself or to a check page |
| `script-check` | an empty script shell, or a page that asks for script before it answers |
| `consent` | a consent wall standing in front of the results |

- **The remedies, one per cause:**
  - `rate` → **slow:** the engine's interval doubles, up to 60 seconds, and a longer `Retry-After`
    is waited out.
  - `cookie` → **warm:** the engine's warm-up page is fetched first and its cookies carried.
  - `consent` → **warm**, or the real browser where the engine has no warm-up page.
  - `client-shape` → the first not yet in the engine's recipe of: **reidentify** (the next
    identity, with the jar emptied), the engine's **alternate** endpoint, the real **browser**.
  - `script-check` → the engine's **alternate** endpoint, else the real **browser**.
  - `network` → none. The web layer does not route around its own network; the cause is reported with
    its evidence.
- **One retry.** A refused request is sent again once, with the remedy, in the same search. It is
  never sent again after that in the same search.
- **The recipe.** The remedy is written to the engine's recipe in this repository's store with its
  cause, its evidence and the time, whether or not the retry answers — so a block that comes back moves
  on to the next remedy rather than trying the same one again. The evidence is the status, the block
  wording that decided it, the header or cookie name that decided it, and the final address. Every later
  request to that engine starts from the recipe: its identity, its interval, its route (direct,
  alternate, browser) and its warm-up.
- **A recipe is kept** until a later block moves it on. Deleting an engine's recipe row starts that
  engine afresh.
- **When the retry answers,** the engine counts as healthy, and its state says which remedy cleared
  which cause.
- **When the retry is refused too,** the engine rests (below), and the search names the engine, the
  cause, the evidence and the remedy tried.
- **Naver's 접속 제한 page** lists every cause it might have — request rate, the network, a VPN, an
  extension. Measured from the deployment's network it answers every direct client at any pace and
  from every address, which rules out the rate; its book pages are then served to a real Chrome. So the
  Naver engines read it as the client's shape and move through that cause's remedies.

### Resting an engine

- **When an engine rests:** its request was refused and the one retry did not answer, or it failed
  outright.
  - The cause is written to this repository's store with the time.
  - The engine rests for that cause's length: `network` and a challenge still standing after the
    real browser, one hour; `rate` and `client-shape`, three minutes; any other failure — a `5xx`,
    a refused or broken connection — five seconds.
  - A longer `Retry-After` is followed.
  - A rest says when the engine is asked again; it never ends a request already under way.
- **Not engine suspensions:** a search its caller let go of is no evidence that the engine blocked
  anything, and writes nothing of the engine it was asking.

### An engine answering off the query

- **What it is:** an engine that answers without failing and answers something other than what it
  was asked. There is no status code for it, no CAPTCHA and no error, so nothing downstream notices.
- **How it is caught:** the query is cut into its terms at everything that is not a letter or a
  digit — `chat_message.sent` is three terms — and the whole of the engine's answer is read for
  them. An engine that answered the question it was asked carries the question's own words
  somewhere in what it returned.
- **The verdict:** fewer than half the terms carried, over a query of two terms or more, is an
  engine answering something else. A single-term query and an empty answer are never judged.
- **What follows:** that engine's results are kept out of the merged list — they are not partial
  results, they are another query's — its state goes to `degraded` with the count that decided it,
  and the search says so in its own text.
- **Not a rest:** it is not a refusal, so it carries no rest. The engine is asked again on the next
  search and judged again on that answer.

### Engine states (four)

- `healthy` — the request at that time answered with results or a normal empty result, directly or
  after a remedy
- `degraded` — an answer came, but it was off the query
- `suspended` — resting until a set time for a diagnosed cause
- `unknown` — not yet asked

### Reporting

- **Nothing hidden:** failed engines and their causes are not hidden even when there is at least one
  result.
- **Wording:** a failed engine is named with what it did and what was tried, as in
  `naver (client-shape: HTTP 418 "…접속이 일시적으로 제한…"; tried reidentify)`, and a resting one
  with until when and why.
- **An off-query engine is named on its own line,** apart from the failures, and the text says the
  list is thinner than the count of engines suggests — so a run that finds nothing reports a search
  that ran short of engines rather than a web with nothing on it.
- **A later success** is only evidence that it answered then, not that a block was bypassed.

## Deployment

- **Nothing to apply.** Search runs inside the tool host and its web helper; there is no search
  service of its own, and a deployment carries the engines as code like any other.
- **The store's tables** — engine health, engine recipes, engine cookie jars, the pacing queues and the
  pages no road read — are made by the store as it is first opened (docs/store.md).
