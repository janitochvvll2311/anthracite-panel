import type { Summary } from "./types";

export function channelKind(channel: string): string {
  if (channel === "stable") return "ok";
  if (channel === "beta") return "warn";
  if (channel === "nightly") return "off";
  return "off";
}

export function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function age(seconds: number, now = Date.now() / 1000): string {
  if (!seconds) return "never refreshed";
  const delta = Math.max(0, Math.floor(now - seconds));
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)} min ago`;
  if (delta < 86_400) return `${Math.floor(delta / 3600)} h ago`;
  return `${Math.floor(delta / 86_400)} d ago`;
}

export function initials(name: string): string {
  const parts = name
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2);
  if (parts.length === 0) return "?";
  return parts.map((part) => part[0]?.toUpperCase() ?? "").join("");
}

export function matches(summary: Summary, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    summary.id.toLowerCase().includes(needle) ||
    summary.name.toLowerCase().includes(needle) ||
    summary.summary.toLowerCase().includes(needle) ||
    summary.publisher.toLowerCase().includes(needle)
  );
}

export function shortDigest(digest: string): string {
  return digest.length > 12 ? `${digest.slice(0, 12)}…` : digest;
}
