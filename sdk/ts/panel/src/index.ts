// The panel contract, in TypeScript (§7.7). A panel declares the slots it
// implements; the engine supplies every contribution and serves the module.
// This package owns the vocabulary and the deterministic order, so a panel does
// not reimplement either.

export * from "./iframe";
export * from "./dom";

import type { App, Contribution } from "./model";
export type { App, Contribution, Slot } from "./model";

export const VIEW = "panel.view";
export const ACTION = "panel.action";
export const SETTINGS = "panel.settings";
export const PORTABLE = [VIEW, ACTION, SETTINGS] as const;

/// Stable identity for a mounted pane or window.
export function contributionKey(contribution: Contribution): string {
  return `${contribution.app.id}#${contribution.slot.id}#${contribution.slot.tag}`;
}

/// Percent-encode one module segment, keeping `/` as the separator.
export function encodeModule(module: string): string {
  return module.split("/").map(encodeURIComponent).join("/");
}

/// The engine route for a module, with the hash as a cache-busting key: the
/// route is immutable between composition revisions, so a new hash is a new URL.
export function assetUrl(contribution: Contribution): string {
  const app = encodeURIComponent(contribution.app.id);
  const module = encodeModule(contribution.slot.module);
  return `/api/apps/${app}/assets/${module}?v=${contribution.slot.sha256.slice(0, 16)}`;
}

/// Code-unit order, not `localeCompare`: the layout must not depend on the
/// browser's locale, and the Rust SDK uses the same ordering.
function compare(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/// Every contribution to a slot, ordered by `order`, app id and tag. This is the
/// same tie-break as the Rust SDK, so two panels lay out the same surface the
/// same way.
export function ordered(apps: App[], slot: string): Contribution[] {
  return apps
    .flatMap((app) =>
      app.slots
        .filter((candidate) => candidate.id === slot)
        .map((candidate) => ({ app, slot: candidate })),
    )
    .sort(
      (left, right) =>
        left.slot.order - right.slot.order ||
        compare(left.app.id, right.app.id) ||
        compare(left.slot.tag, right.slot.tag),
    );
}

/// Contributions this panel cannot mount. A development build reports them;
/// the engine never learns which slots a panel implements.
export function ignored(apps: App[], implemented: readonly string[]): Contribution[] {
  return apps.flatMap((app) =>
    app.slots
      .filter((slot) => !implemented.includes(slot.id))
      .map((slot) => ({ app, slot })),
  );
}
