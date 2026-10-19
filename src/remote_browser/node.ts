// The node's own half of a remote browser run: one process that drives a browser over CDP there.
//
// It is shipped to the node whole and run by bun or node, so it imports nothing but the runtime's
// own modules. Everything it is told arrives in its environment — the plan in BMO_RB_PLAN, each
// secret in BMO_RB_SECRET_<NAME> — and everything it says is one JSON object a line on stdout. A
// secret is typed into the page and never printed (docs/remote-browser.md).

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

type Step = Record<string, unknown> & { do: string };

interface Plan {
  port: number;
  profile: string;
  // The step to begin at and the page to begin on, where an earlier exec paused.
  start: number;
  target: string | null;
  // How long this one exec may run before it pauses and hands the rest to the next.
  budgetMs: number;
  steps: Step[];
}

interface Target {
  id: string;
  type: string;
  url: string;
  title: string;
  webSocketDebuggerUrl?: string;
}

const WINDOWS_POWERSHELL = "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";
// What one exec may print before it pauses: well under the tunnel's own output ceiling.
const OUTPUT_MOST = 5 * 1024 * 1024;
const WAIT_DEFAULT_MS = 30_000;
const WAIT_MOST_MS = 120_000;
const VALUE_MOST = 4_000;

let printed = 0;
// Every dialog a page raised, on whichever page, and how many of them have been said.
const dialogs: { type: string; message: string }[] = [];
let dialogsSaid = 0;

function say(event: Record<string, unknown>): void {
  const line = `${JSON.stringify(event)}\n`;

  printed += line.length;
  process.stdout.write(line);
}

function sayDialogs(index: number): void {
  for (const dialog of dialogs.slice(dialogsSaid)) say({ e: "dialog", i: index, ...dialog });
  dialogsSaid = dialogs.length;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

class StepFailure extends Error {}

function fail(reason: string): never {
  throw new StepFailure(reason);
}

// One CDP connection to one page, with the frames' main-world contexts kept as they come and go.
class PageSession {
  private next = 0;
  private readonly waiting = new Map<number, { resolve: (value: Record<string, unknown>) => void; reject: (error: Error) => void }>();
  // frameId → the id of its default execution context.
  readonly contexts = new Map<string, number>();
  // How many times the page has fired its load event, and the frames still loading now.
  loads = 0;
  readonly busy = new Set<string>();

  private constructor(private readonly socket: WebSocket) {}

  static async open(url: string): Promise<PageSession> {
    const socket = new WebSocket(url);

    await new Promise<void>((resolve, reject) => {
      socket.onopen = () => resolve();
      socket.onerror = () => reject(new Error(`the page's debugging socket did not open (${url})`));
    });

    const session = new PageSession(socket);

    socket.onmessage = (message) => session.receive(String(message.data));
    socket.onclose = () => {
      for (const one of session.waiting.values()) one.reject(new Error("the page's debugging socket closed"));
      session.waiting.clear();
    };
    await session.call("Page.enable");
    await session.call("DOM.enable");
    await session.call("Runtime.enable");

    return session;
  }

  private receive(text: string): void {
    const message = JSON.parse(text) as { id?: number; result?: Record<string, unknown>; error?: { message?: string }; method?: string; params?: Record<string, any> };

    if (message.id !== undefined) {
      const waiter = this.waiting.get(message.id);

      this.waiting.delete(message.id);
      if (message.error !== undefined) waiter?.reject(new Error(message.error.message ?? "CDP error"));
      else waiter?.resolve(message.result ?? {});
      return;
    }

    const params = message.params ?? {};

    if (message.method === "Runtime.executionContextCreated") {
      const context = params.context as { id: number; auxData?: { frameId?: string; isDefault?: boolean } };

      if (context.auxData?.isDefault === true && context.auxData.frameId !== undefined) this.contexts.set(context.auxData.frameId, context.id);
    } else if (message.method === "Runtime.executionContextDestroyed") {
      for (const [frame, id] of this.contexts) if (id === params.executionContextId) this.contexts.delete(frame);
    } else if (message.method === "Runtime.executionContextsCleared") {
      this.contexts.clear();
    } else if (message.method === "Page.loadEventFired") {
      this.loads += 1;
    } else if (message.method === "Page.frameStartedLoading") {
      this.busy.add(String(params.frameId));
    } else if (message.method === "Page.frameStoppedLoading" || message.method === "Page.frameDetached") {
      this.busy.delete(String(params.frameId));
    } else if (message.method === "Page.javascriptDialogOpening") {
      // An alert left open holds every later call; it is read, written down and accepted.
      dialogs.push({ type: String(params.type), message: String(params.message ?? "") });
      void this.call("Page.handleJavaScriptDialog", { accept: true }).catch(async () => undefined);
    }
  }

  call(method: string, params: Record<string, unknown> = {}): Promise<Record<string, any>> {
    const id = ++this.next;

    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close(): void {
    this.socket.close();
  }
}

class Browser {
  session: PageSession | null = null;
  target: string | null;

  constructor(private readonly port: number, target: string | null) {
    this.target = target;
  }

  private url(path: string): string {
    return `http://127.0.0.1:${this.port}${path}`;
  }

  async version(): Promise<{ Browser?: string } | null> {
    try {
      const answer = await fetch(this.url("/json/version"), { signal: AbortSignal.timeout(2_000) });

      return answer.ok ? ((await answer.json()) as { Browser?: string }) : null;
    } catch {
      return null;
    }
  }

  async pages(): Promise<Target[]> {
    const answer = await fetch(this.url("/json/list"), { signal: AbortSignal.timeout(5_000) });

    return ((await answer.json()) as Target[]).filter((one) => one.type === "page");
  }

  async open(url: string): Promise<Target> {
    const answer = await fetch(this.url(`/json/new?${url}`), { method: "PUT", signal: AbortSignal.timeout(10_000) });

    if (!answer.ok) fail(`the browser would not open a new page (${answer.status})`);

    return (await answer.json()) as Target;
  }

  async use(target: Target): Promise<void> {
    if (this.session !== null && this.target === target.id) return;

    this.session?.close();
    this.session = null;
    await fetch(this.url(`/json/activate/${target.id}`), { signal: AbortSignal.timeout(5_000) }).catch(async () => undefined);
    this.session = await PageSession.open(target.webSocketDebuggerUrl ?? `ws://127.0.0.1:${this.port}/devtools/page/${target.id}`);
    this.target = target.id;
  }

  async closePage(): Promise<string | null> {
    const closing = this.target;

    this.session?.close();
    this.session = null;
    this.target = null;
    if (closing !== null) await fetch(this.url(`/json/close/${closing}`), { signal: AbortSignal.timeout(5_000) }).catch(async () => undefined);

    return closing;
  }

  // The browser itself shut down, over its own debugging socket.
  async quit(): Promise<void> {
    const version = (await (await fetch(this.url("/json/version"), { signal: AbortSignal.timeout(5_000) })).json()) as { webSocketDebuggerUrl?: string };

    this.session?.close();
    this.session = null;
    this.target = null;
    if (version.webSocketDebuggerUrl === undefined) fail("the browser gave no socket to close it through");

    const socket = new WebSocket(version.webSocketDebuggerUrl);

    await new Promise<void>((resolve, reject) => {
      socket.onopen = () => resolve();
      socket.onerror = () => reject(new Error("the browser's own debugging socket did not open"));
    });
    socket.send(JSON.stringify({ id: 1, method: "Browser.close" }));

    // Shut is when the port stops answering, not when the request was sent.
    for (let turn = 0; turn < 40; turn += 1) {
      await sleep(250);
      if ((await this.version()) === null) return;
    }
    fail(`the browser on port ${this.port} was asked to shut down and is still answering`);
  }

  // The page every step acts on: the one chosen, or the first there is where none has been.
  async page(): Promise<PageSession> {
    if (this.session !== null) return this.session;

    const pages = await this.pages();
    const chosen = pages.find((one) => one.id === this.target) ?? pages[0];

    if (chosen === undefined) fail("the browser has no page open");
    await this.use(chosen);

    return this.session!;
  }
}

function edgeProfileWindows(profile: string, port: number): string[] {
  const script = [
    "$e = @(\"${env:ProgramFiles(x86)}\\Microsoft\\Edge\\Application\\msedge.exe\", \"$env:ProgramFiles\\Microsoft\\Edge\\Application\\msedge.exe\") | Where-Object { Test-Path $_ } | Select-Object -First 1",
    "if (-not $e) { Write-Output 'no Edge is installed'; exit 3 }",
    `Start-Process -FilePath $e -ArgumentList '--remote-debugging-port=${port}', "--user-data-dir=$env:LOCALAPPDATA\\${profile}", '--no-first-run', '--no-default-browser-check', 'about:blank'`,
  ].join("\n");

  return ["-NoProfile", "-NonInteractive", "-Command", script];
}

function linuxBrowser(): string | null {
  for (const name of ["microsoft-edge", "microsoft-edge-stable", "google-chrome", "chromium", "chromium-browser"]) {
    const found = spawnSync("sh", ["-c", `command -v ${name}`], { encoding: "utf8" });

    if (found.status === 0 && found.stdout.trim() !== "") return found.stdout.trim();
  }

  return null;
}

// The browser on its own profile and its own debugging port: attached to where it already listens,
// started where it does not. Edge on the Windows side of a WSL node, a Chromium browser on Linux.
async function ensureBrowser(browser: Browser, plan: Plan): Promise<void> {
  const already = await browser.version();

  if (already !== null) {
    say({ e: "browser", launched: false, product: already.Browser ?? null });
    return;
  }

  if (existsSync(WINDOWS_POWERSHELL)) {
    const started = spawnSync(WINDOWS_POWERSHELL, edgeProfileWindows(plan.profile, plan.port), { encoding: "utf8", timeout: 30_000 });

    if (started.status !== 0) fail(`Edge could not be started: ${(started.stdout + started.stderr).trim().slice(0, 300) || `powershell ended with ${started.status}`}`);
  } else {
    const binary = linuxBrowser();

    if (binary === null) fail("no Edge or Chromium browser is installed on this node");

    const profileDir = join(homedir(), ".local", "share", "bmo-remote-browser", plan.profile);
    const args = [`--remote-debugging-port=${plan.port}`, `--user-data-dir=${profileDir}`, "--no-first-run", "--no-default-browser-check"];

    mkdirSync(profileDir, { recursive: true });
    if (process.env.DISPLAY === undefined && process.env.WAYLAND_DISPLAY === undefined) args.push("--headless=new");
    // Its own session, so the end of this exec's process group does not take the browser with it.
    spawn(binary, [...args, "about:blank"], { detached: true, stdio: "ignore" }).unref();
  }

  for (let turn = 0; turn < 40; turn += 1) {
    const version = await browser.version();

    if (version !== null) {
      say({ e: "browser", launched: true, product: version.Browser ?? null });
      return;
    }
    await sleep(500);
  }

  fail(`the browser did not open its debugging port ${plan.port} within 20 seconds — a browser already running on the profile ${plan.profile} without that port keeps it shut, and has to be closed first`);
}

function flatFrames(tree: { frame: { id: string; url: string; name?: string }; childFrames?: unknown[] }): { id: string; url: string; name: string }[] {
  const children = (tree.childFrames ?? []) as (typeof tree)[];

  return [{ id: tree.frame.id, url: tree.frame.url, name: tree.frame.name ?? "" }, ...children.flatMap(flatFrames)];
}

// The execution context a step's `frame` names — by the frame's name or a piece of its address —
// or none for the page's top document.
async function contextOf(page: PageSession, frame: unknown, until: number): Promise<number | undefined> {
  if (typeof frame !== "string") return undefined;

  for (;;) {
    const { frameTree } = await page.call("Page.getFrameTree");
    const found = flatFrames(frameTree).slice(1).find((one) => one.name === frame || one.url.includes(frame));
    const context = found === undefined ? undefined : page.contexts.get(found.id);

    if (context !== undefined) return context;
    if (Date.now() > until) fail(`no frame named or addressed ${JSON.stringify(frame)} is on the page`);
    await sleep(250);
  }
}

async function evaluate(page: PageSession, expression: string, context: number | undefined, byValue = true): Promise<Record<string, any>> {
  const answer = await page.call("Runtime.evaluate", {
    expression,
    returnByValue: byValue,
    awaitPromise: true,
    ...(context === undefined ? {} : { contextId: context }),
  });

  if (answer.exceptionDetails !== undefined) {
    const details = answer.exceptionDetails as { text?: string; exception?: { description?: string } };

    fail(`the page threw: ${(details.exception?.description ?? details.text ?? "an exception").slice(0, 300)}`);
  }

  return answer.result as Record<string, any>;
}

// The element a step names, by selector or by the words it shows, found in the page itself. Of
// several showing the words, the smallest is the one meant — the button rather than the bar it
// sits in.
function finder(step: Step): string {
  return `(() => {
    const visible = (e) => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.display !== "none"; };
    const selector = ${JSON.stringify(typeof step.selector === "string" ? step.selector : null)};
    const text = ${JSON.stringify(typeof step.text === "string" && step.do === "click" ? step.text : null)};
    if (selector !== null) { const e = document.querySelector(selector); return e && visible(e) ? e : null; }
    const norm = (s) => (s || "").replace(/\\s+/g, " ").trim();
    const want = norm(text);
    const label = (e) => norm(e.innerText || e.value || e.getAttribute("title") || e.getAttribute("aria-label") || e.getAttribute("alt"));
    const all = [...document.querySelectorAll("body *")].filter((e) => !["SCRIPT", "STYLE"].includes(e.tagName) && visible(e));
    let hits = all.filter((e) => label(e) === want);
    if (hits.length === 0) hits = all.filter((e) => label(e).includes(want));
    const area = (e) => { const r = e.getBoundingClientRect(); return r.width * r.height; };
    hits.sort((a, b) => area(a) - area(b));
    return hits[0] || null;
  })()`;
}

function named(step: Step): string {
  return typeof step.selector === "string" ? step.selector : JSON.stringify(step.text);
}

// Where on the screen the element stands, in the top page's own coordinates whichever frame holds
// it, once it has been scrolled into view.
async function pointOf(page: PageSession, step: Step, until: number): Promise<{ x: number; y: number }> {
  if (typeof step.x === "number" && typeof step.y === "number") return { x: step.x, y: step.y };

  const context = await contextOf(page, step.frame, until);

  for (;;) {
    // Scrolled at once rather than smoothly, measured once it has come to rest, and taken only where
    // the point is the element's own and nothing lies over it.
    const element = await evaluate(page, `(async () => {
      const e = ${finder(step)};
      if (!e) return null;
      e.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
      await new Promise((resolve) => setTimeout(resolve, 60));
      const r = e.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return hit && (hit === e || e.contains(hit) || hit.contains(e)) ? e : null;
    })()`, context, false);

    if (element.objectId !== undefined) {
      const { quads } = await page.call("DOM.getContentQuads", { objectId: element.objectId });
      const quad = (quads as number[][])[0];

      if (quad !== undefined) {
        return { x: Math.round((quad[0]! + quad[2]! + quad[4]! + quad[6]!) / 4), y: Math.round((quad[1]! + quad[3]! + quad[5]! + quad[7]!) / 4) };
      }
    }
    if (Date.now() > until) fail(`nothing visible and uncovered matches ${named(step)}`);
    await sleep(250);
  }
}

const KEY_CODES: Record<string, number> = {
  Enter: 13, Tab: 9, Escape: 27, Backspace: 8, Delete: 46, Space: 32, Home: 36, End: 35,
  PageUp: 33, PageDown: 34, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40,
};

async function waitFor(check: () => Promise<boolean>, until: number, what: string): Promise<void> {
  for (;;) {
    try {
      if (await check()) return;
    } catch (error) {
      // A page halfway through navigating has no context to ask; it is asked again.
      if (error instanceof StepFailure) throw error;
    }
    if (Date.now() > until) fail(`waited in vain for ${what}`);
    await sleep(250);
  }
}

// The page settled: a navigation that is coming has had its moment to start, no frame is still
// loading, and the document says it is complete. Given the load count from before a navigation,
// it also waits for that navigation's own load, so the page left behind cannot answer for it.
async function loaded(page: PageSession, until: number, loadsBefore: number | null = null): Promise<void> {
  await sleep(500);
  await waitFor(
    async () => page.busy.size === 0 && (loadsBefore === null || page.loads > loadsBefore) && (await evaluate(page, "document.readyState", undefined)).value === "complete",
    until,
    "the page to finish loading",
  );
}

async function navigate(page: PageSession, url: string, until: number): Promise<void> {
  const before = page.loads;
  const answer = await page.call("Page.navigate", { url });

  if (typeof answer.errorText === "string") fail(`the page could not be opened: ${answer.errorText}`);
  await loaded(page, until, before);
}

function secretOf(step: Step): string {
  const name = String(step.secret);
  const value = process.env[`BMO_RB_SECRET_${name}`];

  if (value === undefined) fail(`the secret ${name} was not handed to this run`);

  return value;
}

function valueText(value: unknown): unknown {
  const text = typeof value === "string" ? value : JSON.stringify(value ?? null);

  return text.length > VALUE_MOST ? `${text.slice(0, VALUE_MOST)}…` : value;
}

async function shoot(page: PageSession, format: string): Promise<string> {
  const { data } = await page.call("Page.captureScreenshot", format === "png" ? { format: "png" } : { format: "jpeg", quality: 80 });

  return String(data);
}

// One step, answering with what it did in a line and, for a read, the value it read.
async function runStep(browser: Browser, step: Step, index: number): Promise<{ detail: string; value?: unknown }> {
  const until = Date.now() + Math.min(typeof step.timeoutMs === "number" ? step.timeoutMs : WAIT_DEFAULT_MS, WAIT_MOST_MS);

  if (step.do === "page") {
    if (typeof step.new === "string") {
      const opened = await browser.open("about:blank");

      await browser.use(opened);
      await navigate(browser.session!, step.new, until);

      return { detail: `opened a new page at ${step.new}` };
    }

    const match = String(step.match);
    let found: Target | undefined;

    await waitFor(async () => {
      found = (await browser.pages()).find((one) => one.url.includes(match) || one.title.includes(match));
      return found !== undefined;
    }, until, `a page whose address or title holds ${JSON.stringify(match)}`);
    await browser.use(found!);

    return { detail: `on the page ${found!.url}` };
  }
  if (step.do === "close") {
    return { detail: (await browser.closePage()) === null ? "no page was chosen to close" : "closed the page" };
  }
  if (step.do === "quit") {
    await browser.quit();

    return { detail: "shut the browser down" };
  }

  const page = await browser.page();

  switch (step.do) {
    case "goto": {
      await navigate(page, String(step.url), until);

      return { detail: `went to ${step.url}` };
    }
    case "click": {
      const { x, y } = await pointOf(page, step, until);

      await page.call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
      await page.call("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
      await page.call("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
      await sleep(400);

      return { detail: `clicked ${step.x === undefined ? named(step) : "the point"} at (${x}, ${y})` };
    }
    case "type": {
      const text = step.secret !== undefined ? secretOf(step) : String(step.text);

      if (typeof step.selector === "string") {
        const context = await contextOf(page, step.frame, until);
        const clear = step.clear !== false;

        await waitFor(async () => (await evaluate(page, `(() => { const e = ${finder(step)}; if (!e) return false; e.focus(); if (${clear} && "value" in e) e.value = ""; return true; })()`, context)).value === true, until, `a field matching ${step.selector}`);
      }
      await page.call("Input.insertText", { text });

      const what = step.secret !== undefined ? `the secret ${String(step.secret)} (${text.length} characters)` : `${text.length} characters`;

      return { detail: `typed ${what}${typeof step.selector === "string" ? ` into ${step.selector}` : ""}` };
    }
    case "key": {
      const key = String(step.key);
      const code = KEY_CODES[key] ?? (key.length === 1 ? key.toUpperCase().charCodeAt(0) : 0);
      const text = key === "Enter" ? "\r" : key.length === 1 ? key : undefined;
      const common = { key, code: key.length === 1 ? `Key${key.toUpperCase()}` : key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code };

      await page.call("Input.dispatchKeyEvent", { type: text === undefined ? "rawKeyDown" : "keyDown", ...common, ...(text === undefined ? {} : { text }) });
      await page.call("Input.dispatchKeyEvent", { type: "keyUp", ...common });
      await sleep(200);

      return { detail: `pressed ${key}` };
    }
    case "wait": {
      if (typeof step.ms === "number") {
        await sleep(Math.min(step.ms, WAIT_MOST_MS));

        return { detail: `waited ${step.ms}ms` };
      }

      const context = await contextOf(page, step.frame, until);

      if (typeof step.selector === "string") {
        await waitFor(async () => (await evaluate(page, `(${finder(step)}) !== null`, context)).value === true, until, `something visible matching ${step.selector}`);
      } else if (typeof step.text === "string") {
        await waitFor(async () => (await evaluate(page, `document.body !== null && document.body.innerText.includes(${JSON.stringify(step.text)})`, context)).value === true, until, `the words ${JSON.stringify(step.text)}`);
      } else if (typeof step.url === "string") {
        await waitFor(async () => String((await evaluate(page, "location.href", undefined)).value).includes(String(step.url)), until, `an address holding ${JSON.stringify(step.url)}`);
      } else {
        await loaded(page, until);
      }

      return { detail: "the wait was met" };
    }
    case "read": {
      const context = await contextOf(page, step.frame, until);
      const value = (await evaluate(page, String(step.js), context)).value;

      return { detail: `read ${typeof step.as === "string" ? step.as : "a value"}`, value: valueText(value) };
    }
    case "shot": {
      const format = step.format === "png" ? "png" : "jpeg";

      say({ e: "shot", i: index, name: typeof step.as === "string" ? step.as : `step-${index + 1}`, format, data: await shoot(page, format) });

      return { detail: "took a screenshot" };
    }
  }

  return fail(`there is no step called ${step.do}`);
}

async function where(browser: Browser): Promise<{ url: string | null; title: string | null }> {
  try {
    if (browser.session === null) return { url: null, title: null };

    const page = browser.session;

    if (page === null) return { url: null, title: null };

    const value = (await evaluate(page, "[location.href, document.title]", undefined)).value as [string, string];

    return { url: value[0], title: value[1] };
  } catch {
    return { url: null, title: null };
  }
}

async function main(): Promise<void> {
  const plan = JSON.parse(process.env.BMO_RB_PLAN ?? "null") as Plan | null;

  if (plan === null) {
    say({ e: "fail", i: null, reason: "no plan was handed to the helper" });
    return;
  }
  if (typeof WebSocket === "undefined") {
    say({ e: "fail", i: null, reason: "this node's runtime has no WebSocket; bun, or node 22 or later, is needed" });
    return;
  }

  const began = Date.now();
  const browser = new Browser(plan.port, plan.target);
  let index = plan.start;

  try {
    if (plan.start === 0) await ensureBrowser(browser, plan);

    for (; index < plan.steps.length; index += 1) {
      // Each exec takes at least one step, so a budget however short still moves the run on.
      if (index > plan.start && (Date.now() - began > plan.budgetMs || printed > OUTPUT_MOST)) {
        say({ e: "pause", next: index, target: browser.target });
        return;
      }

      const step = plan.steps[index]!;
      const done = await runStep(browser, step, index);
      const at = await where(browser);

      sayDialogs(index);
      say({ e: "step", i: index, do: step.do, detail: done.detail, ...(done.value === undefined ? {} : { value: done.value }), url: at.url });
    }

    const at = await where(browser);

    say({ e: "done", target: browser.target, url: at.url, title: at.title });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    const at = await where(browser);

    sayDialogs(index);
    say({ e: "fail", i: index, reason, url: at.url, title: at.title });

    // What the screen showed when it failed is what tells anybody why.
    if (browser.session !== null) {
      try {
        say({ e: "shot", i: index, name: "failure", format: "jpeg", data: await shoot(browser.session, "jpeg") });
      } catch {
        // No picture to be had; the reason stands on its own.
      }
    }
  } finally {
    browser.session?.close();
  }
}

await main();
process.exit(0);
