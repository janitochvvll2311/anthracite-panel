/**
 * Registering the custom element.
 *
 * The shell imports a plugin module with `import()`, then creates the element.
 * That means registration is a side effect of the import, and the same module
 * may be imported twice in one realm — a re-mounted slot, a second build of the
 * bundle — where a second `customElements.define` throws and the widget vanishes
 * from the panel. Registration therefore has to be idempotent.
 */

/**
 * Registers `tag` once per realm.
 *
 * Returns `false` when the tag was already defined, which is not an error: it is
 * the re-import case. A tag defined by *something else* is a real conflict and
 * throws, because silently mounting the wrong element is worse than a loud
 * failure the console names.
 */
export function registerWidget(tag: string, element: CustomElementConstructor): boolean {
  if (!tag.includes("-")) {
    throw new Error(`"${tag}" is not a valid custom element name`);
  }
  const existing = customElements.get(tag);
  if (existing === undefined) {
    customElements.define(tag, element);
    return true;
  }
  if (existing === element) {
    return false;
  }
  throw new Error(
    `"${tag}" is already registered by another plugin; each plugin needs its own tag`,
  );
}

/**
 * Tells the shell which anchors share this module.
 *
 * A module may serve a dashboard widget and a full page. The shell sets
 * `data-slot` on the element, so one class can present differently without a
 * second file. This helper reads it back with a default, so a widget opened
 * outside a slot still renders.
 */
export function currentSlot(element: HTMLElement, fallback = ""): string {
  return element.dataset.slot ?? fallback;
}
