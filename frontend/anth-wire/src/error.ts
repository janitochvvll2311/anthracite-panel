/**
 * One error type for the whole protocol, mirroring `anth_wire::ErrorKind`.
 *
 * v0.1.0 defined a separate error enum per feature and mapped each one by hand
 * to an HTTP status, then again to a CLI exit code, so the two mappings could
 * disagree. Here a failure names a kind and both mappings derive from it in one
 * place — the daemon's `ErrorKind`, and this, its browser counterpart.
 */

/**
 * Why a request failed, in the terms a caller can act on.
 *
 * The wire tags these `snake_case`, so the string union *is* the protocol value:
 * it is what arrives on the line, not a translation of a number.
 */
export type ErrorKind =
  | "not_found"
  | "forbidden"
  | "conflict"
  | "invalid"
  | "unsupported"
  | "too_large"
  | "internal";

/** HTTP status for a kind: the browser side of `ErrorKind::http_status`. */
export function httpStatus(kind: ErrorKind): number {
  switch (kind) {
    case "not_found":
      return 404;
    case "forbidden":
      return 403;
    case "conflict":
      return 409;
    case "invalid":
      return 400;
    case "unsupported":
      return 501;
    case "too_large":
      return 413;
    case "internal":
      return 500;
  }
}

/** The kind a status belongs to, or `undefined` for one outside the mapping. */
export function errorKind(status: number): ErrorKind | undefined {
  switch (status) {
    case 400:
      return "invalid";
    case 403:
      return "forbidden";
    case 404:
      return "not_found";
    case 409:
      return "conflict";
    case 413:
      return "too_large";
    case 500:
      return "internal";
    case 501:
      return "unsupported";
    default:
      return undefined;
  }
}

/**
 * The body of every failed exchange: `{"error": "..."}` and nothing else.
 *
 * The daemon deliberately sends no kind or code — it answered two shapes in
 * v0.1.0 and every client had to handle both — so the message is the detail and
 * the status carries the classification.
 */
export interface ApiErrorBody {
  error: string;
}
