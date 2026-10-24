/**
 * Turning a plugin's declared slot into a mounted element.
 *
 * A bundle cannot reach into the shell: it declares `[[ui]]` in its manifest,
 * the daemon lists that at `/api/plugins`, and the shell imports the module it
 * names. Two separate concerns live behind two interfaces so neither has to be
 * faked to test the other — this file is the import, `app.ts` is where the slots
 * go on the page.
 */

import { type PluginSummary, type UiSlot, listPlugins } from "./repl-client";

/** The slot a plugin mounts into to appear on the dashboard. */
export const DASHBOARD_SLOT = "dashboard.widget";

/** Where the shell finds out which slots exist. */
export interface PluginCatalog {
  list(): Promise<PluginSummary[]>;
}

/** The real catalog: one route, no logic. */
export class HttpPluginCatalog implements PluginCatalog {
  constructor(private readonly base = "") {}

  list(): Promise<PluginSummary[]> {
    return listPlugins(this.base);
  }
}

/** Imports a declared module and mounts the element it registers into `into`. */
export interface WidgetLoader {
  mount(plugin: PluginSummary, slot: UiSlot, into: HTMLElement): Promise<void>;
}

/** Everything the shell needs to fill its slots. */
export interface WidgetHost {
  readonly catalog: PluginCatalog;
  readonly loader: WidgetLoader;
}

/** The real host: the daemon's plugin list and the modules it serves. */
export class HttpWidgetHost implements WidgetHost {
  readonly catalog: PluginCatalog;
  readonly loader: WidgetLoader;

  constructor(base = "") {
    this.catalog = new HttpPluginCatalog(base);
    this.loader = new ModuleWidgetLoader(base);
  }
}

/**
 * The real loader: `import()` of the module the manifest declares.
 *
 * `base` is empty for the daemon that served the page, so the module arrives
 * from the same origin on `/p/<id>/<module>` — the route that serves only what
 * the manifest names. It is also the seam a test points at a directory of real
 * modules, which is why nothing here reaches for `location` or `fetch`.
 */
export class ModuleWidgetLoader implements WidgetLoader {
  constructor(private readonly base = "") {}

  async mount(plugin: PluginSummary, slot: UiSlot, into: HTMLElement): Promise<void> {
    const tag = tagName(slot.tag);
    const url = this.url(plugin.id, slot.module);
    try {
      await import(/* @vite-ignore */ url);
    } catch (error) {
      throw new Error(
        `plugin \`${plugin.id}\` could not load \`${slot.module}\`: ${detail(error)}`,
      );
    }

    if (customElements.get(tag) === undefined) {
      throw new Error(`\`${slot.module}\` did not register \`<${tag}>\``);
    }

    const element = document.createElement(tag);
    element.dataset.slot = slot.slot;
    into.append(element);
  }

  private url(id: string, module: string): string {
    const root = this.base.replace(/\/+$/, "");
    const path = module.split("/").map(encodeURIComponent).join("/");
    return `${root}/p/${encodeURIComponent(id)}/${path}`;
  }
}

/**
 * The element name a slot may mount.
 *
 * The name arrives from a manifest, so it is checked rather than handed to
 * `createElement`. A name without a hyphen is not a custom element and would
 * create a built-in one; uppercase cannot match, because `customElements` keys
 * are case-sensitive and a manifest is written by hand.
 */
export function tagName(tag: string): string {
  if (!/^[a-z][a-z0-9._]*-[a-z0-9._-]*$/.test(tag)) {
    throw new Error(`"${tag}" is not a valid custom element name`);
  }
  return tag;
}

/** Every slot `plugin` declares for `slot`, skipping one that did not load. */
export function slotsFor(plugin: PluginSummary, slot: string): UiSlot[] {
  if (plugin.disabled_reason !== null) {
    return [];
  }
  return (plugin.ui ?? []).filter((declared) => declared.slot === slot);
}

function detail(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
