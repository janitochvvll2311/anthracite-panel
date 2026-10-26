import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import type {
  MaintenanceClient,
  RebootAccepted,
  RestartAccepted,
} from "./maintenance-client";
import { AnthMaintenance } from "./maintenance";

const RESTARTED: RestartAccepted = {
  restarting: true,
  pid: 4242,
  applies_settings: true,
  drop_terminals: true,
};

const REBOOTED: RebootAccepted = { rebooting: true, delay_s: 5 };

/**
 * A client that records what the page asked for.
 *
 * Not a mock of the transport: the assertions below are about which method the
 * page calls and what it does with an `error` the daemon sent back, and the
 * routes themselves are covered by the Rust suite against a real router.
 */
class Recorder implements MaintenanceClient {
  restarts = 0;
  delays: number[] = [];
  failure: Error | undefined;

  async restart(): Promise<RestartAccepted> {
    this.restarts += 1;
    if (this.failure) {
      throw this.failure;
    }
    return RESTARTED;
  }

  async reboot(delaySeconds: number): Promise<RebootAccepted> {
    this.delays.push(delaySeconds);
    if (this.failure) {
      throw this.failure;
    }
    return { ...REBOOTED, delay_s: delaySeconds };
  }
}

function mount(client: MaintenanceClient): AnthMaintenance {
  AnthMaintenance.client = client;
  const element = new AnthMaintenance();
  document.body.append(element);
  // Without a shadow root every query below answers nothing and every
  // assertion would pass against an empty element.
  expect(element.shadowRoot).not.toBeNull();
  return element;
}

function button(element: AnthMaintenance, action: string): HTMLButtonElement {
  const found = element.shadowRoot?.querySelector<HTMLButtonElement>(
    `[data-action="${action}"]`,
  );
  expect(found).not.toBeNull();
  return found!;
}

function said(element: AnthMaintenance): string {
  return element.shadowRoot?.querySelector(".said")?.textContent ?? "";
}

/** Answers the page's `confirm` and remembers the question. */
function answerConfirm(agreed: boolean): { asked: string[] } {
  const asked: string[] = [];
  globalThis.confirm = (message?: string) => {
    asked.push(message ?? "");
    return agreed;
  };
  return { asked };
}

const realConfirm = globalThis.confirm;

beforeEach(() => {
  AnthMaintenance.client = undefined;
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.confirm = realConfirm;
});

describe("the maintenance page", () => {
  test("offers the two actions as separate controls", () => {
    const element = mount(new Recorder());

    // Two operations with different blast radii must be two buttons, not one
    // button and a checkbox.
    expect(button(element, "restart")).toBeDefined();
    expect(button(element, "reboot")).toBeDefined();
  });

  test("says what each one costs", () => {
    const element = mount(new Recorder());
    const text = element.shadowRoot?.textContent ?? "";

    // The page is the only place an operator learns that a restart keeps the
    // session and a reboot needs a supervisor.
    expect(text).toContain("terminals");
    expect(text).toContain("--allow-reboot");
    expect(text).toContain("does not apply any setting");
  });

  test("a restart asks first, then calls the route", async () => {
    const client = new Recorder();
    const element = mount(client);
    const confirmations = answerConfirm(true);

    button(element, "restart").click();
    await Bun.sleep(0);

    expect(confirmations.asked).toHaveLength(1);
    expect(confirmations.asked[0]).toContain("terminal");
    expect(client.restarts).toBe(1);
    // The pid is what the new process kept, so showing it is the proof the
    // answer came from the old one.
    expect(said(element)).toContain("4242");
  });

  test("declining the question sends nothing", async () => {
    const client = new Recorder();
    const element = mount(client);
    answerConfirm(false);

    button(element, "restart").click();
    await Bun.sleep(0);

    expect(client.restarts).toBe(0);
    expect(said(element)).toBe("");
  });

  test("a reboot sends the delay the operator typed", async () => {
    const client = new Recorder();
    const element = mount(client);
    answerConfirm(true);
    const delay = element.shadowRoot?.querySelector<HTMLInputElement>("[data-delay]");
    expect(delay).not.toBeNull();
    delay!.value = "30";

    button(element, "reboot").click();
    await Bun.sleep(0);

    expect(client.delays).toEqual([30]);
    expect(said(element)).toContain("30s");
  });

  test("the reboot question says the host goes down", async () => {
    const element = mount(new Recorder());
    const confirmations = answerConfirm(false);

    button(element, "reboot").click();
    await Bun.sleep(0);

    // A question that only said "reboot?" would let the operator read it as a
    // panel restart, which is the confusion this page exists to remove.
    expect(confirmations.asked[0]).toContain("host");
    expect(confirmations.asked[0]).toContain("Every process");
  });

  test("a refused reboot shows the daemon's own wording", async () => {
    const client = new Recorder();
    const { WireError } = await import("@anth/wire");
    client.failure = new WireError(
      "this daemon may not reboot the host: start it with --allow-reboot",
      403,
    );
    const element = mount(client);
    answerConfirm(true);

    button(element, "reboot").click();
    await Bun.sleep(0);

    // The daemon names the flag; replacing that with "forbidden" would hide the
    // one piece of information that fixes it.
    expect(said(element)).toContain("--allow-reboot");
    expect(element.shadowRoot?.querySelector(".said")?.classList.contains("err")).toBe(true);
  });

  test("a page with no client says so instead of doing nothing", async () => {
    const element = mount(undefined as unknown as MaintenanceClient);
    AnthMaintenance.client = undefined;
    answerConfirm(true);

    button(element, "restart").click();
    await Bun.sleep(0);

    expect(said(element)).toContain("No client");
  });
});
