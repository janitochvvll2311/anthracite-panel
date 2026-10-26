/**
 * One SSE subscription, and nothing above it.
 *
 * The counterpart of `send` in `exchange.ts`: that one takes one reply, this one
 * keeps taking them. It is `EventSource` rather than a `fetch` reader on
 * purpose — reconnecting, honouring the server's `retry:` hint and resuming from
 * `Last-Event-ID` are the browser's job, already written and already tested
 * against every proxy in the wild. A reader in this file would be a second SSE
 * implementation whose bugs would look like panel bugs.
 *
 * What this module owns is the part the browser does not do: naming the event,
 * parsing the payload, and turning the four lifecycle moments into one status a
 * component can render.
 */

/** Where a subscription is, as one value rather than three booleans. */
export type StreamStatus = "connecting" | "live" | "error" | "closed";

export interface WatchRequest<T> {
  /** The full path, query included, e.g. `/api/host-info/watch?interval=2s`. */
  path: string;
  /** The SSE `event:` name the server sends. Anything else is ignored. */
  event: string;
  onSnapshot: (snapshot: T) => void;
  onStatus?: (status: StreamStatus) => void;
  /**
   * A malformed payload or a dropped stream, already turned into prose.
   *
   * Not called for every `error`: a close the caller asked for is not one, and
   * the browser retrying on its own reports through `onStatus` alone when the
   * component has nothing to add to it.
   */
  onError?: (message: string) => void;
  /**
   * The `EventSource` to construct.
   *
   * Injected because a test environment may not have one — happy-dom does not —
   * and because a test that passes its own can assert the URL and the event name
   * without binding a port. Production passes the global.
   */
  EventSourceImpl?: typeof EventSource;
}

/**
 * Subscribes to one SSE endpoint and returns the function that closes it.
 *
 * Closing is idempotent: a component may unsubscribe in `disconnectedCallback`
 * and again when the user pauses, and the second call must not throw.
 */
export function watch<T>(request: WatchRequest<T>): () => void {
  const Impl = request.EventSourceImpl ?? globalThis.EventSource;
  if (!Impl) {
    // A browser without `EventSource` is not a browser this panel supports, and
    // saying so is better than a subscription that silently never delivers.
    request.onError?.("This browser has no EventSource, so the panel cannot stream");
    request.onStatus?.("error");
    return () => {};
  }

  const source = new Impl(request.path);
  let closed = false;

  const close = (status: StreamStatus) => {
    if (closed) {
      return;
    }
    closed = true;
    source.close();
    request.onStatus?.(status);
  };

  request.onStatus?.("connecting");

  source.onopen = () => {
    if (!closed) {
      request.onStatus?.("live");
    }
  };

  source.addEventListener(request.event, (event) => {
    if (closed) {
      return;
    }
    const data = (event as MessageEvent<string>).data;
    try {
      request.onSnapshot(JSON.parse(data) as T);
    } catch {
      // A frame that is not the documented shape is a bug on the daemon side.
      // Reporting it beats throwing inside an event handler, where the failure
      // would reach the console and nowhere else.
      request.onError?.(`the daemon sent a frame that is not JSON: ${data.slice(0, 80)}`);
    }
  });

  source.onerror = () => {
    if (closed) {
      return;
    }
    // The `error` event is fired for two different situations and says nothing
    // about which: a socket that dropped and will be retried, and a reply that
    // will never succeed — a `404` from a daemon without the route, a `401`, a
    // CORS refusal. `readyState` is the browser's own answer: it leaves the
    // source `CONNECTING` for the retry and `CLOSED` when it has given up.
    //
    // Reading it is what keeps the panel honest. Treating both as a blip shows
    // "reconnecting" forever against a route that answers `404`, and no frame is
    // ever coming to clear it.
    if (source.readyState === Impl.CLOSED) {
      close("closed");
      request.onError?.(
        "The host stream was refused; the daemon may not have the watch route (is it the running version?)",
      );
      return;
    }
    // Still `CONNECTING`: the browser is retrying on its own, so the panel
    // shows the blip and clears it when the next frame arrives, instead of
    // telling the operator to do something.
    request.onError?.("The host stream was interrupted; reconnecting");
    request.onStatus?.("error");
  };

  return () => close("closed");
}

/**
 * The query the watch route expects.
 *
 * `encodeURIComponent` because the value is the same `500ms` / `2s` text the CLI
 * flag takes, and a caller that passed something exotic must get the server's
 * `400` naming it rather than a URL that changed shape on the way.
 */
export function watchPath(base: string, interval: string): string {
  return `${base}/watch?interval=${encodeURIComponent(interval)}`;
}
