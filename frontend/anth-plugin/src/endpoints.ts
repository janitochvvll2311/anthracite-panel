/**
 * Talking to a plugin's own endpoints.
 *
 * A plugin's web module runs on the panel's origin and calls back through
 * `/api/plugins/<id><path>`. Each plugin used to reimplement that — the base URL,
 * the credentials, the error body — and the copies drifted. This class is that
 * contract, and it is configurable rather than inherited: a Lit component, a
 * plain custom element and a one-off script all build one the same way.
 *
 * Two rules the host depends on, which this class enforces rather than
 * documents:
 *
 * - **Only relative paths.** A module runs with the operator's session, so a
 *   request to another origin would carry nothing useful and would tempt a
 *   plugin into shipping its own credentials. An absolute path is rejected
 *   instead of silently prefixed.
 * - **The id may be an alias.** The daemon resolves either, and a widget built
 *   by hand often knows the short one.
 */

import { responseError } from "@anth/wire";

/** Where a plugin's endpoints are served, relative to the panel's origin. */
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
  /** The short handle the bundle declared, if any. Accepted wherever `id` is. */
  alias?: string;
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
 * Reads an error from a response without throwing.
 *
 * Taken from `@anth/wire` rather than reimplemented: a guest and the daemon
 * answer the same `{"error": "..."}` shape, so there is one reader for both and
 * one fallback to get right. v0.1.0 had this function twice and the copies
 * drifted. It is imported *and* re-exported, because the class below reads it
 * too: `export ... from` would leave no local binding.
 */
export { responseError as anchorError };

/** Everything a plugin may configure about its own transport. */
export interface PluginEndpointsOptions {
  /** Plugin id or its alias, used to build the endpoint prefix. */
  pluginId: string;
  /**
   * Origin the panel is served from. Defaults to the current origin, which is
   * what a module loaded by the shell is running on.
   */
  origin?: string;
  /** Extra headers sent with every request, merged over the defaults. */
  headers?: Record<string, string>;
  /** Replaced in tests; defaults to the ambient `fetch`. */
  fetchImpl?: typeof fetch;
}

/** Per-call overrides, for a request that is not like the others. */
export interface RequestOptions {
  headers?: Record<string, string>;
  /** Lets a caller abandon a request, e.g. when a widget is torn down. */
  signal?: AbortSignal;
}

/**
 * Calls one plugin's endpoints.
 *
 * The base URL is derived, never configured by hand: a plugin that hardcodes a
 * path works in one deployment and breaks in the next.
 */
export class PluginEndpoints {
  readonly #base: string;
  readonly #origin: string;
  readonly #headers: Record<string, string>;
  readonly #fetch: typeof fetch;

  constructor(options: PluginEndpointsOptions) {
    if (!options.pluginId) {
      throw new Error("PluginEndpoints needs a plugin id or alias");
    }
    this.#origin = options.origin ?? globalThis.location?.origin ?? "";
    this.#base = `${this.#origin}${ANCHOR_PREFIX}/${encodeURIComponent(options.pluginId)}`;
    this.#headers = { ...options.headers };
    this.#fetch = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
  }

  /** The same transport for another plugin, keeping the origin and headers. */
  forPlugin(pluginId: string): PluginEndpoints {
    return new PluginEndpoints({
      pluginId,
      origin: this.#origin,
      headers: this.#headers,
      fetchImpl: this.#fetch,
    });
  }

  /** The URL one endpoint path is served from. */
  url(path: string, query?: Record<string, string>): string {
    if (/^[a-z]+:/i.test(path) || path.startsWith("//")) {
      throw new Error(
        `an endpoint path must be relative, got "${path}": a plugin talks to its own origin`,
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
  async get<T>(path: string, query?: Record<string, string>, options?: RequestOptions): Promise<T> {
    const response = await this.#fetch(this.url(path, query), {
      credentials: "same-origin",
      headers: this.#merge(options),
      ...(options?.signal ? { signal: options.signal } : {}),
    });
    return this.#json<T>(response);
  }

  /**
   * `<method> <path>` with a JSON body.
   *
   * The body is serialized here so a plugin cannot forget the content type, and
   * the caller gets the parsed reply rather than a `Response` it has to unwrap.
   */
  async send<T>(
    method: string,
    path: string,
    body?: unknown,
    options?: RequestOptions,
  ): Promise<T> {
    const response = await this.#fetch(this.url(path), {
      method: method.toUpperCase(),
      credentials: "same-origin",
      headers: this.#merge(options, { "content-type": "application/json" }),
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      ...(options?.signal ? { signal: options.signal } : {}),
    });
    return this.#json<T>(response);
  }

  /** `DELETE <path>`, the shape a remove call almost always has. */
  delete<T>(path: string, options?: RequestOptions): Promise<T> {
    return this.send<T>("DELETE", path, undefined, options);
  }

  #merge(options?: RequestOptions, extra: Record<string, string> = {}): Record<string, string> {
    return {
      accept: "application/json",
      ...this.#headers,
      ...extra,
      ...options?.headers,
    };
  }

  async #json<T>(response: Response): Promise<T> {
    if (!response.ok) {
      throw new Error(await responseError(response));
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  }
}
