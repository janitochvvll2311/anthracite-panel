import { expect, type FrameLocator, type Locator, type Page } from "@playwright/test";

import { TESTID } from "../testids";

export type MountedContribution = {
  app?: string;
  tag?: string;
  kind?: "view" | "action" | "settings" | "modal";
};

/// One contribution mounted in the sandboxed frame. The host is the panel-side
/// container (`pane-body`, `window-body`, `#modal-body` or a settings section);
/// everything inside the frame comes through `contentFrame()`.
export class SlotFrame {
  readonly page: Page;
  readonly host: Locator;

  constructor(page: Page, host: Locator) {
    this.page = page;
    this.host = host;
  }

  get iframe(): Locator {
    return this.host.getByTestId(TESTID.appFrame);
  }

  get frame(): FrameLocator {
    return this.iframe.contentFrame();
  }

  /// The frame's `<body>`, where the module mounts. The panel marks it with the
  /// contribution it mounted, which is how a fake render is ruled out.
  get body(): Locator {
    return this.frame.getByTestId(TESTID.slotRoot);
  }

  /// The frame's `<html>`, which carries the panel's viewport mode.
  get html(): Locator {
    return this.frame.locator("html");
  }

  get error(): Locator {
    return this.host.getByTestId(TESTID.errorCard);
  }

  get loading(): Locator {
    return this.host.getByTestId(TESTID.loading);
  }

  /// Waits for the whole handshake: the iframe mounted the module and the frame
  /// reports the same contribution the panel asked for. Never waits on a label.
  async waitUntilMounted(
    expected: MountedContribution = {},
  ): Promise<void> {
    await expect(this.iframe).toBeVisible();
    await expect(this.iframe).toHaveAttribute("data-state", "mounted", {
      timeout: 20_000,
    });
    await expect(this.body).toHaveAttribute("data-state", "mounted");
    if (expected.app) {
      await expect(this.body).toHaveAttribute("data-app", expected.app);
    }
    if (expected.tag) {
      await expect(this.body).toHaveAttribute("data-tag", expected.tag);
    }
    if (expected.kind) {
      await expect(this.body).toHaveAttribute("data-kind", expected.kind);
    }
    await expect(this.error).toHaveCount(0);
    // The module renders into the frame body; a mounted body with no node is a
    // module that mounted an empty view.
    await expect(this.body.locator("*").first()).toBeAttached();
  }
}
