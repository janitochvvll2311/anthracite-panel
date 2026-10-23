/**
 * The browser REPL: the same commands as the CLI prompt, over HTTP.
 *
 * The symmetry is the point. `backend/anth-panel/src/repl/mod.rs` types into the
 * socket; this types into `/api`, and both reach `Daemon::handle`. What differs
 * is only what has to: a browser cannot hold a socket, so the exchange goes
 * through `@anth/wire`'s `send` and the route names the command.
 *
 * The executor is separate from the element so a test can drive every command
 * against a fake transport and assert on the text that appears, without a daemon
 * and without a DOM.
 */

import type { HostInfo } from "@anth/wire";

import {
  type PluginCall,
  type PluginSummary,
  invokePlugin,
  listPlugins,
  readHostInfo,
} from "./repl-client";

import { parseCommand, type ReplCommand } from "./repl-command";

/** What the REPL needs from the panel, injected so a test never binds a port. */
export interface ReplBackend {
  hostInfo(): Promise<HostInfo>;
  plugins(): Promise<PluginSummary[]>;
  call(request: PluginCall): Promise<unknown>;
}

/** The real backend: three routes, no logic. */
export class HttpReplBackend implements ReplBackend {
  constructor(private readonly base = "") {}

  hostInfo(): Promise<HostInfo> {
    return readHostInfo(this.base);
  }

  plugins(): Promise<PluginSummary[]> {
    return listPlugins(this.base);
  }

  call(request: PluginCall): Promise<unknown> {
    return invokePlugin(this.base, request);
  }
}

/** One line of output, tagged so the element can style it. */
export interface ReplLine {
  kind: "input" | "output" | "error" | "help";
  text: string;
}

/** Runs one line and returns what to show. */
export async function runLine(
  backend: ReplBackend,
  line: string,
): Promise<ReplLine[]> {
  let command: ReplCommand;
  try {
    command = parseCommand(line);
  } catch (error) {
    return [{ kind: "error", text: message(error) }];
  }

  switch (command.kind) {
    case "empty":
      return [];
    case "quit":
      // The element owns leaving; the executor only says it was asked to.
      return [{ kind: "output", text: "" }];
    case "help":
      return [{ kind: "help", text: helpText() }];
    case "host-info":
      return run(backend.hostInfo().then(formatHostInfo));
    case "plugin-list":
      return run(backend.plugins().then(formatPluginList));
    case "plugin-call": {
      const { id, line: payload, pipe } = command;
      return run(backend.call({ id, pipe, payload }).then(formatReply));
    }
  }
}

/** Runs one request and turns a refusal into a line rather than a rejection. */
async function run(request: Promise<string>): Promise<ReplLine[]> {
  try {
    return [{ kind: "output", text: await request }];
  } catch (error) {
    return [{ kind: "error", text: message(error) }];
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The plugin list as text.
 *
 * A disabled plugin is listed with its reason, like the CLI does, because one
 * that vanished would look like it was never installed.
 */
export function formatPluginList(plugins: PluginSummary[]): string {
  if (plugins.length === 0) {
    return "no plugins installed";
  }

  const rows = plugins.map((plugin) => [
    plugin.id,
    plugin.version,
    plugin.disabled_reason === null ? "enabled" : "disabled",
    plugin.anchors.join(", "),
  ]);
  const header = ["ID", "VERSION", "STATE", "ANCHORS"];
  const widths = header.map((title, index) =>
    Math.max(title.length, ...rows.map((row) => row[index].length)),
  );

  const render = (cells: string[]) =>
    cells.map((cell, index) => cell.padEnd(widths[index])).join("  ").trimEnd();

  const lines = [render(header), ...rows.map(render)];
  const disabled = plugins.filter((plugin) => plugin.disabled_reason !== null);
  if (disabled.length > 0) {
    lines.push("");
    for (const plugin of disabled) {
      lines.push(`  ${plugin.id}: ${plugin.disabled_reason}`);
    }
  }
  return lines.join("\n");
}

/** A plugin's value as text: its string, or its JSON. */
export function formatReply(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (value === null || value === undefined) {
    return "";
  }
  return JSON.stringify(value, null, 2);
}

function formatHostInfo(info: HostInfo): string {
  return [
    `${info.hostname} (${info.os.name} ${info.os.version}, ${info.os.arch})`,
    `kernel ${info.os.kernel}`,
    `${info.logical_cores} cores, ${info.cpu_model}`,
    `load ${info.load.one} ${info.load.five} ${info.load.fifteen}`,
    `memory ${info.memory.total_kib} KiB, ${info.memory.available_kib} KiB available`,
  ].join("\n");
}

export function helpText(): string {
  return [
    "commands:",
    "  host-info                 one snapshot of the host",
    "  plugin list               every plugin the daemon found",
    "  plugin call <id> [LINE] [--http] [--direct]",
    "                            run one plugin over its own pipe",
    "  help                      this message",
    "  quit | exit               leave",
  ].join("\n");
}
