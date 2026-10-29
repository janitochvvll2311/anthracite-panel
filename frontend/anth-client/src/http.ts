/**
 * One HTTP transport shared by every client, mirroring `anth_client`'s
 * `Endpoint` plus the JSON helpers.
 *
 * The browser default is same-origin (`baseUrl: ''`), which is all the panel
 * needs. A custom integration can point at another origin and add a bearer
 * token or extra headers without reimplementing a single route.
 */

import type { ErrorKind } from 'anth-wire'

/** The `error` object an API response carries, when it has one. */
export interface ErrorBody {
  kind?: ErrorKind
  code?: string
  message?: string
}

/** A failed request, with the same fields as `anth_wire::WireError`. */
export class WireError extends Error {
  readonly kind: ErrorKind
  readonly code: string
  readonly status?: number

  constructor(
    kind: ErrorKind,
    code: string,
    message: string,
    status?: number,
  ) {
    super(message)
    this.name = 'WireError'
    this.kind = kind
    this.code = code
    this.status = status
  }
}

/** Kept for callers that only care that a response failed. */
export type ResponseError = WireError

/** Parses an `{ "error": { "kind", "code", "message" } }` body. */
export function parseErrorBody(raw: string): ErrorBody {
  try {
    const body = JSON.parse(raw) as {
      error?: { kind?: unknown; code?: unknown; message?: unknown }
    }
    return {
      kind:
        typeof body.error?.kind === 'string'
          ? (body.error.kind as ErrorKind)
          : undefined,
      code:
        typeof body.error?.code === 'string' ? body.error.code : undefined,
      message:
        typeof body.error?.message === 'string'
          ? body.error.message
          : undefined,
    }
  } catch {
    return {}
  }
}

const KIND_BY_STATUS: Record<number, ErrorKind> = {
  400: 'invalid',
  401: 'unauthorized',
  403: 'forbidden',
  404: 'not_found',
  409: 'conflict',
  413: 'too_large',
  429: 'too_many',
  501: 'unsupported',
}

/** Turns a failed response into the daemon's error shape. */
export async function responseError(response: Response): Promise<WireError> {
  const fallback = `${response.status} ${response.statusText}`.trim()
  let raw = ''
  try {
    raw = await response.text()
  } catch {
    // Keep the status text.
  }
  const body = parseErrorBody(raw)
  const kind = body.kind ?? KIND_BY_STATUS[response.status] ?? 'internal'
  return new WireError(
    kind,
    body.code ?? kind,
    body.message ?? fallback,
    response.status,
  )
}

export interface HttpTransportOptions {
  /** Base address, e.g. `https://panel.example`. Empty is same-origin. */
  baseUrl?: string
  /** Sent as `Authorization: Bearer <token>` on every request. */
  token?: string
  /** Extra headers for every request. */
  headers?: Record<string, string>
  /** A fetch implementation; defaults to the global one. */
  fetch?: typeof fetch
}

/** Shared connection settings and JSON helpers. */
export class HttpTransport {
  readonly baseUrl: string
  private readonly token?: string
  private readonly defaultHeaders: Record<string, string>
  private readonly fetchImpl: typeof fetch

  constructor(options: HttpTransportOptions = {}) {
    this.baseUrl = (options.baseUrl ?? '').replace(/\/+$/, '')
    this.token = options.token
    this.defaultHeaders = options.headers ?? {}
    // A browser rejects `obj.fetch(...)`; bind the global so a transport can
    // never be the receiver.
    this.fetchImpl = (options.fetch ?? fetch).bind(globalThis)
  }

  /** Appends a route to the base address. */
  url(path: string): string {
    return `${this.baseUrl}${path}`
  }

  /** A copy with a bearer token. */
  withToken(token: string): HttpTransport {
    return new HttpTransport({
      baseUrl: this.baseUrl,
      token,
      headers: this.defaultHeaders,
      fetch: this.fetchImpl,
    })
  }

  /** Sends one request without decoding the body or mapping its status. */
  async request(
    method: string,
    path: string,
    body?: unknown,
    init: RequestInit = {},
  ): Promise<Response> {
    const headers = new Headers(this.defaultHeaders)
    if (this.token) {
      headers.set('authorization', `Bearer ${this.token}`)
    }
    let payload: BodyInit | undefined
    if (body !== undefined && body !== null) {
      if (isBodyInit(body)) {
        payload = body
      } else {
        headers.set('content-type', 'application/json')
        payload = JSON.stringify(body)
      }
    }
    for (const [name, value] of new Headers(init.headers)) {
      headers.set(name, value)
    }
    return this.fetchImpl(this.url(path), {
      ...init,
      method,
      headers,
      body: payload,
    })
  }

  /** Sends one request and decodes a JSON answer. */
  async json<T>(method: string, path: string, body?: unknown): Promise<T> {
    const response = await this.request(method, path, body)
    if (!response.ok) {
      throw await responseError(response)
    }
    return (await response.json()) as T
  }

  /** Sends one request that has no value to return. */
  async call(method: string, path: string, body?: unknown): Promise<void> {
    const response = await this.request(method, path, body)
    if (!response.ok) {
      throw await responseError(response)
    }
  }

  /** Streams a binary answer. */
  async blob(path: string): Promise<Blob> {
    const response = await this.request('GET', path)
    if (!response.ok) {
      throw await responseError(response)
    }
    return response.blob()
  }
}

/** Same-origin transport used when a caller does not configure one. */
export const defaultTransport = new HttpTransport()

/** Accepts a configured transport or a bare fetch for tests and embedding. */
export function asTransport(
  value: HttpTransport | typeof fetch = defaultTransport,
): HttpTransport {
  return value instanceof HttpTransport
    ? value
    : new HttpTransport({ fetch: value })
}

function isBodyInit(value: unknown): value is BodyInit {
  return (
    typeof value === 'string' ||
    (typeof Blob !== 'undefined' && value instanceof Blob) ||
    (typeof FormData !== 'undefined' && value instanceof FormData) ||
    (typeof URLSearchParams !== 'undefined' &&
      value instanceof URLSearchParams) ||
    value instanceof ArrayBuffer ||
    ArrayBuffer.isView(value)
  )
}
