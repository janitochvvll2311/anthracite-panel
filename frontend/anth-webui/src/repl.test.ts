import { describe, expect, test } from "bun:test";

import { type ReplBackend, formatPluginList, formatReply, runLine } from "./repl";
import type { PluginCall, PluginSummary } from "./repl-client";

const HOST: Awaited<ReturnType<ReplBackend["hostInfo"]>> = {
  hostname: "box",
  os: { name: "Debian", version: "13", kernel: "6.1", arch: "x86_64" },
  uptime_seconds: 90,
  cpu_model: "Example",
  logical_cores: 4,
  load: { one: 0.1, five: 0.2, fifteen: 0.3 },
  memory: { total_kib: 8000, available_kib: 4000, swap_total_kib: 0, swap_free_kib: 0 },
};

const PLUGINS: PluginSummary[] = [
  {
    id: "org.anth.hello",
    name: "Hello",
    version: "0.2.0",
    anchors: ["GET /greeting"],
    disabled_reason: null,
  },
  {
    id: "org.anth.broken",
    name: "Broken",
    version: "0.1.0",
    anchors: [],
    disabled_reason: "the module digest does not match",
  },
];

/** A backend that records what it was asked, so the route is assertable. */
function fake(overrides: Partial<ReplBackend> = {}): ReplBackend & { calls: PluginCall[] } {
  const calls: PluginCall[] = [];
  return {
    calls,
    hostInfo: async () => HOST,
    plugins: async () => PLUGINS,
    call: async (request) => {
      calls.push(request);
      return { message: "hola" };
    },
    ...overrides,
  };
}

describe("runLine", () => {
  test("an empty line prints nothing and touches no route", async () => {
    const backend = fake();
    expect(await runLine(backend, "  ")).toEqual([]);
    expect(backend.calls).toEqual([]);
  });

  test("host-info renders the snapshot", async () => {
    const lines = await runLine(fake(), "host-info");
    expect(lines[0].kind).toBe("output");
    expect(lines[0].text).toContain("box");
    expect(lines[0].text).toContain("4 cores");
  });

  test("plugin list renders a table with the disabled reason", async () => {
    const lines = await runLine(fake(), "plugin list");
    expect(lines[0].text).toContain("org.anth.hello");
    expect(lines[0].text).toContain("disabled");
    expect(lines[0].text).toContain("the module digest does not match");
  });

  test("a plugin call sends the id, the pipe and the line", async () => {
    const backend = fake();
    await runLine(backend, "plugin call org.anth.hello greet --name Ana");

    expect(backend.calls).toEqual([
      { id: "org.anth.hello", pipe: "cli", payload: "greet --name Ana" },
    ]);
  });

  // A refusal is a line in the transcript, not a rejected promise: the prompt
  // has to keep reading after it.
  test("a refused request becomes an error line", async () => {
    const backend = fake({
      plugins: async () => {
        throw new Error("no such route");
      },
    });

    const lines = await runLine(backend, "plugin list");

    expect(lines).toEqual([{ kind: "error", text: "no such route" }]);
  });

  test("a parse error is an error line, not a throw", async () => {
    const lines = await runLine(fake(), "frobnicate");

    expect(lines[0].kind).toBe("error");
    expect(lines[0].text).toContain("unknown command");
  });

  test("help names the commands the backend has", async () => {
    const lines = await runLine(fake(), "help");

    expect(lines[0].kind).toBe("help");
    expect(lines[0].text).toContain("plugin call");
    expect(lines[0].text).toContain("host-info");
  });
});

describe("formatting", () => {
  test("an empty plugin list says so", () => {
    expect(formatPluginList([])).toBe("no plugins installed");
  });

  test("a string reply is printed without quotes", () => {
    expect(formatReply("hola")).toBe("hola");
  });

  test("an object reply is indented JSON", () => {
    expect(formatReply({ message: "hola" })).toContain('"message": "hola"');
  });

  test("a null reply is blank, not the word null", () => {
    expect(formatReply(null)).toBe("");
  });
});
