import { expect, type Locator, type Page } from "@playwright/test";

import { TESTID } from "../testids";

/// `/login` is public and is the only page the suite drives without a session.
export class LoginPage {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  get form(): Locator {
    return this.page.getByTestId(TESTID.loginForm);
  }

  get password(): Locator {
    return this.page.getByTestId(TESTID.loginPassword);
  }

  get submitButton(): Locator {
    return this.page.getByTestId(TESTID.loginSubmit);
  }

  get status(): Locator {
    return this.page.getByTestId(TESTID.loginStatus);
  }

  async goto(): Promise<void> {
    await this.page.goto("/login");
    await expect(this.form).toBeVisible();
  }

  /// Fills and submits without assuming the outcome, so the refusal case is a
  /// first-class state of the same page object.
  async submit(password: string): Promise<void> {
    await this.password.fill(password);
    await this.submitButton.click();
  }

  /// Submits and waits for the gate to hand the browser to the shell.
  async signIn(password: string): Promise<void> {
    await this.submit(password);
    await this.page.waitForURL((url) => url.pathname !== "/login", {
      timeout: 20_000,
    });
  }

  async expectRefused(): Promise<void> {
    await expect(this.status).toHaveAttribute("data-state", "bad");
    await expect(this.status).not.toBeEmpty();
    await expect(this.form).toBeVisible();
  }

  /// The gate is the page itself; a shell route that needs a session lands
  /// here instead of rendering.
  async expectGate(): Promise<void> {
    await expect(this.page).toHaveURL(/\/login$/);
    await expect(this.form).toBeVisible();
  }
}
