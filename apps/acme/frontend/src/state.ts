import type { Capability } from "./types";

/// Pure helpers shared by the view and the modal, covered by `bun test`.

export function badgeClass(status: string): string {
  if (status === "valid") return "ok";
  if (status === "pending" || status === "ready" || status === "processing") {
    return "warn";
  }
  if (status === "invalid" || status === "revoked") return "bad";
  if (status === "issued-unstored") return "warn";
  return "off";
}

export function directoryLabel(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/// `a.test, www.a.test` or newline-separated names become a list. A wildcard is
/// kept whole; HTTP-01 refuses it later.
export function splitNames(value: string): string[] {
  return value
    .split(/[,\s]+/)
    .map((name) => name.trim())
    .filter(Boolean);
}

export function isStaging(capabilities: Capability[], url: string): boolean {
  return capabilities.some((capability) =>
    (capability["staging-directories"] ?? []).includes(url),
  );
}
