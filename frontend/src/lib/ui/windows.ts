/**
 * Geometry and state for floating windows.
 *
 * Pure functions, no DOM: the shell keeps the state and the component renders
 * it, so drag maths and clamping are testable without a browser.
 */

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export interface Viewport {
  width: number
  height: number
}

/** A window never shrinks below this, or its own controls become unreachable. */
export const MIN_WIDTH = 320
export const MIN_HEIGHT = 200
/** Height of the title bar, which must stay on screen to drag the window back. */
export const TITLE_BAR_HEIGHT = 36

export interface WindowState {
  id: string
  rect: Rect
  minimized: boolean
  /** Higher paints on top. */
  z: number
}

/** A default position that cascades, so two windows do not land on each other. */
export function initialRect(index: number, viewport: Viewport): Rect {
  const offset = 32
  const step = 28
  const width = Math.max(
    MIN_WIDTH,
    Math.min(720, Math.round(viewport.width * 0.62)),
  )
  const height = Math.max(
    MIN_HEIGHT,
    Math.min(560, Math.round(viewport.height * 0.66)),
  )
  const cascade = (index % 5) * step
  return {
    x: clamp(offset + cascade, 0, Math.max(0, viewport.width - width)),
    y: clamp(offset + cascade, 0, Math.max(0, viewport.height - height)),
    width,
    height,
  }
}

/**
 * Keeps a dragged or resized window inside the viewport.
 *
 * The origin is preserved and the *size* is cut to the space that is left.
 * Moving the window instead would slide it left on every resize near an edge,
 * which reads as the drag fighting the pointer. The title bar is what must stay
 * reachable, so `y` is bounded by `TITLE_BAR_HEIGHT` rather than by the height.
 */
export function clampRect(rect: Rect, viewport: Viewport): Rect {
  const width = clamp(rect.width, MIN_WIDTH, Math.max(MIN_WIDTH, viewport.width))
  const height = clamp(
    rect.height,
    MIN_HEIGHT,
    Math.max(MIN_HEIGHT, viewport.height),
  )

  // The position is kept when the window fits there, and the size is cut to the
  // space that is left. Moving the window instead would slide it left on every
  // resize near an edge, which looks like the drag is fighting the pointer.
  // It only moves when even the minimum size cannot fit, so the title bar is
  // always reachable.
  const x = clamp(rect.x, 0, Math.max(0, viewport.width - MIN_WIDTH))
  const y = clamp(rect.y, 0, Math.max(0, viewport.height - TITLE_BAR_HEIGHT))

  return {
    x,
    y,
    width: Math.max(MIN_WIDTH, Math.min(width, viewport.width - x)),
    height: Math.max(MIN_HEIGHT, Math.min(height, viewport.height - y)),
  }
}

/**
 * Brings a window fully into view after the viewport itself changed.
 *
 * A browser window that shrank, or a layout restored from storage at a
 * different screen size, leaves windows outside the new bounds. Unlike a drag,
 * there is no pointer to respect here, so the window moves rather than being
 * cut down to a sliver.
 */
export function fitToViewport(rect: Rect, viewport: Viewport): Rect {
  const width = clamp(rect.width, MIN_WIDTH, Math.max(MIN_WIDTH, viewport.width))
  const height = clamp(
    rect.height,
    MIN_HEIGHT,
    Math.max(MIN_HEIGHT, viewport.height),
  )
  return {
    x: clamp(rect.x, 0, Math.max(0, viewport.width - width)),
    y: clamp(rect.y, 0, Math.max(0, viewport.height - TITLE_BAR_HEIGHT)),
    width,
    height,
  }
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

/** The next z index, so the clicked window comes to the front. */
export function nextZ(states: readonly WindowState[]): number {
  return states.reduce((top, state) => Math.max(top, state.z), 0) + 1
}

/**
 * Applies a pointer delta to a drag origin.
 *
 * The delta is applied to the *origin* rather than to the current position so
 * a fast drag cannot accumulate rounding drift, and clamping happens on the
 * result so a window dragged past an edge stops there instead of following the
 * pointer off-screen and needing a second drag to come back.
 */
export function dragRect(
  origin: Rect,
  delta: { dx: number; dy: number },
  viewport: Viewport,
): Rect {
  return clampRect(
    { ...origin, x: origin.x + delta.dx, y: origin.y + delta.dy },
    viewport,
  )
}

/** The same for a bottom-right resize handle. */
export function resizeRect(
  origin: Rect,
  delta: { dx: number; dy: number },
  viewport: Viewport,
): Rect {
  return clampRect(
    {
      ...origin,
      width: origin.width + delta.dx,
      height: origin.height + delta.dy,
    },
    viewport,
  )
}

const STORAGE_PREFIX = 'anth.windows.'

/** Persisted geometry, keyed by the shell's storage key. */
export function saveWindows(key: string, states: readonly WindowState[]): void {
  try {
    globalThis.localStorage?.setItem(
      STORAGE_PREFIX + key,
      JSON.stringify(states),
    )
  } catch {
    // Private mode and a full quota both throw. Losing the layout is not worth
    // failing the interaction that triggered the save.
  }
}

/**
 * Reads a stored layout, discarding anything malformed.
 *
 * A layout from an older build (or a hand-edited entry) must not be able to
 * crash the shell on boot, so every field is checked before it is used.
 */
export function loadWindows(key: string): WindowState[] {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_PREFIX + key)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed.filter(isWindowState).map((state) => ({
      ...state,
      rect: { ...state.rect },
    }))
  } catch {
    return []
  }
}

function isWindowState(value: unknown): value is WindowState {
  if (typeof value !== 'object' || value === null) return false
  const candidate = value as Partial<WindowState>
  const rect = candidate.rect as Partial<Rect> | undefined
  return (
    typeof candidate.id === 'string' &&
    candidate.id.length > 0 &&
    typeof candidate.minimized === 'boolean' &&
    typeof candidate.z === 'number' &&
    Number.isFinite(candidate.z) &&
    rect !== undefined &&
    typeof rect === 'object' &&
    [rect.x, rect.y, rect.width, rect.height].every(
      (n) => typeof n === 'number' && Number.isFinite(n),
    )
  )
}
