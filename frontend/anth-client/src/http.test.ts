import { expect, test } from 'vitest'

import { parseErrorBody, responseError } from './http'

test('parses the API error envelope and ignores malformed bodies', () => {
  expect(
    parseErrorBody('{"error":{"message":"nope","code":"denied"}}'),
  ).toEqual({ message: 'nope', code: 'denied' })
  expect(parseErrorBody('{"error":{}}')).toEqual({
    message: undefined,
    code: undefined,
  })
  expect(parseErrorBody('not json')).toEqual({})
  expect(parseErrorBody('')).toEqual({})
})

test('builds a ResponseError with the API message and code', async () => {
  const response = new Response(
    JSON.stringify({ error: { message: 'nope', code: 'denied' } }),
    { status: 403, statusText: 'Forbidden' },
  )

  const error = await responseError(response)

  expect(error).toMatchObject({ message: 'nope', code: 'denied', status: 403 })
})

test('falls back to the HTTP status when the body is empty', async () => {
  const error = await responseError(
    new Response('', { status: 500, statusText: 'Server Error' }),
  )

  expect(error.message).toBe('500 Server Error')
  expect(error.kind).toBe('internal')
  expect(error.code).toBe('internal')
  expect(error.status).toBe(500)
})
