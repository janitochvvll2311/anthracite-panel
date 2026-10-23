/**
 * The host-info client: the only place that knows the route.
 *
 * Injected into the panel so a test can pass a plain object and never bind a
 * port, which is what keeps the component's tests about rendering.
 */

import { type HostInfo, responseError } from "./wire";

export interface HostInfoClient {
  read(): Promise<HostInfo>;
}

export class HttpHostInfoClient implements HostInfoClient {
  constructor(private readonly base = "") {}

  async read(): Promise<HostInfo> {
    const response = await fetch(`${this.base}/api/host-info`, {
      credentials: "same-origin",
      headers: { accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(await responseError(response));
    }
    return (await response.json()) as HostInfo;
  }
}
