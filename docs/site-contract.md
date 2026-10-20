# The site contract

## What a site tool is

- **A site read again and again is a tool, not a search.** When the same site is opened for the
  same kind of value a second time — a rate, a spot price, a listing, a stock figure — the way to
  read it is written down once and kept. Every later question about that value is one call.
- **Why it is not left to the search.** A search finds the page afresh each time: it spends the
  room's wait on it, it can land on a different page than last time, and what it quotes depends on
  which of them it happened to open. A tool answers the same way today and next month, and when
  the site moves under it, one repair fixes every question that follows.
- **It is not a throwaway tool.** A tool written for one job is thrown away after it. A site tool is
  kept, listed, tested, and upgraded as the site changes.

## What becomes one

- **Only what is asked for.** A tool is made where its maintainer orders one. Nothing raises a tool's
  row by itself: a tool for every site a search meets would be a tool for every site there is.
- **A lookup that comes round again.** The same site, the same kind of value, asked for a second
  and a third time. The search history bmo-artifact keeps is where that is read off: a question asked
  three times over a fortnight is the clearest case there is.
- **Not a one-off.** A question asked once is a search, however awkward the page was to read.
- **A value, not an opinion.** A tool reads a figure or a listing the site itself publishes. What
  a page argues, recommends or reviews is read by the search, not by a tool.
- **The site has to show the value to anyone.** A site that serves the value as JSON or as plain
  HTML is read with a plain request. A page it serves only to a real browser — a shop that sends
  other clients to its login page, a page filled in by script — is read through the real browser
  (「What a tool is given」). A value behind a login or behind a challenge is never a tool's.

## Where a tool lives

- **One tool per site,** placed with the tool host by its own manifest: a repository of its own, or a
  folder of a service repository's `tools/`. It holds the reading and the tool together, its manifest,
  its input schemas and the bodies its tests read, under its own `fixtures/`. Its executable answers
  each call of its entries and each claim the host asks of it, and reaches the site through the
  record's web helper (「What a tool is given」).
- **The reading is pure** — a parse and a render — so it is checked against bodies kept as they
  came; the fetching is handed in by whoever serves the tool.
- **The site contract is a module of this repository:** the site-tool trait, the roads a tool is
  given (「What a tool is given」) and the stand-in a test reads its fixtures through, the client a
  placed tool reaches the web helper by and the wrapper serving a site tool as a placed entry, the
  readings several tools share — markup, scripts, numbers, journals, the forum engines. This
  repository's own tools and the web helper link it.
- **A site tool of another repository stands on the wire, not on this code:** it speaks the tool host's
  call protocol and asks the record's web helper on its socket, in the shapes 「The web helper's socket」
  states. Nothing of this repository is a dependency of it.
- **The forms a road answers in** — the kinds a web read fails by, its error and result, a search's hits
  and an engine's health, the ceiling a fetch's asked length is clamped to, the part of a text cut to the
  asked length with where the rest starts, and the page text made one space to a run and one line to a
  blank run — stand below both the web layer and the site contract, so a road crosses the helper's
  socket unchanged and a placed site tool's executable carries no web layer.
- **A page is read by the parser of its format.** Markup is read as a tree, scraper over html5ever
  parsing it and CSS selectors finding the elements a value stands in; a script's values by the
  JavaScript parser oxc; JSON by serde, with the readers' ordered tree where a site's own key order
  is what a reader shows; XML and feeds by roxmltree. A pattern is run only over text already
  taken out of an element — a price, a date, a line of a script — never over the markup itself, and
  a reading no library gives says why beside its code.
- **The catalogue is the tools themselves.** A tool answers the site-tool trait — its name, title,
  what it brings back in one line, the site it reads, the date it was made, the description its
  caller shows, its arguments as a type, the page addresses it claims (「A page a tool claims」), and
  whether it reads through the search engines, which holds it to the plain search's refusals.
  - **A placed tool's manifest declares what the host reads without running it:** its line in the
    `web` listing, its item in the `web` namespace's site catalogue and the hosts whose pages it
    claims (the tool host's `docs/mcp-host.md`, 「What a namespace says of itself」). A test of the
    tool's own holds the manifest to what the trait answers, so the two cannot go apart. It is resident
    where the searcher's lines list it (the tool host's `docs/toolbook.md`, 「Which tools are
    resident」): shown to a searcher by its own name rather than reached through the searcher's toolbook.
  - Adding a tool is writing it with its manifest and putting it with the tool host; nothing else
    anywhere is told about it.
- **The tool host serves every site tool** under the `web` namespace out of its registry, beside the
  web's core tools (the tool host's `docs/mcp-host.md`, 「What a record's tools keep」), each asking for
  `web.sites`, which only the searcher role gives, so every research session is shown the same tools
  with no list of its own, and no other line is shown them.

## What placing a tool costs

- **Measured on the operations machine on 2026-10-14,** release builds, a reader of
  `https://r.jina.ai/https://example.com/` (the wrapped page, by the direct road) with no pause
  between calls to one host: the linked side a tool host with every site tool linked, the placed
  side a host serving the web helper, `web_search`, `web_fetch` and that reader out of a tools tree.
  Eleven calls each way, the first apart and the other ten as their median and range.
  - **Its own call through the toolbook:** linked, first 120 ms, then 97 ms (89 to 119); placed,
    first 173 ms with the helper started for the record, then 121 ms (113 to 134).
  - **`web_fetch` claimed by it:** linked, first 89 ms, then 103 ms (88 to 111); placed, first
    146 ms, then 124 ms (114 to 154).
  - **The executable alone,** started, handed a claim on standard input, answering and ending: a
    median of 0.7 ms over 21 runs (0.6 to 1.3).
- **The placed call costs about 20 to 25 ms more than the linked one,** what the call's own process,
  its socket to the record's helper and the helper's answer add, beside a fetch of about 90 ms.

## What a tool is given

- **It never opens a connection itself.** The record's web reading hands it its roads, so one
  place holds the pacing, the deadline and the policy every outward request of this machine goes
  through (docs/web-access.md). A placed tool asks the record's web helper for each road on the
  helper's socket (「The web helper's socket」):
  - `page` — one public page or endpoint, fetched the way a browser would ask for it; a tool may
    post a body to it with headers of its own, for an endpoint the site's own page posts to, or ask
    for it with headers of its own and no body, for a site that answers a browser's request
    otherwise than another client's;
  - `rendered` — one page opened in the real Chrome and read after its script has run, for a site
    that serves the page only to a real browser; a tool may ask for it in a browser of its own,
    started for that page and ended after it, where its site checks the second page one browser
    opens, and may name when the page it came for has arrived, so the page is read only once the
    site's own browser check has moved on to it by itself (docs/web-access.md);
    - the arrival is named as data, since it crosses the helper's socket: the address paths the
      site's check stands under, the titles the check shows, and whether an untitled page has
      arrived only at an address carrying one of given query pairs;
  - `engine` — the search engines, for a site that refuses this machine's readers outright;
  - `document` — one page read and sliced the way `web_fetch` reads it, its PDF reading and its
    Chrome fallback included, for a tool whose site wraps another site's page or serves its sheets
    as PDFs; a tool may ask for a PDF's layout reading, which keeps a table's rows apart
    (docs/web-access.md, 「A PDF」);
  - `head` — what a server answers about one address asked for without its body: its status, the
    address it ended at and its headers, whatever the status, for a site that serves files too
    large to read whose size, date and checksum stand in the headers.
- **Every road passes the reading's checks,** whatever shape its request takes: each hop's address is checked
  and connected to as it passed, each hop is paced per host, and a redirect is followed one checked hop at a time
  (docs/web-access.md, 「Address checks」 and 「Pacing」). A `page` and a `head` keep their own request: the
  browser's name and language and the tool's own headers, with no cookies kept between them.
- **The research guard stands round every call,** in the record's web reading:
  - a call is refused as research has closed before its tool reads anything;
  - a tool that reads through the engines is held to the same refusals the plain web search is,
    and its answer counts as a search;
  - a failure is answered as the reading answers a failed page, a read cut short by the research
    close said as the close.
- **A tool's failed road is its own to say in its answer;** nothing of it is written down.

## The web helper's socket

- **One request a connection:** one JSON object on one line, answered with one JSON object on one line,
  on the socket the tool host hands a call under `shared.web`. A line that does not read is answered as a
  failure: a tool result marked `isError`, its text opening `ERROR: the web helper was asked what it does
  not read`.
- **Each request names its `kind`,** with the fields of its kind beside it:

| `kind` | Its fields | Answered with |
|---|---|---|
| `search` | `query`, `count?`, `language?`, `category?` | the tool result of a web search |
| `fetch` | `url`, `offset?`, `max_chars?`, `raw` (false where absent) | the tool result of a page read |
| `picture` | `image`, the address of an image file | the tool result holding the picture |
| `page` | `url`, `request?`: `body?` and `headers`, a list of name and value pairs | the road's result: the body |
| `rendered` | `url`, `fresh` (a browser of its own; false where absent), `arrival?` | the road's result: `url`, `status`, `html` and `text` |
| `engine` | `query`, `count` | the road's result: the search's `hits`, `unresponsive`, `off_topic` and `health` |
| `document` | `url`, `slice`: `offset?`, `max_chars?`, `layout` | the road's result: the sliced text |
| `head` | `url` | the road's result: `url`, `status` and `headers`, by their lower-cased names |
| `opening` | `engine`, whether the tool reads through the engines | the refusal's text, or null where the call may read |
| `ended` | `engine`, and `outcome`, the tool's own result | the tool result the call answers with |

- **An arrival** is `check_paths` and `check_titles`, each a list that may be absent, `titled`, and
  `untitled_at`, a list of query name and value pairs that may be absent.
- **A road's result** is `{"Ok": …}` with what the road gave, or `{"Err": …}` with the failure it met:
  `message`, the sentence; `kind`, one of `blocked`, `deadline`, `unsafe_address`, `too_large`,
  `transport`, `http` and `browser_unavailable`; `status?`; `url?`; and `chrome_fallback`, true where the
  real Chrome, opened after the direct read failed, failed the page too. A helper out of reach fails the
  road as `browser_unavailable`, a program this machine needs that could not be had.
- **A tool result** is the tool host's: `content`, `isError` and `structuredContent` where there is one.

## A page a tool claims

- **A tool names the addresses it answers for.** A descriptor may carry a claim: which page
  addresses the tool reads better than a plain fetch does, and their forms in one line.
- **`web_fetch` asks the tool host first.** Its call-back socket answers which entry the record
  holds claims the address, and with which arguments (the tool host's `docs/mcp-host.md`, 「A call」).
  An address a tool claims is answered by that tool's reading, called through the same socket and
  headed with the tool's name, so a caller that opened the page learns that the tool exists and gets
  the tool's answer in the same call.
  - Only the entries the record holds are asked, so a session without `web.sites` reads every page
    by the plain fetch.
  - A placed tool's manifest names the hosts whose pages it claims; the host asks its executable
    only of an address on one of them or under it, and the executable answers from the trait's
    claim, with no web helper started for the asking.
  - Those hosts are the ones its forms name, and those its trait adds where a form names no host
    whole: a pattern of subdomains written as `<account>.example.com`, or a second host abbreviated
    beside the first. The tool owns that list, so a claim the forms cannot spell still reaches it.
  - A tool's own tests ask its entries for an address the same way, only those whose manifest
    claims the address's host, so a claim the trait makes on a host the manifest leaves out fails
    there rather than going unasked in the host.
  - An address asked for raw is never claimed: the markup asked for is the page's own.
- **Reading on goes with it.** `web_fetch`'s `offset` and `max_chars`, where they were given, are
  added to the claimed arguments, so the offset a claimed answer names reads on through `web_fetch`
  as it does through the tool.
  - A tool that takes no arguments of those names leaves them unread, so a site tool's input admits
    fields it does not name.
- **The catalogue line shows the claimed forms,** so the searcher reads, before it searches, which
  addresses go to which tool.

## The one list

- `bmo tool sites` prints the catalogue: each tool's name, what it fetches, the site it reads, when
  it was made, its arguments and the addresses it claims. `--detail` adds the description each
  caller shows. It prints what the running tool host says of the `web` namespace — its facts carry
  the catalogue (the tool host's `docs/mcp-host.md`, 「What a namespace says of itself」) — which is
  every tool that is live.
- **The searcher is given the same list in its task**: the listing the tool host answers for the
  `web` namespace, which a search's instructions are composed with, so a tool made
  today stands in front of tomorrow's search.
- **Read it before writing one.** It is the one place that answers whether a lookup is already
  covered, and it cannot go out of step with the code, because it is the code.
- **A placed site tool is held to the list's shape** by a test of its own: every entry asking for
  `web.sites`, other than one for the system alone, has a listing line headed with its name, a
  catalogue item saying what it fetches, the site and the date it was made, a description a caller can
  go by and a name no other entry holds, and no entry claims an address of a site no tool reads.

## How a search uses them

- **Before the first query.** Where a tool covers what is being asked, the search uses it and runs
  no query for that value at all.
- **An address a tool claims goes to the tool.** A Naver page — a book, a shopping catalog, a blog
  post, a cafe article, a place on the map — is read with its Naver tool, and a 나무위키 document with
  `namu_wiki`, rather than with `web_fetch`; `web_fetch` given one answers through the tool anyway.
- **A broken tool is repaired.** A site changes shape under its reading sooner or later. The
  search says so in `UNCONFIRMED:` with what the tool answered, gets that one value with the web
  tools for that answer, and the tool is fixed. Falling back to searching for the value every time
  is what this exists to stop.
- **A search asks for no new tool.** A site no tool covers is read with the web tools, and what
  could not be read is said in `UNCONFIRMED:`.

## Sites with no road

- **A site a job found no road into is written here,** with what it answers, so a job that meets
  it again reads why nothing reads it before probing it afresh.
- **xcancel.com** — a Nitter instance that mirrors X posts.
  - Every address it has answers 451 with its operator's notice that the service is suspended for
    legal proceedings, and a link to the same path on x.com: a post, a profile, the front page, a
    profile's RSS feed, and `rss.xcancel.com`, which redirects to that feed.
  - The notice stands in front of every path, the root and the feed included, so no page of the
    site holds a post; its answers carry `noindex`, so the search engines keep none of its text.
  - The post an xcancel address names is the X post at the same path on x.com, which the X site tool
    reads; xcancel itself gets no tool.
- **blog.csdn.net** — CSDN's blogs, the articles at `blog.csdn.net/<user>/article/details/<id>`.
  - A plain request is answered 521 with an obfuscated script that sets a cookie and reloads, a
    browser check in the page's place: an article, a blog's front page and RSS feed, the `phoenix`
    and `community/home-api` endpoints, the `<user>.blog.csdn.net` hosts, and `http://` alike.
    `m.blog.csdn.net` redirects to the same address.
  - The real Chrome is answered 403 by the site's WAF with its bot-score and CAPTCHA scripts, the
    blog's front page included.
  - `app-blog.csdn.net` serves an app shell for CSDN's own activity pages and reads no article; the
    article APIs on `bizapi.csdn.net` are the signed editor's console, closed without a login.
  - CSDN's search API (`so.csdn.net/api/v3/search`) answers anyone with listings — title, author,
    date, views and a one-paragraph digest — but finds no article by its id, and its `body` field is
    the text with letters dropped out of it, not the article.
  - No other site serves the articles' text: the Wayback Machine holds a copy only of a page
    someone archived, which a recent article seldom is.
- **bulgari.com** — Bulgari's official store, a product at `bulgari.com/<locale>/<code>.html` and a
  listing under a category path.
  - The store is Salesforce's composable storefront behind Akamai, and Akamai answers the deployment's
    network 403 with its 「Access Denied」 page on every path: a product, a listing, the front page,
    `robots.txt`, the sitemaps, every locale, the storefront's own controller and API paths, under
    any client name, and in the real Chrome alike. Where it let the real Chrome through once, the
    page came back as a script shell with no body.
  - The bare host only redirects to `www.`; there is no mobile host, and `bulgari.co.kr` is a parked
    domain.
  - Salesforce's Commerce API host answers anyone, but a product read there wants a shopper token,
    and the storefront's client is a private one whose secret only the storefront's own proxy on
    `www.` holds: the guest login refuses it as the wrong client type.
  - The search engines carry a product's name, collection and material, never its price.
  - The Wayback Machine holds some product pages whole, the price in their markup, but only as of the
    day someone archived them, a year and more back; Bulgari's counter prices move more often than
    that, so an archived price is not the store's price.
  - A page the store would serve to an ordinary visitor is refused to this network, and the web layer
    does not route around its own network (docs/web-access.md), so bulgari.com gets no tool.

## Making one

1. Read `bmo tool sites` first: a tool that already covers the value is the answer.
2. Find what the site actually serves. A page whose numbers are filled in by script usually has
   an endpoint of its own behind it, and that endpoint is what the tool reads.
3. Write the tool: the reading as pure functions, the tool beside them. Name the source page in what it
   returns, so a value it gives can be cited to the page it came from, and say in the text what the
   value is not — a shop's counter price is not the international quote, and an answer that confuses
   the two is wrong in the room.
4. Test the reading against a body kept as the site sent it, so the test says whether the reading
   still holds when the site changes.
5. Declare in its manifest its listing line, its catalogue item and the hosts it claims, and put it
   with the tool host.

## Why the catalogue is not a table

A tool is a reading of one site: a parse, a render, and what the site's own fields mean. None of
that is a row a person could rename or add. What a table would hold is a name beside code that has
to exist anyway, and the list would then be kept in two places and go out of step. So the
catalogue is the code, and what a person changes about a tool is the tool.
