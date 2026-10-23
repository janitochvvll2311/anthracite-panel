import { describe, expect, test } from "bun:test";

import { CommandError, parseCommand, split } from "./repl-command";

describe("split", () => {
  test("splits on runs of whitespace", () => {
    expect(split("  host-info   ")).toEqual(["host-info"]);
  });

  test("a quoted value stays one word", () => {
    expect(split('plugin call org.anth.hello greet --name "Ana Sol"')).toEqual([
      "plugin",
      "call",
      "org.anth.hello",
      "greet",
      "--name",
      "Ana Sol",
    ]);
  });

  test("an empty line produces no words", () => {
    expect(split("   ")).toEqual([]);
  });

  // The backend's split keeps a quoted empty string as one empty word, which is
  // a command with one empty argument rather than no command.
  test("a quoted empty string is a word", () => {
    expect(split('""')).toEqual([""]);
  });
});

describe("parseCommand", () => {
  test("an empty line is empty, not an error", () => {
    expect(parseCommand("   ")).toEqual({ kind: "empty" });
  });

  test("host-info needs no arguments", () => {
    expect(parseCommand("host-info")).toEqual({ kind: "host-info" });
  });

  test("host-info refuses an argument", () => {
    expect(() => parseCommand("host-info extra")).toThrow(CommandError);
  });

  test("plugin list resolves", () => {
    expect(parseCommand("plugin list")).toEqual({ kind: "plugin-list" });
  });

  test("plugin alone names what it needs", () => {
    expect(() => parseCommand("plugin")).toThrow(/list.*call/);
  });

  test("an unknown plugin action is refused", () => {
    expect(() => parseCommand("plugin frobnicate")).toThrow(/unknown plugin/);
  });

  test("plugin call needs an id", () => {
    expect(() => parseCommand("plugin call")).toThrow(/needs a plugin id/);
  });

  // A caller typing a line stands where the plugin's own CLI stands, so that is
  // the pipe it gets without asking. This is the default the Rust CLI learned
  // the same way.
  test("plugin call defaults to the cli pipe", () => {
    expect(parseCommand("plugin call org.anth.hello greet")).toEqual({
      kind: "plugin-call",
      id: "org.anth.hello",
      line: "greet",
      pipe: "cli",
    });
  });

  test("--http selects the http pipe", () => {
    expect(parseCommand("plugin call org.anth.hello {} --http")).toEqual({
      kind: "plugin-call",
      id: "org.anth.hello",
      line: "{}",
      pipe: "http",
    });
  });

  // A `--direct` payload is JSON, so it carries double quotes; single quotes are
  // how a user types one. The quotes must reach the daemon, or the payload is
  // no longer JSON.
  test("--direct keeps the JSON payload intact", () => {
    expect(parseCommand(`plugin call org.anth.hello '{"action":"x"}' --direct`)).toEqual({
      kind: "plugin-call",
      id: "org.anth.hello",
      line: '{"action":"x"}',
      pipe: "direct",
    });
  });

  test("a single-quoted value with a space stays one word", () => {
    expect(parseCommand("plugin call org.anth.hello greet --name 'Ana Sol'")).toEqual({
      kind: "plugin-call",
      id: "org.anth.hello",
      line: "greet --name Ana Sol",
      pipe: "cli",
    });
  });

  // A flag in the middle must not change what the line is: the remaining words
  // are the command line, in the order they were typed.
  test("a flag in the middle is pulled out and the line keeps its order", () => {
    expect(parseCommand("plugin call org.anth.hello greet --http now")).toEqual({
      kind: "plugin-call",
      id: "org.anth.hello",
      line: "greet now",
      pipe: "http",
    });
  });

  test("quit and exit both leave", () => {
    expect(parseCommand("quit")).toEqual({ kind: "quit" });
    expect(parseCommand("exit")).toEqual({ kind: "quit" });
  });

  test("an unknown command names itself", () => {
    expect(() => parseCommand("nonsense")).toThrow(/unknown command `nonsense`/);
  });
});
