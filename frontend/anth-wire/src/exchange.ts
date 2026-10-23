/**
 * One JSON exchange over `fetch`, and nothing above it.
 *
 * This is the counterpart of `anth_wire::frame`: the crate's job is to put a
 * message on a byte stream and take one back, and it knows nothing about what the
 * message means. Here the stream is `fetch`, so this module carries the media
 * type, the credentials and the two failure shapes — an empty body and a refused
 * request — and stops there. Routing belongs to whoever knows the route.
 */

import { type ApiErrorBody, errorKind } from "./error";

/**
 * The one call an exchange makes.
 *
 * Narrower than `typeof fetch` on purpose: Bun augments the global with a
 * `preconnect` method, so asking for the global type would force every test stub
 * to fake a method nothing here calls.
 */
export type JsonFetch = (
  path: string,
  init: RequestInit,
) => Promise<Response>;

export interface JsonRequest {
  method?: string;
  /** Already-built path, including any query string. */
  path: string;
  body?: unknown;
  fetchImpl?: JsonFetch;
}

/**
 * A failed exchange, carrying the daemon's message and, when the status maps to
 * one, its kind.
 *
 * The kind is derived from the status rather than parsed out of the body,
 * because the body documents only `error`. A status outside the mapping keeps
 * `undefined` instead of being guessed at.
 */
export class WireError extends Error {
  readonly status: number;
  readonly kind: ReturnType<typeof errorKind>;

  constructor(message: string, status: number) {
    super(message);
    this.name = "WireError";
    this.status = status;
    this.kind = errorKind(status);
  }
}

/**
 * The message from an error response, or its status line when the body is not
 * the documented shape.
 *
 * Never throws: reporting a failure must not become a second failure. v0.1.0 had
 * this function twice, in the panel and in each plugin family, and the copies
 * drifted in exactly this fallback.
 */
export async function responseError(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as Partial<ApiErrorBody>;
    if (typeof body.error === "string" && body.error.length > 0) {
      return body.error;
    }
  } catch {
    // A non-JSON body carries no detail worth a second error.
  }
  return `${response.status} ${response.statusText}`.trim();
}

/**
 * Sends one JSON request and returns the parsed reply.
 *
 * `204` yields `undefined`: a command that answers nothing is a success, not a
 * parse failure. Any non-2xx answer throws a [`WireError`], so a caller can
 * `catch` and show the daemon's own wording without inspecting a status first.
 */
export async function send<T>(
  request: JsonRequest,
  defaultFetch: JsonFetch = (path, init) => globalThis.fetch(path, init),
): Promise<T> {
  const hasBody = request.body !== undefined;
  const response = await (request.fetchImpl ?? defaultFetch)(request.path, {
    method: (request.method ?? "GET").toUpperCase(),
    credentials: "same-origin",
    headers: {
      accept: "application/json",
      ...(hasBody ? { "content-type": "application/json" } : {}),
    },
    ...(hasBody ? { body: JSON.stringify(request.body) } : {}),
  });

  if (!response.ok) {
    throw new WireError(await responseError(response), response.status);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}
