import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

import type { LaunchOptions } from "@playwright/test";

/// The signed-in session the suite reuses. Written once by global setup; every
/// spec starts from it, so a login round trip does not dominate the run.
export const STORAGE_STATE = fileURLToPath(
  new URL("../.auth/operator.json", import.meta.url),
);

/// The image that ships the toolchain also ships Chromium. Prefer it over a
/// Playwright browser download; `ANTH_CHROMIUM` overrides the path.
export function browserLaunchOptions(): LaunchOptions {
  const executablePath = process.env.ANTH_CHROMIUM ?? "/usr/bin/chromium";
  if (!existsSync(executablePath)) return {};
  return { executablePath, args: ["--no-sandbox"] };
}

export function panelURL(): string {
  return process.env.ANTH_PANEL_URL ?? "http://127.0.0.1:5173";
}
