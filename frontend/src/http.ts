/** Turns a failed response into the message the panel shows. */
export async function responseError(response: Response): Promise<Error> {
  let detail = `${response.status} ${response.statusText}`.trim()
  try {
    const body = (await response.json()) as { error?: { message?: unknown } }
    if (typeof body.error?.message === 'string') detail = body.error.message
  } catch {
    // Keep the status text.
  }
  return new Error(detail)
}
