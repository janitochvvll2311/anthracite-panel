import { afterEach, describe, expect, test } from "bun:test";

import { AnthHelloLit } from "./hello-lit";

/**
 * The widget's load is its client's `get`; replacing it keeps the path real.
 *
 * The cast goes through `get`'s generic signature rather than around it: the
 * element asks for `Greeting`, and a stub typed `() => Promise<unknown>` would
 * not be assignable to that.
 */
function stub(element: AnthHelloLit, answer: () => Promise<unknown>): void {
  element.endpoints.get = answer as typeof element.endpoints.get;
}

/**
 * Builds the element with an answer already in place.
 *
 * `connectedCallback` loads on append, so stubbing after would leave the real
 * fetch to fail first and the test asserting against that refusal.
 */
function mount(answer?: () => Promise<unknown>): AnthHelloLit {
  const element = document.createElement("anth-hello-lit") as AnthHelloLit;
  if (answer) {
    stub(element, answer);
  }
  document.body.append(element);
  return element;
}

/** The text Lit painted, after its own update cycle settles. */
async function painted(element: AnthHelloLit): Promise<string> {
  await element.updateComplete;
  return element.shadowRoot?.textContent ?? "";
}

describe("a widget that is already a Lit component", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("is registered under its own tag, so it can sit beside the markup one", () => {
    expect(customElements.get("anth-hello-lit")).toBe(AnthHelloLit);
    expect(customElements.get("anth-hello-lit")).not.toBe(
      customElements.get("anth-hello"),
    );
  });

  test("renders the greeting through its own template", async () => {
    const element = mount(async () => ({ message: "Hola desde turing" }));
    await element.refresh();

    expect(element.state).toBe("ready");
    expect(await painted(element)).toContain("Hola desde turing");
  });

  test("shows the refusal in its own error state, with a retry", async () => {
    const element = mount(async () => {
      throw new Error("the host refused `host.hostname`");
    });
    await element.refresh();

    expect(element.state).toBe("error");
    const text = await painted(element);
    expect(text).toContain("the host refused");
    expect(text).toContain("Reintentar");

    stub(element, async () => ({ message: "recovered" }));
    element.shadowRoot?.querySelector("button")?.dispatchEvent(new Event("click"));
    await element.updateComplete;
    await element.updateComplete;

    expect(element.state).toBe("ready");
    expect(await painted(element)).toContain("recovered");
  });

  /**
   * The property this example exists to prove: a reload changes `message` and
   * leaves the element's own state alone, because Lit re-renders in place rather
   * than the host replacing the node.
   */
  test("keeps its own state across a reload", async () => {
    const element = mount(async () => ({ message: "first" }));
    await element.refresh();
    await element.updateComplete;

    // The counter button is the last one in the ready state; the retry only
    // exists in the error state, so this is unambiguous.
    element.shadowRoot?.querySelector("button")?.dispatchEvent(new Event("click"));
    await element.updateComplete;
    expect(element.clicks).toBe(1);

    stub(element, async () => ({ message: "second" }));
    await element.refresh();

    expect(element.clicks).toBe(1);
    expect(await painted(element)).toContain("second");
  });
});
