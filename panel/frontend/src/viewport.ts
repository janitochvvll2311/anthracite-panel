// Discrete layout modes for the panel. `auto` follows the window; the other
// three pin the layout so a desktop browser can inspect the phone shell (and
// the reverse) without resizing the window.

export const VIEWPORT_MODES = ["auto", "desktop", "tablet", "mobile"] as const;

export type ViewportMode = (typeof VIEWPORT_MODES)[number];
export type ResolvedViewport = Exclude<ViewportMode, "auto">;

/// Where the choice lives across reloads.
export const VIEWPORT_KEY = "anth-panel-viewport";

/// Breakpoints are deliberately wide and few: a panel is not a responsive site,
/// and a discrete layout is easier to reason about than a fluid one.
export const TABLET_MIN = 700;
export const DESKTOP_MIN = 1100;

export function isViewportMode(value: string | null | undefined): value is ViewportMode {
  return value === "auto" || value === "desktop" || value === "tablet" || value === "mobile";
}

export function resolveViewport(
  mode: ViewportMode,
  width: number,
): ResolvedViewport {
  if (mode !== "auto") return mode;
  if (width < TABLET_MIN) return "mobile";
  if (width < DESKTOP_MIN) return "tablet";
  return "desktop";
}
