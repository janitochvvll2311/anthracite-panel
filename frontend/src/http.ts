/** Turns a failed response into the message the panel shows. */
export interface ResponseError extends Error {
  code?: string
  status?: number
}

export async function responseError(response: Response): Promise<ResponseError> {
  let detail = `${response.status} ${response.statusText}`.trim()
  const error = new Error(detail) as ResponseError
  error.status = response.status
  try {
    const body = (await response.json()) as {
      error?: { message?: unknown; code?: unknown }
    }
    if (typeof body.error?.message === 'string') {
      detail = body.error.message
    }
    if (typeof body.error?.code === 'string') {
      error.code = body.error.code
    }
  } catch {
    // Keep the status text.
  }
  error.message = detail
  return error
}
