import { describe, expect, test } from "bun:test";

import { type StreamStatus, watch, watchPath } from "./stream";

/**
 * A stand-in for the browser's `EventSource`.
 *
 * It is not a mock of the code under test: `watch` drives a real instance of the
 * DOM interface's shape, so what the test observes is what the panel would do
 * against a browser — the URL that was opened, the event name registered, the
 * order of the status transitions. `close` is recorded because a subscription
 * that forgets to close is the bug this exists to catch.
 */
class FakeEventSource {
  static opened: FakeEventSource[] = [];
  // The spec's lifecycle constants, which live on the constructor as well as on
  // an instance. `watch` reads them from whichever constructor it was given.
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;

  readonly url: string;
  closed = false;
  /**
   * The browser's own lifecycle value, which `watch` has to read.
   *
   * `CONNECTING` after an `error` means the browser will try again; `CLOSED`
   * means it has given up, which happens for a non-200 reply such as the `404`
   * an older daemon answers for a route it does not have.
   */
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  #listeners = new Map<string, ((event: MessageEvent<string>) => void)[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.opened.push(this);
  }

  addEventListener(name: string, listener: (event: MessageEvent<string>) => void): void {
    const existing = this.#listeners.get(name) ?? [];
    existing.push(listener);
    this.#listeners.set(name, existing);
  }

  close(): void {
    this.closed = true;
    this.readyState = 2;
  }

  /** Delivers one frame, the way the browser does for a matching `event:`. */
  emit(name: string, data: string): void {
    for (const listener of this.#listeners.get(name) ?? []) {
      listener({ data } as MessageEvent<string>);
    }
  }

  static latest(): FakeEventSource {
    const source = FakeEventSource.opened.at(-1);
    if (!source) {
      throw new Error("no EventSource was constructed");
    }
    return source;
  }

  static reset(): void {
    FakeEventSource.opened = [];
  }
}

/** A subscriber that records everything, so assertions read as one list. */
function subscriber() {
  const snapshots: unknown[] = [];
  const statuses: StreamStatus[] = [];
  const errors: string[] = [];
  return {
    snapshots,
    statuses,
    errors,
    request: {
      path: "/api/host-info/watch?interval=2s",
      event: "host-info",
      EventSourceImpl: FakeEventSource as unknown as typeof EventSource,
      onSnapshot: (snapshot: unknown) => snapshots.push(snapshot),
      onStatus: (status: StreamStatus) => statuses.push(status),
      onError: (message: string) => errors.push(message),
    },
  };
}

describe("watchPath", () => {
  test("names the interval the way the CLI flag spells it", () => {
    expect(watchPath("/api/host-info", "500ms")).toBe(
      "/api/host-info/watch?interval=500ms",
    );
  });

  test("escapes a value rather than letting it change the URL's shape", () => {
    // The server refuses this, which is the point: the caller must see the
    // daemon's `400` naming the value, not a URL that silently grew a parameter.
    expect(watchPath("/api/host-info", "2s&evil=1")).toBe(
      "/api/host-info/watch?interval=2s%26evil%3D1",
    );
  });
});

describe("watch", () => {
  test("opens the path it was given and registers the named event", () => {
    FakeEventSource.reset();
    const { request } = subscriber();

    watch(request);

    expect(FakeEventSource.latest().url).toBe("/api/host-info/watch?interval=2s");
  });

  test("reports connecting then live, in that order", () => {
    FakeEventSource.reset();
    const { request, statuses } = subscriber();

    watch(request);
    expect(statuses).toEqual(["connecting"]);

    FakeEventSource.latest().onopen?.();
    expect(statuses).toEqual(["connecting", "live"]);
  });

  test("delivers a parsed snapshot for the event it listens to", () => {
    FakeEventSource.reset();
    const { request, snapshots } = subscriber();

    watch(request);
    FakeEventSource.latest().emit("host-info", JSON.stringify({ hostname: "box" }));

    expect(snapshots).toEqual([{ hostname: "box" }]);
  });

  test("ignores a differently named event", () => {
    FakeEventSource.reset();
    const { request, snapshots, errors } = subscriber();

    watch(request);
    // A frame the server sent for another subscriber must not be read as a
    // snapshot: mixing them would render one widget's data in another.
    FakeEventSource.latest().emit("some-other-event", JSON.stringify({ hostname: "box" }));

    expect(snapshots).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("reports a frame that is not JSON instead of throwing", () => {
    FakeEventSource.reset();
    const { request, snapshots, errors } = subscriber();

    watch(request);
    FakeEventSource.latest().emit("host-info", "not json");

    expect(snapshots).toEqual([]);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain("not JSON");
  });

  test("reports an interrupted stream without closing the subscription", () => {
    FakeEventSource.reset();
    const { request, statuses, errors } = subscriber();

    watch(request);
    const source = FakeEventSource.latest();
    source.onerror?.();

    // `EventSource` reconnects on its own, so the subscription is still open and
    // the status is `error`, not `closed`: the panel shows a blip that the next
    // frame clears, rather than telling the operator to reconnect by hand.
    expect(source.closed).toBe(false);
    expect(statuses).toEqual(["connecting", "error"]);
    expect(errors).toHaveLength(1);

    source.emit("host-info", JSON.stringify({ hostname: "after-the-blip" }));
    expect(statuses.at(-1)).toBe("error");
  });

  test("closing calls close once and reports closed", () => {
    FakeEventSource.reset();
    const { request, statuses } = subscriber();

    const unsubscribe = watch(request);
    const source = FakeEventSource.latest();
    unsubscribe();
    // A second call is what a component does when it disconnects and the user
    // also pauses; it must not throw or report twice.
    unsubscribe();

    expect(source.closed).toBe(true);
    expect(statuses).toEqual(["connecting", "closed"]);
  });

  test("a closed subscription delivers no more frames", () => {
    FakeEventSource.reset();
    const { request, snapshots } = subscriber();

    const unsubscribe = watch(request);
    const source = FakeEventSource.latest();
    unsubscribe();
    source.emit("host-info", JSON.stringify({ hostname: "too late" }));

    expect(snapshots).toEqual([]);
  });

  test("says so when the environment has no EventSource", () => {
    const { request, statuses, errors } = subscriber();
    // happy-dom is exactly this case, so the branch is not hypothetical.
    const without = { ...request, EventSourceImpl: undefined as unknown as typeof EventSource };

    const unsubscribe = watch(without);

    expect(statuses).toEqual(["error"]);
    expect(errors[0]).toContain("EventSource");
    expect(() => unsubscribe()).not.toThrow();
  });
});

/**
 * The two ways an `EventSource` `error` differs.
 *
 * The event itself says nothing: the browser fires it for a dropped socket it
 * will retry and for a `404` it will not. `readyState` is the only thing that
 * distinguishes them, and a client that treats both as a blip promises a
 * reconnection that never comes.
 */
describe("a refusal the browser will not retry", () => {
  test("reports a stopped stream as closed, not as reconnecting", () => {
    FakeEventSource.reset();
    const { request, statuses, errors } = subscriber();

    watch(request);
    const source = FakeEventSource.latest();
    // What the browser does for a non-200 reply: it gives up and leaves the
    // source `CLOSED`. A `404` from a daemon older than the route looks exactly
    // like this.
    source.readyState = 2;
    source.onerror?.();

    expect(statuses).toEqual(["connecting", "closed"]);
    expect(errors).toHaveLength(1);
    // The message must not promise a retry that the browser has already
    // abandoned.
    expect(errors[0]).not.toContain("reconnecting");
  });

  test("a refused stream stays closed and sends no more frames", () => {
    FakeEventSource.reset();
    const { request, snapshots, statuses } = subscriber();

    watch(request);
    const source = FakeEventSource.latest();
    source.readyState = 2;
    source.onerror?.();
    source.emit("host-info", JSON.stringify({ hostname: "too late" }));

    expect(snapshots).toEqual([]);
    expect(statuses.at(-1)).toBe("closed");
  });

  test("a retryable error before any frame is still a reconnect", () => {
    FakeEventSource.reset();
    const { request, statuses, errors } = subscriber();

    watch(request);
    const source = FakeEventSource.latest();
    // `CONNECTING` after the error: the browser is retrying.
    source.readyState = 0;
    source.onerror?.();

    expect(statuses).toEqual(["connecting", "error"]);
    expect(errors[0]).toContain("reconnecting");
  });
});
