/** Turns a failed response into the message the panel shows. */
export interface ResponseError extends Error {
  code?: string
  status?: number
}

/** The `error` object an API response carries, when it has one. */
export interface ErrorBody {
  message?: string
  code?: string
}

/** Parses an `{ "error": { "message", "code" } }` body without throwing. */
export function parseErrorBody(raw: string): ErrorBody {
  try {
    const body = JSON.parse(raw) as {
      error?: { message?: unknown; code?: unknown }
    }
    return {
      message:
        typeof body.error?.message === 'string' ? body.error.message : undefined,
      code: typeof body.error?.code === 'string' ? body.error.code : undefined,
    }
  } catch {
    return {}
  }
}

export async function responseError(response: Response): Promise<ResponseError> {
  const detail = `${response.status} ${response.statusText}`.trim()
  const error = new Error(detail) as ResponseError
  error.status = response.status
  let raw = ''
  try {
    raw = await response.text()
  } catch {
    // Keep the status text.
  }
  const body = parseErrorBody(raw)
  if (body.message) {
    error.message = body.message
  }
  if (body.code) {
    error.code = body.code
  }
  return error
}
