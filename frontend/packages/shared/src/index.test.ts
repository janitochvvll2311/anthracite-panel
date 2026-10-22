import { describe, expect, test } from "bun:test";

import { formatKib, formatUptime, responseError } from "./index";

describe("responseError", () => {
  test("uses the documented error field", async () => {
    const response = new Response(JSON.stringify({ error: "control is off" }), { status: 403 });
    expect(await responseError(response)).toBe("control is off");
  });

  test("falls back to the status line for a non-JSON body", async () => {
    const response = new Response("<html>bad gateway</html>", {
      status: 502,
      statusText: "Bad Gateway",
    });
    expect(await responseError(response)).toBe("502 Bad Gateway");
  });

  test("ignores an empty error field rather than reporting nothing", async () => {
    const response = new Response(JSON.stringify({ error: "" }), {
      status: 500,
      statusText: "Internal Server Error",
    });
    expect(await responseError(response)).toBe("500 Internal Server Error");
  });
});

describe("formatKib", () => {
  test("scales to the largest unit that fits", () => {
    expect(formatKib(512)).toBe("512 KiB");
    expect(formatKib(2048)).toBe("2.00 MiB");
    expect(formatKib(1024 * 1024 * 3)).toBe("3.00 GiB");
  });

  test("keeps one decimal once the value reaches ten", () => {
    expect(formatKib(1024 * 42)).toBe("42.0 MiB");
  });
});

describe("formatUptime", () => {
  test("drops the smaller units as the span grows", () => {
    expect(formatUptime(30)).toBe("30s");
    expect(formatUptime(90)).toBe("1m 30s");
    expect(formatUptime(3_600 * 4 + 60 * 12)).toBe("4h 12m");
    expect(formatUptime(86_400 * 3 + 3_600 * 4)).toBe("3d 4h");
  });
});
