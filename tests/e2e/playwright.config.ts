import { existsSync } from "node:fs";

import { defineConfig, devices } from "@playwright/test";

import { panelURL, STORAGE_STATE } from "./src/browser";

const systemChromium = process.env.ANTH_CHROMIUM ?? "/usr/bin/chromium";
const useSystemChromium = existsSync(systemChromium);

/// The suite runs against a live panel: `ANTH_PANEL_URL` names it and
/// `ANTH_PANEL_PASSWORD` supplies the operator password. There is no web
/// server entry: the demo, the release bundle and a deployment all boot
/// differently, and an e2e that only passes against one of them is not an e2e.
export default defineConfig({
  testDir: "./src/specs",
  globalSetup: "./src/global-setup.ts",
  outputDir: "./test-results",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,
  timeout: 45_000,
  expect: { timeout: 10_000 },
  reporter: process.env.CI
    ? [["list"], ["html", { open: "never" }]]
    : [["list"]],
  use: {
    baseURL: panelURL(),
    storageState: STORAGE_STATE,
    actionTimeout: 10_000,
    navigationTimeout: 20_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    ...(useSystemChromium
      ? {
          launchOptions: {
            executablePath: systemChromium,
            args: ["--no-sandbox"],
          },
        }
      : {}),
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
