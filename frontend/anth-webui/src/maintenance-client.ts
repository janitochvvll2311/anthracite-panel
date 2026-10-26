/**
 * The maintenance client: the two routes that change what is running.
 *
 * One file and two methods, because they are two different operations that
 * happen to share a shape. `restart` replaces the daemon process and applies the
 * settings; `reboot` replaces every process on the host and applies nothing. A
 * client that offered one method with a flag would invite the page to present
 * them as equally recoverable.
 *
 * The exchange is `@anth/wire`'s, so this file is the routes and the shape of
 * the answers.
 */

import { send } from "@anth/wire";

/** What a restart accepted: the pid that will be re-used, `execv` keeping it. */
export interface RestartAccepted {
  restarting: boolean;
  pid: number;
  applies_settings: boolean;
  drop_terminals: boolean;
}

/** What a reboot accepted: how long until the host goes down. */
export interface RebootAccepted {
  rebooting: boolean;
  delay_s: number;
}

export interface MaintenanceClient {
  /** Replace the daemon. Resolves with what the old process said before it went. */
  restart(): Promise<RestartAccepted>;
  /**
   * Ask the host to go down.
   *
   * Throws a `WireError` with status 403 when the daemon was started without
   * `--allow-reboot`, which is the answer a page has to show rather than hide:
   * it is the only way the operator learns the switch exists.
   */
  reboot(delaySeconds: number): Promise<RebootAccepted>;
}

export class HttpMaintenanceClient implements MaintenanceClient {
  constructor(private readonly base = "") {}

  restart(): Promise<RestartAccepted> {
    return send<RestartAccepted>({ method: "POST", path: `${this.base}/api/restart` });
  }

  reboot(delaySeconds: number): Promise<RebootAccepted> {
    return send<RebootAccepted>({
      method: "POST",
      path: `${this.base}/api/reboot`,
      body: { delay_s: delaySeconds },
    });
  }
}
