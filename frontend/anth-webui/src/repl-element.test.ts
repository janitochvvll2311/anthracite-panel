import { beforeAll, describe, expect, test } from "bun:test";

import { AnthRepl } from "./repl-element";
import type { ReplBackend } from "./repl";

beforeAll(() => {
  if (!customElements.get("anth-repl")) {
    customElements.define("anth-repl", AnthRepl);
  }
});

const backend: ReplBackend = {
  hostInfo: async () => ({
    hostname: "box",
    os: { name: "Debian", version: "13", kernel: "6.1", arch: "x86_64" },
    uptime_seconds: 90,
    cpu_model: "Example",
    logical_cores: 4,
    load: { one: 0, five: 0, fifteen: 0 },
    memory: { total_kib: 1, available_kib: 1, swap_total_kib: 0, swap_free_kib: 0 },
  }),
  plugins: async () => [],
  call: async () => "hola",
};

function mount(): AnthRepl {
  AnthRepl.backend = backend;
  const element = document.createElement("anth-repl") as AnthRepl;
  document.body.append(element);
  return element;
}

describe("anth-repl", () => {
  test("mounting shows the help and one input", () => {
    const element = mount();
    const input = element.shadowRoot?.querySelector("input");

    expect(input).not.toBeNull();
    expect(element.lines[0].kind).toBe("help");
  });

  test("a submitted command appends the input and its answer", async () => {
    const element = mount();
    await element.submit("plugin call org.anth.hello greet");

    const kinds = element.lines.map((line) => line.kind);
    expect(kinds).toContain("input");
    expect(element.lines.some((line) => line.text === "hola")).toBe(true);
  });

  // A blank answer is a command that printed nothing; a blank transcript entry
  // would look like a failure.
  test("an empty answer is not appended", async () => {
    const element = mount();
    AnthRepl.backend = { ...backend, call: async () => null };
    await element.submit("plugin call org.anth.hello quiet");

    const outputs = element.lines.filter((line) => line.kind === "output");
    expect(outputs).toEqual([]);
  });

  // Rebuilding the shadow tree per command destroyed the input mid-line: focus
  // and any text already typed were lost. The input is now built once.
  test("the input survives a command", async () => {
    const element = mount();
    const before = element.shadowRoot?.querySelector("input");
    await element.submit("host-info");
    const after = element.shadowRoot?.querySelector("input");

    expect(after).toBe(before);
  });

  test("history recalls the previous line", async () => {
    const element = mount();
    await element.submit("host-info");
    await element.submit("plugin list");

    expect(element.previous()).toBe("plugin list");
    expect(element.previous()).toBe("host-info");
    expect(element.next()).toBe("plugin list");
  });

  // A plugin's output is data, so markup in it must not become markup.
  test("a reply containing markup is escaped", async () => {
    const element = mount();
    AnthRepl.backend = { ...backend, call: async () => "<img src=x onerror=alert(1)>" };
    await element.submit("plugin call org.anth.hello greet");

    const transcript = element.shadowRoot?.querySelector(".transcript");
    expect(transcript?.querySelector("img")).toBeNull();
    expect(transcript?.textContent).toContain("<img src=x");
  });

  test("a missing backend reports rather than throwing", async () => {
    const element = document.createElement("anth-repl") as AnthRepl;
    AnthRepl.backend = undefined;
    document.body.append(element);
    await element.submit("host-info");

    expect(element.lines.at(-1)).toEqual({
      kind: "error",
      text: "No backend was configured",
    });
  });
});
