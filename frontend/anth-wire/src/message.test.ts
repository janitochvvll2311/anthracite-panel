import { describe, expect, test } from "bun:test";

import type { HostInfo } from "./message";

/**
 * The payload the daemon actually sends. If the Rust struct gains a field, this
 * literal stops compiling and the two sides are fixed together instead of
 * drifting until a panel renders `undefined`.
 */
const SAMPLE: HostInfo = {
  hostname: "turing",
  os: { name: "Debian GNU/Linux", version: "13", kernel: "6.12.0", arch: "x86_64" },
  uptime_seconds: 90_000,
  cpu_model: "Intel Xeon",
  logical_cores: 8,
  load: { one: 1.5, five: 2.25, fifteen: 3 },
  memory: { total_kib: 8_388_608, available_kib: 6_291_456, swap_total_kib: 0, swap_free_kib: 0 },
};

describe("message", () => {
  test("round-trips the documented payload", () => {
    expect(JSON.parse(JSON.stringify(SAMPLE))).toEqual(SAMPLE);
  });

  /**
   * The rule this package exists to keep: HTTP bodies carry the arguments at the
   * top level. A `{"type":..., "data":...}` envelope here would be a shape no
   * route accepts, which is exactly the unreachable-contract failure.
   */
  test("carries no socket envelope", () => {
    const body = SAMPLE as unknown as Record<string, unknown>;
    expect(body).not.toHaveProperty("type");
    expect(body).not.toHaveProperty("data");
  });
});
