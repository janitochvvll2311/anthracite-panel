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

export interface HttpStreamClientOptions extends HttpClientOptions {
  EventSourceImpl?: typeof EventSource
}
