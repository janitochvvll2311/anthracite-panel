/**
 * What crosses the browser boundary.
 *
 * The counterpart of `backend/anth-wire`, and deliberately the same size of job:
 * the messages, the one error type, and one JSON exchange. It is free of panel
 * code — no formatting, no routes, no components — because a second consumer (a
 * plugin module, a test harness) must be able to use it without pulling in the
 * shell. That is also why it holds no `Request`/`Response` envelope: the socket
 * needs one because a connection has no verb or URL, and HTTP does not.
 */

export { type ApiErrorBody, type ErrorKind, errorKind, httpStatus } from "./error";
export { type JsonFetch, type JsonRequest, WireError, responseError, send } from "./exchange";
export {
  type StreamStatus,
  type WatchRequest,
  watch,
  watchPath,
} from "./stream";
export {
  type HostInfo,
  type LoadAverage,
  type MemoryInfo,
  type OsInfo,
} from "./message";
