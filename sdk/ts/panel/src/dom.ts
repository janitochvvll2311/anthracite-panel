// Shared DOM and frame-context helpers for panels. They are intentionally
// small: the SDK owns the contract, not the panel's layout.

import type { FrameContext } from "./iframe";
import type { Contribution } from "./model";
import { TESTID } from "./testids";

export function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`missing #${id}`);
  return element as T;
}

export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function button(
  className: string,
  text: string,
  title?: string,
): HTMLButtonElement {
  const node = element("button", className, text);
  node.type = "button";
  if (title) node.title = title;
  return node;
}

export function safeDispose(dispose: () => void): void {
  try {
    dispose();
  } catch (error) {
    console.error("disposing a module failed", error);
  }
}

export type ErrorCardOptions = {
  className?: string;
  title?: string;
  detailTag?: "p" | "span";
};

export function errorCard(
  error: unknown,
  {
    className = "error-card",
    title = "This module did not mount",
    detailTag = "p",
  }: ErrorCardOptions = {},
): HTMLElement {
  const card = element("div", className);
  card.dataset.testid = TESTID.errorCard;
  card.append(
    element("strong", "", title),
    element(detailTag, "", error instanceof Error ? error.message : String(error)),
  );
  return card;
}

/// The `ctx` a module receives. `panel` is the caller's declaration, so the
/// official and minimal panels differ only in what they pass here.
export function frameContext(
  contribution: Contribution,
  kind: FrameContext["kind"],
  panel: { slots: readonly string[]; version: string },
  params?: unknown,
): FrameContext {
  return {
    app: {
      id: contribution.app.id,
      version: contribution.app.version,
      risk: contribution.app.risk,
    },
    contribution: { ...contribution.slot, app: contribution.app.id },
    panel: { slots: [...panel.slots], version: panel.version },
    kind,
    ...(params === undefined ? {} : { params }),
  };
}
