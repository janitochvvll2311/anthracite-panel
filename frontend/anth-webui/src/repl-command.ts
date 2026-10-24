/**
 * The REPL's command vocabulary, parsed without touching the network.
 *
 * This mirrors `backend/anth-panel/src/repl/mod.rs`, and mirrors it in the same
 * way `@anth/wire` mirrors `anth-wire`: the same commands, resolved to the same
 * routes. The reason it is a separate pure module is that a parser is the part
 * that goes wrong — quoting, empty lines, an unknown verb — and a test can check
 * all of that without a daemon.
 *
 * The backend splits a line and hands it to clap. There is no clap here, so the
 * grammar is small and explicit, and every command it accepts has a backend
 * counterpart. A verb the backend has and this does not is a gap, not a
 * feature: the goal is that a user cannot tell which side they are typing into.
 */

/** One command, resolved. */
export type ReplCommand =
  | { kind: "host-info" }
  | { kind: "plugin-list" }
  | { kind: "plugin-call"; id: string; line: string; pipe: "cli" | "http" | "direct" }
  | { kind: "help" }
  | { kind: "quit" }
  | { kind: "empty" };

/** A line that no command accepts, with the reason to show. */
export class CommandError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CommandError";
  }
}

/**
 * Parses one line into a command.
 *
 * Throws [`CommandError`] rather than returning a result type: the only caller
 * is the prompt, which reports the message and keeps reading, and a `Result`
 * here would be unwrapped on the next line every time.
 */
export function parseCommand(line: string): ReplCommand {
  const words = split(line);
  const [verb, ...rest] = words;

  if (verb === undefined) {
    return { kind: "empty" };
  }

  switch (verb) {
    case "help":
      return { kind: "help" };
    case "quit":
    case "exit":
      return { kind: "quit" };
    case "host-info":
      expectNoArguments("host-info", rest);
      return { kind: "host-info" };
    case "plugin":
      return parsePlugin(rest);
    default:
      throw new CommandError(
        `unknown command \`${verb}\`; try \`help\``,
      );
  }
}

function parsePlugin(words: string[]): ReplCommand {
  const [action, ...rest] = words;

  if (action === "list") {
    expectNoArguments("plugin list", rest);
    return { kind: "plugin-list" };
  }
  if (action !== "call") {
    throw new CommandError(
      action === undefined
        ? "`plugin` needs `list` or `call`"
        : `unknown plugin command \`${action}\`; try \`plugin list\``,
    );
  }

  // The flags can appear anywhere after `call`, so they are pulled out and the
  // remaining words are the line. Stripping them from the tail instead would
  // make the line depend on where they were typed.
  //
  // They are pulled out of the words *before* the id too: clap reads a flag
  // wherever it appears, so `plugin call --http hello` reaches the HTTP pipe in
  // the CLI. Taking the first word as the id would have sent `--http` as a
  // plugin id instead, which is a different command wearing the same text.
  //
  // `--` ends the panel's own flags, as it does in the CLI: everything after it
  // belongs to the plugin, so `plugin call hello -- greet --http` passes
  // `--http` through instead of standing in for the HTTP pipe.
  let pipe: "cli" | "http" | "direct" = "cli";
  const operands: string[] = [];
  let passthrough = false;
  for (const word of rest) {
    if (passthrough) {
      operands.push(word);
    } else if (word === "--") {
      passthrough = true;
    } else if (word === "--http") {
      pipe = "http";
    } else if (word === "--direct") {
      pipe = "direct";
    } else {
      operands.push(word);
    }
  }

  const [id, ...line] = operands;
  if (id === undefined) {
    throw new CommandError("`plugin call` needs a plugin id");
  }

  return { kind: "plugin-call", id, line: line.join(" "), pipe };
}

function expectNoArguments(command: string, rest: string[]): void {
  if (rest.length > 0) {
    throw new CommandError(`\`${command}\` takes no arguments`);
  }
}

/**
 * Splits a line the way the backend does, so a quoted value survives.
 *
 * The backend's rule, in `repl::split`: single quotes preserve everything
 * literally, double quotes group and honour `\"`, a backslash outside quotes
 * escapes the next character, and runs of whitespace separate. Single quotes are
 * the important case: a `--direct` payload is JSON and carries double quotes, so
 * a splitter that only understood `"` turned `'{"action":"x"}'` into
 * `{action:x}` — not JSON, and every direct call failed to parse.
 */
export function split(line: string): string[] {
  const words: string[] = [];
  let current = "";
  let mode: "plain" | "double" | "single" = "plain";
  let escaped = false;
  let started = false;

  for (const character of line) {
    if (escaped) {
      current += character;
      escaped = false;
      started = true;
      continue;
    }

    if (mode === "plain" && character === '"') {
      mode = "double";
      started = true;
    } else if (mode === "double" && character === '"') {
      mode = "plain";
    } else if (mode === "plain" && character === "'") {
      mode = "single";
      started = true;
    } else if (mode === "single" && character === "'") {
      mode = "plain";
    } else if (mode !== "single" && character === "\\") {
      escaped = true;
    } else if (mode === "plain" && /\s/.test(character)) {
      if (started) {
        words.push(current);
        current = "";
        started = false;
      }
    } else {
      current += character;
      started = true;
    }
  }
  // An escape at the very end has nothing to escape, but the empty word it would
  // have started is still a word: `greet \` is one argument.
  if (escaped || started) {
    words.push(current);
  }
  return words;
}
