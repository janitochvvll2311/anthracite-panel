import { afterEach, describe, expect, test } from "bun:test";

import { escapeHtml, type MarkupWidget } from "./contract";
import { mountWidget } from "./mount";
import { registerWidget } from "./register";

interface Greeting {
  message: string;
}

/** A child element with state of its own, standing in for a Lit component. */
class StatefulChild extends HTMLElement {
  clicks = 0;
  connectedCallback(): void {
    if (!this.shadowRoot) {
      this.attachShadow({ mode: "open" }).innerHTML = `<button>click</button>`;
    }
  }
}

if (!customElements.get("stateful-child")) {
  customElements.define("stateful-child", StatefulChild);
}

/** What a plugin writes: `load`, `view`, and nothing else. */
interface Behaviour {
  answer: () => Promise<Greeting>;
  content: (data: Greeting) => string | HTMLElement;
  updates: string[];
}

function behaviour(): Behaviour {
  return {
    answer: async () => ({ message: "hola" }),
    content: (data) => `<p class="greeting">${escapeHtml(data.message)}</p>`,
    updates: [],
  };
}

/** An element that is a widget, built the way a plugin builds one. */
function makeWidget(tag: string, script: Behaviour) {
  class Widget extends HTMLElement implements MarkupWidget<Greeting> {
    readonly handle = mountWidget(this, this);

    load(): Promise<Greeting> {
      return script.answer();
    }

    view(data: Greeting): string | HTMLElement {
      return script.content(data);
    }

    update(element: HTMLElement, data: Greeting): void {
      script.updates.push(data.message);
      element.setAttribute("data-message", data.message);
    }
  }

  registerWidget(tag, Widget);
  const element = document.createElement(tag) as Widget;
  document.body.append(element);
  return element;
}

/** One script for the whole file, reset between tests. */
let script = behaviour();

afterEach(() => {
  document.body.innerHTML = "";
  script = behaviour();
});

describe("mountWidget", () => {
  test("renders the loading state before the answer arrives", async () => {
    let release: (value: Greeting) => void = () => {};
    script.answer = () => new Promise<Greeting>((resolve) => (release = resolve));

    const element = makeWidget("anth-mount-loading", script);
    await Promise.resolve();
    expect(element.shadowRoot?.textContent).toContain("Cargando");

    release({ message: "hola" });
    await settle();
    expect(element.handle.state).toBe("ready");
  });

  test("renders the data once loaded", async () => {
    const element = makeWidget("anth-mount-ready", script);
    await settle();

    expect(element.handle.state).toBe("ready");
    expect(element.shadowRoot?.querySelector(".greeting")?.textContent).toBe("hola");
  });

  test("turns a failure into a message and a retry button", async () => {
    script.answer = async () => {
      throw new Error("the host refused");
    };

    const element = makeWidget("anth-mount-error", script);
    await settle();

    expect(element.handle.state).toBe("error");
    expect(element.shadowRoot?.textContent).toContain("the host refused");
    expect(element.shadowRoot?.querySelector(".anth-plugin-retry")).not.toBeNull();
  });

  test("a retry that succeeds leaves the error state behind", async () => {
    script.answer = async () => {
      throw new Error("first failure");
    };
    const element = makeWidget("anth-mount-retry", script);
    await settle();
    expect(element.handle.state).toBe("error");

    script.answer = async () => ({ message: "recovered" });
    await element.handle.refresh();

    expect(element.handle.state).toBe("ready");
    expect(element.shadowRoot?.textContent).toContain("recovered");
    expect(element.shadowRoot?.querySelector(".anth-plugin-retry")).toBeNull();
  });

  test("escapes the detail it puts in the failure markup", async () => {
    script.answer = async () => {
      throw new Error("<img src=x onerror=alert(1)>");
    };

    const element = makeWidget("anth-mount-escape", script);
    await settle();

    expect(element.shadowRoot?.querySelector("img")).toBeNull();
    expect(element.shadowRoot?.textContent).toContain("<img src=x");
  });

  test("mounting twice returns the handle it already built", async () => {
    const element = makeWidget("anth-mount-once", script);
    await settle();

    const again = mountWidget(element, element as unknown as MarkupWidget<Greeting>);
    expect(again).toBe(element.handle);

    // A second mount must not have added a second body to the shadow root.
    expect(element.shadowRoot?.querySelectorAll(".anth-plugin-body").length).toBe(1);
  });
});

describe("view returning an element", () => {
  test("keeps the same node mounted across reloads", async () => {
    const child = new StatefulChild();
    script.content = () => child;

    const element = makeWidget("anth-mount-kept", script);
    await settle();
    child.clicks = 3;

    script.answer = async () => ({ message: "again" });
    await element.handle.refresh();

    const mounted = element.shadowRoot?.querySelector("stateful-child");
    expect(mounted).toBe(child);
    expect(child.clicks).toBe(3);
  });

  test("hands the element the data on the first mount as well", async () => {
    const child = new StatefulChild();
    script.content = () => child;

    const element = makeWidget("anth-mount-first-update", script);
    await settle();

    // Without a first `update` the element would mount empty and only fill in
    // on a later reload, which reads as a blank widget.
    expect(child.getAttribute("data-message")).toBe("hola");
    expect(element.shadowRoot?.querySelector("stateful-child")).toBe(child);
  });

  test("replaces the node when the widget returns a different instance", async () => {
    const first = new StatefulChild();
    script.content = () => first;

    const element = makeWidget("anth-mount-replaced", script);
    await settle();

    const second = new StatefulChild();
    script.content = () => second;
    await element.handle.refresh();

    expect(element.shadowRoot?.querySelector("stateful-child")).toBe(second);
  });

  test("goes back to markup when the state is no longer ready", async () => {
    const child = new StatefulChild();
    script.content = () => child;

    const element = makeWidget("anth-mount-error-clears", script);
    await settle();

    script.answer = async () => {
      throw new Error("refused");
    };
    await element.handle.refresh();

    // A kept element must not survive into the error state, or the widget would
    // still show stale data beside its own failure message.
    expect(element.shadowRoot?.querySelector("stateful-child")).toBeNull();
    expect(element.shadowRoot?.textContent).toContain("refused");
  });

  test("does not reload when a click bubbles from inside the kept element", async () => {
    const child = new StatefulChild();
    script.content = () => child;

    const element = makeWidget("anth-mount-child-click", script);
    await settle();
    expect(element.shadowRoot?.querySelector("stateful-child")).toBe(child);

    let loads = 0;
    script.answer = async () => {
      loads += 1;
      return { message: "x" };
    };
    child.shadowRoot
      ?.querySelector("button")
      ?.dispatchEvent(new Event("click", { bubbles: true, composed: true }));
    await settle();

    expect(loads).toBe(0);
  });
});

describe("view returning markup", () => {
  test("still replaces the markup on every repaint", async () => {
    const element = makeWidget("anth-mount-markup", script);
    await settle();
    expect(element.shadowRoot?.textContent).toContain("hola");

    script.answer = async () => ({ message: "changed" });
    await element.handle.refresh();
    expect(element.shadowRoot?.textContent).toContain("changed");
  });

  test("never calls update", async () => {
    const element = makeWidget("anth-mount-no-update", script);
    await settle();
    await element.handle.refresh();

    expect(script.updates).toEqual([]);
  });
});

describe("the optional overrides", () => {
  test("loading, failure and body each replace one piece", async () => {
    let answer: () => Promise<Greeting> = async () => {
      throw new Error("boom");
    };

    class Custom extends HTMLElement implements MarkupWidget<Greeting> {
      readonly handle = mountWidget(this, this);
      load(): Promise<Greeting> {
        return answer();
      }
      view(): string {
        return "<p>ok</p>";
      }
      loading(): string {
        return "<p>waiting</p>";
      }
      failure(detail: string): string {
        return `<p class="own">${detail.toUpperCase()}</p>`;
      }
      body(markup: string): string {
        return `<section class="chrome">${markup}</section>`;
      }
    }

    registerWidget("anth-mount-custom", Custom);
    const element = document.createElement("anth-mount-custom") as Custom;
    document.body.append(element);

    // The loading state goes through `body` too, so a plugin that draws a card
    // keeps it while loading.
    await Promise.resolve();
    expect(element.shadowRoot?.querySelector(".chrome")).not.toBeNull();
    expect(element.shadowRoot?.textContent).toContain("waiting");

    await settle();
    expect(element.shadowRoot?.querySelector(".own")?.textContent).toBe("BOOM");
    expect(element.shadowRoot?.querySelector(".chrome")).not.toBeNull();

    answer = async () => ({ message: "x" });
    await element.handle.refresh();
    expect(element.shadowRoot?.textContent).toContain("ok");
  });
});

describe("registerWidget", () => {
  test("registers once and reports the re-registration honestly", () => {
    class Once extends HTMLElement {}
    expect(registerWidget("anth-once-test", Once)).toBe(true);
    expect(registerWidget("anth-once-test", Once)).toBe(false);
  });

  test("refuses a tag another element already owns", () => {
    class A extends HTMLElement {}
    class B extends HTMLElement {}
    registerWidget("anth-conflict-test", A);
    expect(() => registerWidget("anth-conflict-test", B)).toThrow(/already registered/);
  });

  test("refuses a name that is not a custom element", () => {
    class C extends HTMLElement {}
    expect(() => registerWidget("nocustomdash", C)).toThrow(/not a valid custom element/);
  });
});

describe("escapeHtml", () => {
  test("escapes every character that could end an attribute or an element", () => {
    expect(escapeHtml(`<a href="x">&'`)).toBe("&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  });
});

/** Lets the deferred load and its repaint finish. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}
