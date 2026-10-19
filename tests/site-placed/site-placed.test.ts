import { afterAll, expect, test } from "bun:test";
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

// Placed site tools in one session with the web's core tools, held against an installed tool host
// (docs/site-contract.md, 「Where a tool lives」, 「A page a tool claims」): the host serves this repository's
// `web_search` and `web_fetch` out of a tools tree, its web helper registered as a shared helper, beside two site
// tools of the test's own that speak the call protocol and the helper's socket on the wire. The namespace lists the
// core tools and books the site tools, its facts catalogue the site tools, the control socket names the tool
// claiming an address, and the wrapped page reaches the record's web helper both through the site tool's own call
// and through `web_fetch`'s claim of its address. The wrapped page is on this machine, so the helper's address
// policy refuses it before anything is asked of the network, and the refusal names the wrapped host, which only the
// site tool's reading asks for.
//
// No tool host is named in the suite, so this skips there; `BMO_LIVE_TOOL_HOST` names the installed `mcp-host`, and
// `bmo-pocket` is the one beside it.

const host = process.env.BMO_LIVE_TOOL_HOST ?? "";
const pocket = join(dirname(host), "bmo-pocket");
const repository = resolve(import.meta.dir, "../..");
const fixtures = join(import.meta.dir, "fixtures");
const wrapped = "https://wrap.example.com/http://localhost:9/manual.pdf";
const post = "https://board.example.com/b/notes/102036259";
const running: ReturnType<typeof Bun.spawn>[] = [];
const scratches: string[] = [];

afterAll(async () => {
  for (const one of running.splice(0)) {
    one.kill();
    await one.exited;
  }

  for (const scratch of scratches.splice(0)) {
    rmSync(scratch, { recursive: true, force: true });
  }
});

function scratch(prefix: string): string {
  const made = mkdtempSync(prefix);

  scratches.push(made);
  return made;
}

// This repository's executables the tools tree serves, built as Cargo builds them, by their binaries' names.
function built(packages: string[]): Map<string, string> {
  const cargo = process.env.CARGO ?? "cargo";
  const flags = packages.flatMap((name) => ["-p", name]);
  const build = Bun.spawnSync([cargo, "build", ...flags, "--message-format=json-render-diagnostics"], {
    cwd: repository,
    stdout: "pipe",
    stderr: "inherit",
  });
  const executables = new Map<string, string>();

  expect(build.exitCode).toBe(0);

  for (const line of build.stdout.toString().split("\n")) {
    if (line.trim() === "") {
      continue;
    }

    const message = JSON.parse(line) as { reason?: string; target?: { name: string }; executable?: string | null };

    if (message.reason === "compiler-artifact" && message.target !== undefined && message.executable) {
      executables.set(message.target.name, message.executable);
    }
  }

  return executables;
}

// A placed folder as a deployment leaves it: the manifest and schemas of the source folder, and the executable
// where one was built.
function placed(source: string, into: string, executable?: string): void {
  mkdirSync(into, { recursive: true });

  for (const file of new Bun.Glob("*.{toml,json}").scanSync(source)) {
    copyFileSync(join(source, file), join(into, file));
  }

  if (executable !== undefined) {
    copyFileSync(executable, join(into, executable.split("/").pop() ?? ""));
  }
}

async function rpc(port: number, token: string, namespace: string, method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
  const answer = await fetch(`http://127.0.0.1:${port}/mcp/${namespace}`, {
    method: "POST",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json", accept: "application/json, text/event-stream", "mcp-protocol-version": "2025-06-18" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });

  return ((await answer.json()) as { result: Record<string, unknown> }).result;
}

async function called(port: number, token: string, namespace: string, name: string, args: Record<string, unknown>): Promise<string> {
  const result = (await rpc(port, token, namespace, "tools/call", { name, arguments: args })) as { content: { text: string }[] };

  return result.content.map((piece) => piece.text).join("\n");
}

test.skipIf(host === "" || !existsSync(host) || !existsSync(pocket))("a placed site tool answers its own call and web_fetch's claim of its address through the record's web helper", async () => {
  const root = scratch(join(tmpdir(), "explorer-site-placed-"));
  // The control socket and the calls' sockets beside it stay inside the length a unix socket path may have.
  const sockets = scratch("/tmp/sp-");
  const data = join(root, "data");
  const tools = join(root, "tools");
  const control = join(sockets, "c.sock");
  const executables = built(["web-helper", "web-search-tool", "web-fetch-tool"]);
  const helper = join(root, "staged", "web");

  placed(join(repository, "tools/_shared/web"), helper, executables.get("web-helper"));

  const put = Bun.spawnSync([pocket, "put", helper, "--placed", "--service", "bmo-explorer", "--data-root", data], { stdout: "ignore", stderr: "pipe" });

  expect(put.exitCode, put.stderr.toString()).toBe(0);

  placed(join(repository, "tools/web_search"), join(tools, "web_search", ".placed"), executables.get("web-search-tool"));
  placed(join(repository, "tools/web_fetch"), join(tools, "web_fetch", ".placed"), executables.get("web-fetch-tool"));

  for (const name of ["wrapped_pages", "board_posts"]) {
    cpSync(join(fixtures, name), join(tools, name, ".placed"), { recursive: true });
  }

  const served = Bun.spawn([host, "serve", "--port", "0", "--data-root", data, "--control", control, "--tools", tools], {
    stdout: "pipe",
    stderr: "ignore",
  });

  running.push(served);

  const said = new TextDecoder().decode((await served.stdout.getReader().read()).value);
  const port = Number(/127\.0\.0\.1:(\d+)/.exec(said)?.[1]);
  const controlled = async (path: string, body?: unknown): Promise<unknown> => {
    const answer = await fetch(`http://localhost${path}`, {
      unix: control,
      method: body === undefined ? "GET" : "POST",
      body: body === undefined ? undefined : JSON.stringify(body),
    });

    return answer.json();
  };
  const session = { line: "search", capabilities: ["web.read", "web.sites"], shown: ["web/web_search", "web/web_fetch"], dataRoot: data };
  const { token } = (await controlled("/sessions", session)) as { token: string };
  const listed = ((await rpc(port, token, "web", "tools/list", {})) as { tools: { name: string }[] }).tools.map((tool) => tool.name);
  const booked = await called(port, token, "toolbook", "list", {});
  const sites = ((await controlled("/facts/web")) as { sites: { name: string; reads: string[] }[] }).sites;

  // A site tool the record does not show by name is booked in the toolbook rather than listed (the tool host's
  // docs/toolbook.md).
  expect(listed).toEqual(expect.arrayContaining(["web_search", "web_fetch"]));
  expect(listed).not.toContain("wrapped_pages");
  expect(booked).toContain("web/wrapped_pages");
  expect(booked).toContain("web/board_posts");
  expect(sites.find((site) => site.name === "wrapped_pages")?.reads).toEqual(["wrap.example.com"]);
  expect(sites.find((site) => site.name === "board_posts")?.reads).toEqual(["board.example.com"]);
  expect(await controlled("/claims", { session, address: wrapped })).toEqual({ entry: "web/wrapped_pages", arguments: { url: wrapped } });
  expect(await controlled("/claims", { session, address: post })).toEqual({ entry: "web/board_posts", arguments: { page: post } });
  // An address on a claimed host that its tool does not read is no one's.
  expect(await controlled("/claims", { session, address: "https://board.example.com/b/notes" })).toEqual({ entry: null });

  const own = await called(port, token, "toolbook", "call", { tool: "web/wrapped_pages", arguments: { url: wrapped } });
  const claimed = await called(port, token, "web", "web_fetch", { url: wrapped });

  expect(own).toStartWith("ERROR: Private web address is not allowed: localhost");
  expect(claimed).toBe(own);
}, 600_000);
