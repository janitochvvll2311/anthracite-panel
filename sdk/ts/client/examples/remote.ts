// Remote acid test for the generated TypeScript clients.
//
// `GreetingHttpClient` and `run` share the same injected transport; this one
// speaks HTTP over fetch with the session cookie minted by the panel, so the
// client is remote from the official panel.
//
// ANTH_PANEL_URL=http://127.0.0.1:8090 \
// ANTH_PANEL_OPERATOR=operator \
// ANTH_PANEL_PASSWORD="$(sed -n 's/.*"password":"\([^"]*\)".*/\1/p' state/credentials.txt)" \
// bun sdk/ts/client/examples/remote.ts

import { run } from "../src/cli";
import { GreetingHttpClient } from "../src/http";
import type { Transport } from "../src/transport";

const base = (process.env.ANTH_PANEL_URL ?? "http://127.0.0.1:8090").replace(
  /\/$/,
  "",
);
const operator = process.env.ANTH_PANEL_OPERATOR ?? "operator";
const password = process.env.ANTH_PANEL_PASSWORD;
if (!password) {
  throw new Error(
    "ANTH_PANEL_PASSWORD is required (read it from state/credentials.txt)",
  );
}

async function login(): Promise<string> {
  const response = await fetch(`${base}/api/engine/session`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ operator, password }),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`login answered ${response.status}: ${text}`);
  }
  const header = response.headers.get("set-cookie");
  if (!header) throw new Error("the panel did not set a session cookie");
  return header.split(";")[0]!;
}

const cookie = await login();
const request: Transport = async (method, path, body) => {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: {
      cookie,
      ...(body ? { "content-type": "application/json" } : {}),
    },
    body,
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`the panel answered ${response.status}: ${text}`);
  }
  return text;
};

const http = new GreetingHttpClient(request);
const greeting = await http.greet("Ada");
check("http", greeting);
console.log("http:", greeting);

const cli = await run(request, "greet", ["--name", "Ada"]);
check("cli", cli);
console.log("cli:", cli);

// The acid test is only meaningful if the panel actually answered with the
// app's greeting, not with an empty body or an error envelope.
function check(layer: string, body: string): void {
  const expected = "hello, Ada";
  if (!body.includes(expected)) {
    throw new Error(`${layer}: expected \`${expected}\` in \`${body}\``);
  }
}
