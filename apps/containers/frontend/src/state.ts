import type { Container } from "./types";

/// Pure helpers for the container view and the create modal.

export function stateKind(state: string): string {
  if (state === "running") return "ok";
  if (state === "paused") return "warn";
  if (state === "restarting") return "warn";
  if (state === "exited" || state === "dead") return "bad";
  return "off";
}

export function shortId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) : id;
}

export function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function percent(value: number): string {
  return `${value.toFixed(1)}%`;
}

/// `KEY=value` lines; a value may contain `=`.
export function parseEnv(lines: string): Array<{ name: string; value: string }> {
  return lines
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const index = line.indexOf("=");
      if (index <= 0) throw new Error(`environment line \`${line}\` needs KEY=value`);
      return { name: line.slice(0, index), value: line.slice(index + 1) };
    });
}

/// `name:/container/path` lines. A source with `/` is a bind mount and is
/// refused here as well as by the service.
export function parseVolumes(
  lines: string,
): Array<{ name: string; path: string }> {
  return lines
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const index = line.indexOf(":");
      if (index <= 0) throw new Error(`volume line \`${line}\` needs name:/path`);
      const name = line.slice(0, index);
      const path = line.slice(index + 1);
      if (name.includes("/")) throw new Error(`bind mounts are not supported: ${name}`);
      if (!path.startsWith("/")) throw new Error(`\`${path}\` is not an absolute path`);
      return { name, path };
    });
}

export function parseLabels(
  lines: string,
): Array<{ name: string; value: string }> {
  return lines
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const index = line.indexOf("=");
      if (index <= 0) throw new Error(`label line \`${line}\` needs name=value`);
      return { name: line.slice(0, index), value: line.slice(index + 1) };
    });
}

export function matches(container: Container, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    container.name.toLowerCase().includes(needle) ||
    container.image.toLowerCase().includes(needle) ||
    container.id.toLowerCase().includes(needle)
  );
}
