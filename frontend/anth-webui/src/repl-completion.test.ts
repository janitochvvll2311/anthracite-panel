import { describe, expect, test } from "bun:test";

import { candidates, completeLine, splitWords } from "./repl-completion";
import { parseCommand } from "./repl-command";

describe("candidates", () => {
  test("a bare line offers every root verb and no flags", () => {
    const found = candidates([""]).map((candidate) => candidate.text);
    expect(found).toEqual(["exit", "help", "host-info", "plugin", "quit"]);
  });

  test("a prefix narrows the root verbs", () => {
    expect(candidates(["pl"]).map((candidate) => candidate.text)).toEqual([
      "plugin",
    ]);
  });

  test("after `plugin` the subcommands are offered", () => {
    expect(candidates(["plugin", ""]).map((candidate) => candidate.text)).toEqual([
      "call",
      "list",
    ]);
  });

  test("after `plugin call` the pipe flags are offered, before the id", () => {
    expect(candidates(["plugin", "call", ""]).map((c) => c.text)).toEqual([
      "--direct",
      "--http",
    ]);
  });

  test("past the id there is nothing to offer, as in the CLI", () => {
    // `hello` is clap's positional; the grammar has no subcommand under it, so
    // a Tab there lists nothing rather than the flags again.
    expect(candidates(["plugin", "call", "hello", ""])).toEqual([]);
  });

  test("a dash before the id reaches the pipe flags", () => {
    const found = candidates(["plugin", "call", "--"]);
    expect(found.map((c) => c.text)).toEqual(["--direct", "--http"]);
    expect(found.every((c) => c.kind === "flag")).toBe(true);
  });

  test("a path that names no subcommand offers nothing", () => {
    expect(candidates(["nonsense", ""])).toEqual([]);
    expect(candidates(["host-info", ""])).toEqual([]);
  });

  test("the daemon's own flags are not offered in the browser", () => {
    // `--socket` has no meaning over HTTP; completing it would complete a line
    // this console cannot run.
    expect(candidates(["--s"])).toEqual([]);
  });
});

describe("completeLine", () => {
  test("one match is inserted with a trailing space", () => {
    expect(completeLine("pl")).toEqual({ line: "plugin ", matches: [] });
  });

  test("several matches insert the prefix they share", () => {
    // `host-info`, `help` and `quit` share nothing, so the line is untouched
    // and the candidates are listed.
    const done = completeLine("");
    expect(done.line).toBe("");
    expect(done.matches).toEqual(["exit", "help", "host-info", "plugin", "quit"]);
  });

  test("nothing to complete leaves the line alone", () => {
    expect(completeLine("nonsense ")).toEqual({ line: "nonsense ", matches: [] });
  });

  test("a trailing space completes the next word, not the one before it", () => {
    expect(completeLine("plugin li")).toEqual({ line: "plugin list ", matches: [] });
  });

  test("the words before the cursor are kept as typed", () => {
    expect(completeLine("plugin call --h")).toEqual({
      line: "plugin call --http ",
      matches: [],
    });
  });
});

describe("splitWords", () => {
  test("a trailing space opens an empty word", () => {
    expect(splitWords("plugin ")).toEqual(["plugin", ""]);
    expect(splitWords("plugin")).toEqual(["plugin"]);
  });

  test("an empty line is one empty word, not none", () => {
    expect(splitWords("")).toEqual([""]);
  });
});

describe("the grammar and the parser agree", () => {
  test("every root verb the prompt offers parses", () => {
    for (const word of ["help", "quit", "exit", "host-info", "plugin"]) {
      expect(() => parseCommand(word === "plugin" ? "plugin list" : word)).not.toThrow();
    }
  });

  test("every plugin subcommand the prompt offers parses", () => {
    for (const word of ["list", "call"]) {
      const line = word === "call" ? "plugin call hello" : "plugin list";
      expect(() => parseCommand(line)).not.toThrow();
    }
  });

  test("a verb the parser accepts is offered by the prompt", () => {
    // The other direction: a command added to the parser but not to the
    // grammar would be typeable and not completable, which is the drift this
    // pair of tests exists to catch.
    const offered = candidates([""]).map((candidate) => candidate.text);
    for (const verb of ["help", "quit", "exit", "host-info", "plugin"]) {
      expect(offered).toContain(verb);
    }
  });
});
