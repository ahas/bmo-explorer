import { connect } from "node:net";

// The executable of the board-posts stand-in, one call of the tool host's protocol 1 (the tool host's
// docs/api.md, 「The call protocol」): a claim of a post's address answered from the address alone, and a call
// read through the record's web helper on its socket (docs/site-contract.md, 「The web helper's socket」) — the
// research guard asked first, the post read by the `page` road, and a failed road handed back for the helper to
// answer as a failed page.

type Handed = {
  kind: string;
  address?: string;
  arguments?: { page?: string; offset?: number; max_chars?: number };
  shared?: Record<string, string>;
};

type Road = { Ok: string } | { Err: Record<string, unknown> };

// Whether an address is a post's, `/b/<board>/<id>` on the site's host or its `www.`.
function isPost(address: string): boolean {
  try {
    const url = new URL(address);
    const [b, board, id, ...rest] = url.pathname.split("/").filter((part) => part !== "");

    return (
      ["board.example.com", "www.board.example.com"].includes(url.hostname) &&
      b === "b" &&
      board !== undefined &&
      /^\d+$/.test(id ?? "") &&
      rest.length === 0
    );
  } catch {
    return false;
  }
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
  const page = handed.arguments?.page ?? "";

  if (socket === undefined) {
    return text("ERROR: the call was handed no web helper", true);
  }

  if (!isPost(page)) {
    return text(`ERROR: ${page} is not a board.example.com post`, true);
  }

  const refused = (await ask(socket, { kind: "opening", engine: false })) as string | null;

  if (refused !== null) {
    return text(refused);
  }

  const road = (await ask(socket, { kind: "page", url: page })) as Road;

  if ("Ok" in road) {
    const offset = handed.arguments?.offset ?? 0;

    return text(road.Ok.slice(offset, offset + (handed.arguments?.max_chars ?? road.Ok.length)));
  }

  return ask(socket, { kind: "ended", engine: false, outcome: road });
}

const handed = JSON.parse(await Bun.stdin.text()) as Handed;

if (handed.kind === "claim") {
  const address = handed.address ?? "";

  console.log(JSON.stringify({ arguments: isPost(address) ? { page: address } : null }));
} else {
  console.log(JSON.stringify(await call(handed)));
}
