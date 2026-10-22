/** Turns a non-OK API response into an `Error` with the server's detail. */
export async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()

  try {
    const body = (await response.json()) as { error?: unknown }
    if (typeof body.error === 'string') detail = body.error
  } catch {
    // The body was not JSON; keep the status text.
  }

  return new Error(detail)
}

export interface HttpClientOptions {
  baseUrl?: string
  fetchImpl?: typeof fetch
}

/**
 * A `fetch` that retries once when the access token has quietly expired.
 *
 * Every client talks to the same protected API, so a client that loses its
 * access token mid-session would otherwise fail every call until the silent
 * refresh timer fired. Retrying centrally keeps that recovery out of the
 * eleven clients and their call sites.
 *
 * Only `401` retries, and only if a refresh is registered: the retry has to be
 * able to change the outcome. A repeated 401 is left to the caller.
 */
export type ApiFetch = typeof fetch

/** What `withRetry` needs from the auth layer; `AuthClient` satisfies it. */
export interface RefreshCapable {
  refresh(): Promise<unknown>
}

/**
 * The fetch the panel registry should hand to a new client.
 *
 * Set once by the shell with a retrying wrapper; `undefined` (unit tests, a
 * harness that injects its own client) means "use the default".
 */
let apiFetch: typeof fetch | undefined

export function useApiFetch(fetchImpl: typeof fetch | undefined): void {
  apiFetch = fetchImpl
}

export function currentApiFetch(): typeof fetch | undefined {
  return apiFetch
}

export function withRetry(
  fetchImpl: typeof fetch,
  auth: RefreshCapable | undefined,
): ApiFetch {
  let pending: Promise<unknown> | undefined

  return async (input, init) => {
    const response = await fetchImpl(input, init)
    if (response.status !== 401 || !auth) return response

    try {
      // One shared refresh: a page with several panels would otherwise rotate
      // the same token concurrently and sign itself out.
      pending ??= auth.refresh().finally(() => {
        pending = undefined
      })
      await pending
    } catch {
      return response
    }

    return fetchImpl(input, init)
  }
}

export interface HttpStreamClientOptions extends HttpClientOptions {
  EventSourceImpl?: typeof EventSource
}
