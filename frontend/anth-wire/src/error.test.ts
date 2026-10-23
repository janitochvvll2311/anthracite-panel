import { describe, expect, test } from "bun:test";

import { type ErrorKind, errorKind, httpStatus } from "./error";

const KINDS: readonly ErrorKind[] = [
  "not_found",
  "forbidden",
  "conflict",
  "invalid",
  "unsupported",
  "too_large",
  "internal",
];

describe("httpStatus", () => {
  test("matches the status the daemon maps each kind to", () => {
    expect(KINDS.map(httpStatus)).toEqual([404, 403, 409, 400, 501, 413, 500]);
  });
});

describe("errorKind", () => {
  /** The two directions must agree, or a client would classify a status the
   * daemon never produces for that kind. */
  test("is the inverse of httpStatus for every kind", () => {
    for (const kind of KINDS) {
      expect(errorKind(httpStatus(kind))).toBe(kind);
    }
  });

  test("leaves a status outside the mapping unclassified", () => {
    expect(errorKind(418)).toBeUndefined();
    expect(errorKind(502)).toBeUndefined();
  });
});
