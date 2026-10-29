/**
 * The one error vocabulary, matching `anth_wire::WireError`.
 *
 * `kind` is what a caller branches on; `code` is the stable machine tag;
 * `message` is for a human and is never a branch condition.
 */
export type ErrorKind =
  | 'not_found'
  | 'forbidden'
  | 'unauthorized'
  | 'conflict'
  | 'invalid'
  | 'unsupported'
  | 'too_large'
  | 'too_many'
  | 'internal'

/** The `error` object the daemon sends on a failed request. */
export interface WireErrorBody {
  kind: ErrorKind
  code: string
  message: string
}
