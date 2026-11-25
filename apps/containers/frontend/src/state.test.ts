import { expect, test } from "bun:test";

import {
  formatBytes,
  matches,
  parseCommand,
  parseEnv,
  parseLabels,
  parseVolumes,
  percent,
  shortId,
  stateKind,
} from "./state";
import type { Container } from "./types";

const container: Container = {
  id: "0123456789abcdef",
  name: "web",
  image: "nginx:1.27",
  state: "running",
  status: "Up",
  health: null,
  "created-at": 10,
  ports: [],
  labels: [],
};

test("states map to the panel badge tones", () => {
  expect(stateKind("running")).toBe("ok");
  expect(stateKind("paused")).toBe("warn");
  expect(stateKind("exited")).toBe("bad");
  expect(stateKind("created")).toBe("off");
});

test("ids are shortened and bytes are human", () => {
  expect(shortId(container.id)).toBe("0123456789ab");
  expect(shortId("short")).toBe("short");
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(1536)).toBe("1.5 KiB");
  expect(percent(12.34)).toBe("12.3%");
});

test("a command keeps quoted arguments together", () => {
  expect(parseCommand("nginx -g 'daemon off;'")).toEqual([
    "nginx",
    "-g",
    "daemon off;",
  ]);
  expect(parseCommand('sh -c "echo hello world"')).toEqual([
    "sh",
    "-c",
    "echo hello world",
  ]);
  expect(parseCommand("  ")).toEqual([]);
  expect(() => parseCommand("'open")).toThrow();
});

test("env lines keep a value that contains equals", () => {
  expect(parseEnv("A=1\nB=x=y")).toEqual([
    { name: "A", value: "1" },
    { name: "B", value: "x=y" },
  ]);
  expect(() => parseEnv("NO_EQUALS")).toThrow();
});

test("volume lines reject a bind-like source", () => {
  expect(parseVolumes("site:/usr/share/nginx/html")).toEqual([
    { name: "site", path: "/usr/share/nginx/html" },
  ]);
  expect(() => parseVolumes("/etc:/etc")).toThrow();
  expect(() => parseVolumes("site:relative")).toThrow();
});

test("labels need name=value", () => {
  expect(parseLabels("com.example=web")).toEqual([
    { name: "com.example", value: "web" },
  ]);
  expect(() => parseLabels("broken")).toThrow();
});

test("search matches name, image and id", () => {
  expect(matches(container, "web")).toBe(true);
  expect(matches(container, "NGINX")).toBe(true);
  expect(matches(container, "0123")).toBe(true);
  expect(matches(container, "postgres")).toBe(false);
});
