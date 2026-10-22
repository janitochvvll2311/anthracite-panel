/**
 * The browser half of the plugin contract.
 *
 * A plugin's web module is imported by the shell, registers a custom element and
 * talks back to its own anchors. Each plugin used to reimplement that last part —
 * the base URL, the credentials, the error body — and the copies drifted. This
 * package is that contract, so a plugin writes behaviour.
 *
 * Two rules the host depends on, which this package enforces rather than
 * documents:
 *
 * - **The module registers an element; it does not export one to be mounted.**
 *   The shell imports the module and then does `document.createElement(tag)`,
 *   so registration has to be a side effect.
 * - **Only relative URLs.** A module runs on the panel's origin with the
 *   operator's session, so a request to another origin would carry nothing
 *   useful and would tempt a plugin into shipping its own credentials. The client
 *   rejects an absolute URL instead of silently prefixing it.
 */

/** Where a plugin's anchors are served, relative to the panel's origin. */
export const ANCHOR_PREFIX = "/api/plugins";

/** One anchor as the catalogue declares it. */
export interface PluginAnchor {
  slot: string;
  tag: string;
  module: string;
  /** Tab label for panel slots; the shell falls back to the plugin name. */
  label?: string;
}

/** What `/api/plugins` reports about one installed plugin. */
export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  capabilities: string[];
  api: { method: string; path: string }[];
  ui: PluginAnchor[];
}

export interface PluginCatalogue {
  enabled: boolean;
  install_from_web: boolean;
  plugins: PluginSummary[];
  problems: string[];
}

/**
 * The shape of every failed anchor.
 *
 * The daemon answers `{"error": "..."}` and a guest is told to do the same, so
 * one reader covers both.
 */
export interface AnchorErrorBody {
  error: string;
}

/**
 * Reads an error from a response without throwing.
 *
 * Reporting a failure must not itself fail: a non-JSON body yields the status
 * line. This mirrors the panel's `responseError`, which is why a plugin can rely
 * on the same wording in the same places.
 */
export async function anchorError(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as Partial<AnchorErrorBody>;
    if (typeof body.error === "string" && body.error.length > 0) {
      return body.error;
    }
  } catch {
    // A non-JSON body carries no detail worth a second error.
  }
  return `${response.status} ${response.statusText}`.trim();
}

export interface AnchorClientOptions {
  /** Plugin id, used to build the anchor prefix. */
  pluginId: string;
  /**
   * Origin the panel is served from. Defaults to the current origin, which is
   * what a module loaded by `anth-plugin-slot` is running on.
   */
  origin?: string;
  fetchImpl?: typeof fetch;
}

/**
 * Calls one plugin's anchors.
 *
 * The base URL is derived, never configured by hand: a plugin that hardcodes a
 * path works in one deployment and breaks in the next.
 */
export class AnchorClient {
  readonly #base: string;
  readonly #fetch: typeof fetch;

  constructor(options: AnchorClientOptions) {
    if (!options.pluginId) {
      throw new Error("an AnchorClient needs a plugin id");
    }
    const origin = options.origin ?? globalThis.location?.origin ?? "";
    this.#base = `${origin}${ANCHOR_PREFIX}/${encodeURIComponent(options.pluginId)}`;
    this.#fetch = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
  }

  /** The URL one anchor path is served from. */
  url(path: string, query?: Record<string, string>): string {
    if (/^[a-z]+:/i.test(path) || path.startsWith("//")) {
      throw new Error(
        `an anchor path must be relative, got "${path}": a plugin talks to its own origin`,
      );
    }
    const suffix = path.startsWith("/") ? path : `/${path}`;
    const search = query ? `?${new URLSearchParams(query)}` : "";
    return `${this.#base}${suffix}${search}`;
  }

  /**
   * `GET <path>` parsed as JSON.
   *
   * A non-2xx answer becomes a thrown `Error` whose message is the daemon's
   * detail, so a caller can `catch` and display it without inspecting a status.
   */
  async get<T>(path: string, query?: Record<string, string>): Promise<T> {
    const response = await this.#fetch(this.url(path, query), {
      credentials: "same-origin",
      headers: { accept: "application/json" },
    });
    return this.#json<T>(response);
  }

  /**
   * `<method> <path>` with a JSON body.
   *
   * The body is serialized here so a plugin cannot forget the content type, and
   * the caller gets the parsed reply rather than a `Response` it has to unwrap.
   */
  async send<T>(method: string, path: string, body?: unknown): Promise<T> {
    const response = await this.#fetch(this.url(path), {
      method: method.toUpperCase(),
      credentials: "same-origin",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    return this.#json<T>(response);
  }

  async #json<T>(response: Response): Promise<T> {
    if (!response.ok) {
      throw new Error(await anchorError(response));
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  }
}
