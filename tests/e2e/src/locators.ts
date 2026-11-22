import type { Locator } from "@playwright/test";

/// Narrows a test-id locator with one of the node's identity attributes. The
/// test id says *what* the node is; the identity attribute says *which* app or
/// contribution it belongs to. No CSS class is ever part of a selector.
export function withAttr(
  base: Locator,
  name: string,
  value: string,
): Locator {
  return base.and(base.page().locator(`[${name}="${value}"]`));
}

/// Applies every defined identity attribute, in order.
export function scoped(
  base: Locator,
  attrs: Record<string, string | undefined>,
): Locator {
  let locator = base;
  for (const [name, value] of Object.entries(attrs)) {
    if (value !== undefined) locator = withAttr(locator, name, value);
  }
  return locator;
}
