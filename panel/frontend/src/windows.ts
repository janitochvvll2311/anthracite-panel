// The state transitions of a floating window. The DOM and the mounts live in
// main.ts; these rules stay pure so they can be tested without a browser.

export type WindowMode = "normal" | "maximized" | "minimized";
export type RestoreMode = Exclude<WindowMode, "minimized">;

/// What the maximize control does to a visible window.
export function toggleMaximized(mode: WindowMode): RestoreMode {
  return mode === "maximized" ? "normal" : "maximized";
}

/// The mode a minimized window returns to. A window minimized while maximized
/// comes back maximized, not at the old floating size.
export function minimizedFrom(mode: WindowMode): RestoreMode {
  return mode === "maximized" ? "maximized" : "normal";
}
