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
}

export class HttpHostInfoClient implements HostInfoClient {
  constructor(private readonly base = "") {}

  read(): Promise<HostInfo> {
    return send<HostInfo>({ path: `${this.base}/api/host-info` });
  }
}
