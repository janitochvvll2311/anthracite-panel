/**
 * The auth client: the four routes a session needs, and nothing else.
 *
 * Injected into `anth-login` the same way `HostInfoClient` is injected into the
 * shell, so the element's tests are about the form and never bind a port. The
 * exchange is `@anth/wire`'s, so this file is the routes.
 *
 * There is no token in here. The daemon sets `HttpOnly` cookies, and
 * `credentials: "same-origin"` in `send` is what carries them: a client that
 * read a token out of a response would be a client that could leak it to a
 * script.
 */

import { send, WireError } from "@anth/wire";

/** Whether an account exists yet, which decides register-or-login. */
export interface AuthState {
  registered: boolean;
  /**
   * Whether a session is required at all.
   *
   * `false` when the daemon runs with `[auth] enabled = false`: the gate then
   * has nothing to ask for, and showing a form whose `login` refuses every
   * password would be a dead end.
   */
  required: boolean;
}

export interface Credentials {
  username: string;
  password: string;
}

export interface AuthClient {
  state(): Promise<AuthState>;
  /**
   * Whether this browser already holds a live session.
   *
   * `/api/auth/state` answers whether an account exists, which is not the same
   * question: a reload with a valid cookie must not show the form again. There
   * is no "whoami" route, and the honest probe is the cheapest protected route —
   * so this asks the daemon instead of guessing from the cookie, which
   * JavaScript cannot read anyway.
   */
  session(): Promise<boolean>;
  register(credentials: Credentials): Promise<void>;
  login(credentials: Credentials): Promise<void>;
  logout(): Promise<void>;
}

export class HttpAuthClient implements AuthClient {
  constructor(private readonly base = "") {}

  state(): Promise<AuthState> {
    return send<AuthState>({ path: `${this.base}/api/auth/state` });
  }

  async session(): Promise<boolean> {
    try {
      await send<unknown>({ path: `${this.base}/api/host-info` });
      return true;
    } catch (error) {
      if (error instanceof WireError && error.status === 401) {
        return false;
      }
      throw error;
    }
  }

  register(credentials: Credentials): Promise<void> {
    return send<void>({
      method: "POST",
      path: `${this.base}/api/auth/register`,
      body: credentials,
    });
  }

  login(credentials: Credentials): Promise<void> {
    return send<void>({
      method: "POST",
      path: `${this.base}/api/auth/login`,
      body: credentials,
    });
  }

  logout(): Promise<void> {
    return send<void>({ method: "POST", path: `${this.base}/api/auth/logout` });
  }
}
