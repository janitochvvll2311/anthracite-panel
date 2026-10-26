import { afterEach, describe, expect, test } from "bun:test";

import { WireError } from "@anth/wire";

import { AnthAuthGate } from "./auth-gate";
import type { AuthClient, Credentials } from "./auth-client";

/**
 * A client that records what it was asked and answers what the case needs.
 *
 * The element's job is the form and the two routes it chooses between, so the
 * seam is the client: these tests never bind a port and never hash a password.
 */
function stub(overrides: Partial<AuthClient> = {}): AuthClient & {
  calls: string[];
  credentials: Credentials[];
} {
  const calls: string[] = [];
  const credentials: Credentials[] = [];
  return {
    calls,
    credentials,
    async state() {
      calls.push("state");
      return { registered: true, required: true };
    },
    async session() {
      calls.push("session");
      return false;
    },
    async register(body) {
      calls.push("register");
      credentials.push(body);
    },
    async login(body) {
      calls.push("login");
      credentials.push(body);
    },
    async logout() {
      calls.push("logout");
    },
    ...overrides,
  };
}

/** Mounts the gate and lets its `connectedCallback` probe settle. */
async function mount(client: AuthClient): Promise<AnthAuthGate> {
  AnthAuthGate.client = client;
  const element = document.createElement("anth-auth-gate") as AnthAuthGate;
  document.body.append(element);
  await settle();
  return element;
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function text(element: AnthAuthGate): string {
  return element.shadowRoot?.textContent ?? "";
}

function form(element: AnthAuthGate): HTMLFormElement {
  const found = element.shadowRoot?.querySelector("form");
  if (!found) {
    throw new Error("the gate has no form");
  }
  return found as HTMLFormElement;
}

function submit(element: AnthAuthGate, username: string, password: string): void {
  const root = element.shadowRoot;
  if (!root) {
    throw new Error("the gate has no shadow root");
  }
  (root.querySelector("input[name=username]") as HTMLInputElement).value = username;
  (root.querySelector("input[name=password]") as HTMLInputElement).value = password;
  form(element).dispatchEvent(new Event("submit", { cancelable: true, bubbles: true }));
}

/** Whether the submit button currently refuses a second press. */
function buttonState(element: AnthAuthGate): boolean {
  return (
    element.shadowRoot
      ?.querySelector("button[type=submit]")
      ?.hasAttribute("disabled") ?? false
  );
}

/** The event the composition root listens on to reveal the shell. */
function signedIn(): { fired: () => boolean; stop: () => void } {
  let fired = false;
  const listener = (): void => {
    fired = true;
  };
  window.addEventListener(AnthAuthGate.SIGNED_IN, listener);
  return {
    fired: () => fired,
    stop: () => window.removeEventListener(AnthAuthGate.SIGNED_IN, listener),
  };
}

afterEach(() => {
  document.body.innerHTML = "";
  AnthAuthGate.client = undefined;
});

describe("anth-auth-gate", () => {
  test("a registered daemon asks to sign in, not to register", async () => {
    const element = await mount(stub());
    expect(text(element)).toContain("Sign in");
    expect(text(element)).not.toContain("Create the operator account");

    const button = element.shadowRoot?.querySelector("button[type=submit]");
    expect(button?.textContent?.trim()).toBe("Sign in");
  });

  test("a daemon with no account asks to create one", async () => {
    const client = stub({ state: async () => ({ registered: false, required: true }) });
    const element = await mount(client);

    expect(text(element)).toContain("Create the operator account");
    expect(client.calls).not.toContain("session");
  });

  test("submitting the login form sends the credentials and announces the session", async () => {
    const client = stub();
    const element = await mount(client);
    const event = signedIn();
    try {
      submit(element, "operator", "correct horse battery");
      await settle();
      await settle();

      expect(client.credentials).toEqual([
        { username: "operator", password: "correct horse battery" },
      ]);
      expect(client.calls).toContain("login");
      expect(event.fired()).toBe(true);
    } finally {
      event.stop();
    }
  });

  test("the bootstrap registers rather than logging in", async () => {
    const client = stub({ state: async () => ({ registered: false, required: true }) });
    const element = await mount(client);
    const event = signedIn();
    try {
      submit(element, "operator", "correct horse battery");
      await settle();
      await settle();

      expect(client.calls).toContain("register");
      expect(client.calls).not.toContain("login");
      expect(event.fired()).toBe(true);
    } finally {
      event.stop();
    }
  });

  test("a live cookie skips the form entirely", async () => {
    const client = stub({ session: async () => true });
    const event = signedIn();
    try {
      await mount(client);
      expect(event.fired()).toBe(true);
    } finally {
      event.stop();
    }
  });

  test("a daemon with auth off mounts the shell without a form", async () => {
    const client = stub({
      state: async () => ({ registered: false, required: false }),
    });
    const event = signedIn();
    try {
      const element = await mount(client);
      expect(event.fired()).toBe(true);
      // No session to probe for: asking would be a request the answer cannot
      // change, and a form here would refuse every password it was given.
      expect(client.calls).not.toContain("session");
      expect(client.calls).not.toContain("login");
      expect(element.shadowRoot?.querySelector("form")).toBeNull();
    } finally {
      event.stop();
    }
  });

  test("a refused credential is shown and does not announce a session", async () => {
    const client = stub({
      login: async () => {
        throw new WireError("the password is not correct", 401);
      },
    });
    const element = await mount(client);
    const event = signedIn();
    try {
      submit(element, "operator", "wrong");
      await settle();
      await settle();

      expect(text(element)).toContain("the password is not correct");
      expect(event.fired()).toBe(false);
    } finally {
      event.stop();
    }
  });

  test("an empty field is refused before any request", async () => {
    const client = stub();
    const element = await mount(client);
    const before = client.credentials.length;

    submit(element, "", "");
    await settle();

    expect(client.credentials.length).toBe(before);
    expect(text(element)).toContain("both required");
  });

  test("the error is escaped, not injected", async () => {
    const client = stub({
      login: async () => {
        throw new Error('<img src=x onerror="alert(1)">');
      },
    });
    const element = await mount(client);
    submit(element, "operator", "x");
    await settle();
    await settle();

    expect(element.shadowRoot?.querySelector("img")).toBeNull();
    expect(text(element)).toContain("<img");
  });

  test("a failing state probe leaves the form usable", async () => {
    const client = stub({
      state: async () => {
        throw new Error("the daemon is unreachable");
      },
    });
    const element = await mount(client);

    expect(text(element)).toContain("the daemon is unreachable");
    expect(element.shadowRoot?.querySelector("form")).not.toBeNull();
  });

  test("the button is disabled while a request is in flight", async () => {
    let release: () => void = () => {};
    const client = stub({
      login: () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    });
    const element = await mount(client);
    submit(element, "operator", "correct horse battery");
    await settle();

    // Re-queried each time: `render()` rewrites the card, so a held reference
    // points at a detached button and its attributes no longer mean anything.
    expect(buttonState(element)).toBe(true);

    release();
    await settle();
    await settle();
    expect(buttonState(element)).toBe(false);
  });
});
