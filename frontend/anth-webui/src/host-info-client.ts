/**
 * The host-info client: the only place that knows the route.
 *
 * Injected into the panel so a test can pass a plain object and never bind a
 * port, which is what keeps the component's tests about rendering. The exchange
 * itself is `@anth/wire`'s, so this file is the route and nothing else.
 */

import { type HostInfo, send } from "@anth/wire";

export interface HostInfoClient {
  read(): Promise<HostInfo>;
  /**
   * Where the SSE watch lives, for the widget to subscribe on its own.
   *
   * A URL rather than a `watch()` call: the widget already imports the stream
   * from `@anth/wire`, and handing it a subscription would mean the client owned
   * the element's lifecycle. This keeps the seam down to "which route".
   *
   * Optional because a client that only renders a fixed snapshot has no stream
   * to open, and an element given no URL stays a plain renderer rather than
   * subscribing to a route the client never named.
   */
  readonly watchUrl?: string;
}

export class HttpHostInfoClient implements HostInfoClient {
  readonly watchUrl: string;

  constructor(private readonly base = "") {
    this.watchUrl = `${this.base}/api/host-info`;
  }

  read(): Promise<HostInfo> {
    return send<HostInfo>({ path: `${this.base}/api/host-info` });
  }
}
