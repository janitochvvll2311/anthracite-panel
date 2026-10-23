import { afterEach, describe, expect, test } from "bun:test";

import { AnthHelloLit } from "./hello-lit";
import type { HelloCard } from "./lit-card";

/** A widget's load is its client's `get`; replacing it keeps the path real. */
function stub(element: AnthHelloLit, answer: () => Promise<unknown>): void {
  const client = (element as unknown as { client: { get: () => Promise<unknown> } }).client;
  client.get = answer;
}

function mount(): AnthHelloLit {
  const element = document.createElement("anth-hello-lit") as AnthHelloLit;
  document.body.append(element);
  return element;
}

function card(element: AnthHelloLit): HelloCard | null {
  return element.shadowRoot?.querySelector("hello-card") as HelloCard | null;
}

describe("a widget holding a Lit component", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("is registered under its own tag, so it can sit beside the markup one", () => {
    expect(customElements.get("anth-hello-lit")).toBe(AnthHelloLit);
    expect(customElements.get("anth-hello")).not.toBe(AnthHelloLit);
  });

  test("mounts the component and hands it the loaded data", async () => {
    const element = mount();
    stub(element, async () => ({ message: "Hola desde turing" }));
    await element.refresh();

    expect(element.state).toBe("ready");
    // The card renders into its own shadow root, which is what proves the base
    // mounted the element rather than interpolating it into a string.
    expect(card(element)?.message).toBe("Hola desde turing");
    expect(card(element)?.shadowRoot?.textContent).toContain("Hola desde turing");
  });

  test("keeps the same node across reloads, so its state survives", async () => {
    const element = mount();
    stub(element, async () => ({ message: "first" }));
    await element.refresh();

    const before = card(element);
    expect(before).not.toBeNull();
    before!.clicks = 7;

    stub(element, async () => ({ message: "second" }));
    await element.refresh();

    const after = card(element);
    expect(after).toBe(before); // same instance, not a re-created one
    expect(after!.clicks).toBe(7); // state preserved
    expect(after!.message).toBe("second"); // data still updated
  });

  test("updates the component rather than replacing it when the error clears", async () => {
    const element = mount();
    stub(element, async () => {
      throw new Error("the host refused `host.hostname`");
    });
    await element.refresh();
    expect(element.state).toBe("error");
    expect(card(element)).toBeNull();

    stub(element, async () => ({ message: "recovered" }));
    await element.refresh();

    expect(element.state).toBe("ready");
    expect(card(element)?.message).toBe("recovered");
  });

  test("does not mistake a click inside the component for a retry", async () => {
    const element = mount();
    stub(element, async () => ({ message: "x" }));
    await element.refresh();

    let loads = 0;
    stub(element, async () => {
      loads += 1;
      return { message: "x" };
    });

    // The card's own button bubbles a click; the base's delegated listener must
    // ignore it, or every click inside a kept widget would reload the data.
    const button = card(element)?.shadowRoot?.querySelector("button");
    button?.dispatchEvent(new Event("click", { bubbles: true, composed: true }));
    await Promise.resolve();
    await Promise.resolve();

    expect(loads).toBe(0);
  });
});
