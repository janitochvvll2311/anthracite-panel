/**
 * `anth-auth-gate`: the login form, shown until the daemon stops refusing.
 *
 * The panel is useless without a session — every data route answers `401` — so
 * the gate is the app's first screen rather than a modal over a broken one. It
 * asks `/api/auth/state` once to choose between "create the operator account"
 * and "sign in", because the two forms differ in one field and in what they mean.
 *
 * It owns the shell's lifetime: `anth-app` is not in the document until a
 * session exists, because a mounted shell fetches a snapshot on connect and
 * would spend its first paint on a `401`. Creating it after the cookie arrives
 * means its first request is an authorised one.
 *
 * The element owns `state` and the form, never a token. The credential travels
 * as a `POST` body and comes back as an `HttpOnly` cookie, which is why nothing
 * here reads a response.
 */

import { escape } from "./html";
import type { AuthClient } from "./auth-client";

export class AnthAuthGate extends HTMLElement {
  /** Set before mounting by `main`, or replaced by a test. */
  static client: AuthClient | undefined;

  /**
   * Fired on the window once a session exists.
   *
   * An event rather than a callback property: the shell is a sibling in the
   * document, not a child of this element, and a callback would make one element
   * hold a reference to the other. A listener on `window` lets the composition
   * root wire the two without either knowing the other exists.
   */
  static readonly SIGNED_IN = "anth-signed-in";

  #registered = false;
  #busy = false;
  #error = "";
  #mounted = false;
  /**
   * Whether `/api/auth/state` has answered yet.
   *
   * Nothing is painted before it does: with auth off the gate is about to
   * announce a session, and a login form drawn for one frame would flash and
   * take typing for a password the daemon will never ask for.
   */
  #ready = false;

  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" });
    }
    // `customElements.define` connects the element while this module is
    // evaluated, which is before the composition root has assigned `client`.
    // Deferring one microtask lets that happen, exactly as `anth-app` does, so a
    // mounted gate does not report a client that is about to arrive.
    queueMicrotask(() => void this.check());
  }

  /** Asks which form applies. A failure leaves the login form up. */
  async check(): Promise<void> {
    this.#ready = true;
    const client = AnthAuthGate.client;
    if (!client) {
      this.#error = "No auth client was configured";
      this.render();
      return;
    }
    try {
      const state = await client.state();
      this.#registered = state.registered;
      // A daemon with auth off has nothing to ask: the shell mounts directly,
      // and the gate never renders a form. This is checked before the session
      // probe because there is no session to look for.
      if (!state.required) {
        window.dispatchEvent(new CustomEvent(AnthAuthGate.SIGNED_IN));
        return;
      }
      // A registered account plus a live cookie means the form is not needed at
      // all, which is the reload case: without this probe a refresh would ask
      // for the password again while the daemon was still honouring the session.
      if (state.registered && (await client.session())) {
        window.dispatchEvent(new CustomEvent(AnthAuthGate.SIGNED_IN));
        return;
      }
    } catch (error) {
      this.#error = error instanceof Error ? error.message : String(error);
    }
    this.render();
  }

  private render(): void {
    const root = this.shadowRoot;
    if (!root || !this.#ready) {
      return;
    }
    if (!this.#mounted) {
      root.innerHTML = `<style>${STYLE}</style><div class="card" role="main"></div>`;
      this.#mounted = true;
      this.#wire();
    }
    const card = root.querySelector(".card");
    if (!card) {
      return;
    }
    card.innerHTML = this.#form();
    root.querySelector<HTMLButtonElement>("button[type=submit]")?.toggleAttribute(
      "disabled",
      this.#busy,
    );
  }

  #form(): string {
    const heading = this.#registered ? "Sign in" : "Create the operator account";
    const action = this.#registered ? "Sign in" : "Create account";
    return `
      <h1>Anthracite Panel</h1>
      <p class="lead">${escape(heading)}</p>
      <form novalidate>
        <label>Username
          <input name="username" autocomplete="username" required autofocus>
        </label>
        <label>Password
          <input name="password" type="password" autocomplete="${
            this.#registered ? "current-password" : "new-password"
          }" required>
        </label>
        ${this.#registered ? "" : '<p class="hint">At least 8 characters.</p>'}
        ${this.#error ? `<p class="error" role="alert">${escape(this.#error)}</p>` : ""}
        <button type="submit">${escape(action)}</button>
      </form>`;
  }

  #wire(): void {
    this.shadowRoot?.addEventListener("submit", (event) => {
      event.preventDefault();
      void this.#submit();
    });
  }

  async #submit(): Promise<void> {
    const client = AnthAuthGate.client;
    if (!client || this.#busy) {
      return;
    }
    const form = this.shadowRoot?.querySelector("form");
    const data = new FormData(form ?? undefined);
    const username = String(data.get("username") ?? "").trim();
    const password = String(data.get("password") ?? "");
    if (!username || !password) {
      this.#error = "A username and a password are both required";
      this.render();
      return;
    }

    this.#busy = true;
    this.#error = "";
    this.render();
    try {
      if (this.#registered) {
        await client.login({ username, password });
      } else {
        await client.register({ username, password });
        this.#registered = true;
      }
      window.dispatchEvent(new CustomEvent(AnthAuthGate.SIGNED_IN));
    } catch (error) {
      this.#error = error instanceof Error ? error.message : String(error);
    }
    this.#busy = false;
    this.render();
  }
}

const STYLE = `
  /* A shadow root does not inherit the document's universal box-sizing reset:
     that selector stops at the boundary. Without it, a declared width adds to
     the element's own padding, and the card measured 424px inside a 390px
     viewport, scrolling the page sideways. */
  *, *::before, *::after { box-sizing: border-box; }

  :host {
    display: grid;
    place-items: center;
    min-height: 100svh;
    min-height: 100dvh;
    padding: max(1rem, env(safe-area-inset-top)) max(1rem, env(safe-area-inset-right))
             max(1rem, env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left));
    background: var(--anth-background, #101418);
    color: var(--anth-foreground, #e6edf3);
    font: 16px/1.5 system-ui, sans-serif;
  }
  :host([hidden]) { display: none; }
  .card {
    width: min(28rem, 100%);
    padding: 2rem;
    border-radius: 0.75rem;
    background: var(--anth-surface, #161b22);
    border: 1px solid var(--anth-border, #30363d);
  }
  h1 { margin: 0 0 0.25rem; font-size: 1.25rem; }
  .lead { margin: 0 0 1.5rem; color: var(--anth-muted, #8b949e); }
  form { display: grid; gap: 1rem; }
  label { display: grid; gap: 0.35rem; font-size: 0.875rem; }
  input {
    /* 16px on a phone: Safari zooms the page when a focused field is smaller,
       and a zoomed login form is one the visitor has to pinch back. */
    min-height: 44px;
    padding: 0.6rem 0.7rem;
    font-size: max(16px, 1rem);
    border-radius: 0.4rem;
    border: 1px solid var(--anth-border, #30363d);
    background: var(--anth-background, #101418);
    color: inherit;
    font: inherit;
  }
  button {
    min-height: 44px;
    padding: 0.65rem 1rem;
    border-radius: 0.4rem;
    border: 0;
    background: var(--anth-accent, #2f81f7);
    color: #fff;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  button[disabled] { opacity: 0.6; cursor: progress; }
  .error { margin: 0; color: var(--anth-danger, #f85149); }
  .hint { margin: 0; font-size: 0.8125rem; color: var(--anth-muted, #8b949e); }

  /* A phone: the card is the whole screen, so its own padding is wasted space
     the form needs for the keyboard-safe area. */
  @media (max-width: 30rem) {
    .card { padding: 1.5rem 1.25rem; }
    h1 { font-size: 1.15rem; }
  }
`;

customElements.define("anth-auth-gate", AnthAuthGate);
