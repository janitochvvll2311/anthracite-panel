// Remote acid test for the generated TypeScript clients.
//
// `GreetingHttpClient` and `run` share the same injected transport; this one
// speaks HTTP over fetch with Basic auth, so the client is remote from the
// official panel.
//
// ANTH_PANEL_URL=http://127.0.0.1:8090 \
// ANTH_PANEL_AUTH=operator:anthracite-dev \
// bun sdk/ts/client/examples/remote.ts

import { run } from "../src/cli";
import { GreetingHttpClient } from "../src/http";
import type { Transport } from "../src/transport";

const base = (process.env.ANTH_PANEL_URL ?? "http://127.0.0.1:8090").replace(
  /\/$/,
  "",
);
const credentials =
  process.env.ANTH_PANEL_AUTH ?? "operator:anthracite-dev";
const authorization = `Basic ${btoa(credentials)}`;

const request: Transport = async (method, path, body) => {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: {
      authorization,
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
console.log("http:", await http.greet("Ada"));

const cli = await run(request, "greet", ["--name", "Ada"]);
console.log("cli:", cli);
