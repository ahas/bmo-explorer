import { connect } from "node:net";

// The executable of the wrapped-pages stand-in, one call of the tool host's protocol 1 (the tool host's
// docs/api.md, 「The call protocol」): a claim answered from the address alone, and a call read through the
// record's web helper on its socket (docs/site-contract.md, 「The web helper's socket」) — the research guard
// asked first, the wrapped page read by the `document` road, and a failed road handed back for the helper to
// answer as a failed page.

type Handed = {
  kind: string;
  address?: string;
  arguments?: { url?: string; offset?: number; max_chars?: number };
  shared?: Record<string, string>;
};

type Road = { Ok: string } | { Err: Record<string, unknown> };

const HOST = "wrap.example.com";

// The page an address on the wrapping host carries after its host, its scheme added where it was left out; null
// for an address of another host or one carrying nothing.
function wrapped(address: string): string | null {
  let url: URL;

  try {
    url = new URL(address);
  } catch {
    return null;
  }

  if (url.hostname !== HOST) {
    return null;
  }

  const rest = `${url.pathname.replace(/^\/+/, "")}${url.search}${url.hash}`;

  if (rest === "") {
    return null;
  }

  return /^https?:\/\//.test(rest) ? rest : `https://${rest}`;
}

// One request on the helper's socket, one line asked and one answered.
function ask(socket: string, asked: unknown): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const stream = connect(socket);
    let said = "";

    stream.setEncoding("utf8");
    stream.on("connect", () => stream.write(`${JSON.stringify(asked)}\n`));
    stream.on("data", (piece: string) => {
      said += piece;

      const end = said.indexOf("\n");

      if (end >= 0) {
        stream.end();
        resolve(JSON.parse(said.slice(0, end)));
      }
    });
    stream.on("error", reject);
  });
}

function text(said: string, isError = false): unknown {
  return { content: [{ type: "text", text: said }], isError };
}

async function call(handed: Handed): Promise<unknown> {
  const socket = handed.shared?.web;
  const given = handed.arguments?.url ?? "";
  const url = wrapped(given) ?? given;

  if (socket === undefined) {
    return text("ERROR: the call was handed no web helper", true);
  }

  const refused = (await ask(socket, { kind: "opening", engine: false })) as string | null;

  if (refused !== null) {
    return text(refused);
  }

  const slice = { offset: handed.arguments?.offset, max_chars: handed.arguments?.max_chars, layout: false };
  const road = (await ask(socket, { kind: "document", url, slice })) as Road;

  if ("Ok" in road) {
    return text(road.Ok);
  }

  return ask(socket, { kind: "ended", engine: false, outcome: road });
}

const handed = JSON.parse(await Bun.stdin.text()) as Handed;

if (handed.kind === "claim") {
  const url = wrapped(handed.address ?? "");

  console.log(JSON.stringify({ arguments: url === null ? null : { url: handed.address } }));
} else {
  console.log(JSON.stringify(await call(handed)));
}
