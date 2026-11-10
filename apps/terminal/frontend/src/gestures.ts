import type { KeyName } from "./keys";

type GestureHandlers = {
  sendKey: (key: KeyName) => void;
  focusInput: () => void;
  getFontSize: () => number;
  setFontSize: (size: number) => void;
};

/// Mobile gestures on the terminal surface:
/// - double tap sends Tab;
/// - pinch changes the terminal font size;
/// - long press and drag sends arrow keys;
/// - a long press without movement keeps the native text selection.
export function bindTerminalGestures(
  screen: HTMLElement,
  handlers: GestureHandlers,
): () => void {
  let lastTap = 0;
  let tapReset: number | undefined;
  let hold: number | undefined;
  let startX = 0;
  let startY = 0;
  let lastX = 0;
  let lastY = 0;
  let arrows = false;
  let moved = false;
  let pinching = false;
  let suppressTap = false;
  let pinchStart = 0;
  let pinchFont = 0;

  const clearHold = (): void => {
    if (hold !== undefined) {
      window.clearTimeout(hold);
      hold = undefined;
    }
  };

  const distance = (touches: TouchList): number => {
    const first = touches[0];
    const second = touches[1];
    if (!first || !second) return 0;
    return Math.hypot(
      first.clientX - second.clientX,
      first.clientY - second.clientY,
    );
  };

  const onStart = (event: TouchEvent): void => {
    if (event.touches.length === 2) {
      clearHold();
      arrows = false;
      moved = false;
      pinching = true;
      suppressTap = true;
      pinchStart = distance(event.touches);
      pinchFont = handlers.getFontSize();
      event.preventDefault();
      return;
    }
    if (event.touches.length !== 1) return;
    const touch = event.touches[0];
    if (!touch) return;
    suppressTap = false;
    startX = lastX = touch.clientX;
    startY = lastY = touch.clientY;
    moved = false;
    arrows = false;
    pinching = false;
    clearHold();
    hold = window.setTimeout(() => {
      hold = undefined;
      arrows = true;
    }, 450);
  };

  const onMove = (event: TouchEvent): void => {
    if (pinching) {
      if (event.touches.length < 2) return;
      event.preventDefault();
      const current = distance(event.touches);
      if (pinchStart > 0) {
        handlers.setFontSize(pinchFont * (current / pinchStart));
      }
      return;
    }
    const touch = event.touches[0];
    if (!touch) return;
    if (arrows) {
      event.preventDefault();
      window.getSelection()?.removeAllRanges();
      const deltaX = touch.clientX - lastX;
      const deltaY = touch.clientY - lastY;
      const threshold = 24;
      if (
        Math.abs(deltaX) >= threshold ||
        Math.abs(deltaY) >= threshold
      ) {
        if (Math.abs(deltaX) >= Math.abs(deltaY)) {
          handlers.sendKey(deltaX > 0 ? "right" : "left");
        } else {
          handlers.sendKey(deltaY > 0 ? "down" : "up");
        }
        lastX = touch.clientX;
        lastY = touch.clientY;
      }
      return;
    }
    if (
      Math.abs(touch.clientX - startX) > 8 ||
      Math.abs(touch.clientY - startY) > 8
    ) {
      moved = true;
      clearHold();
    }
  };

  const onEnd = (event: TouchEvent): void => {
    if (pinching) {
      if (event.touches.length >= 2) return;
      pinching = false;
      suppressTap = true;
      return;
    }
    if (arrows) {
      arrows = false;
      moved = true;
      return;
    }
    clearHold();
    if (moved) {
      moved = false;
      return;
    }
    if (suppressTap) {
      suppressTap = false;
      return;
    }
    const now = Date.now();
    if (now - lastTap < 320) {
      event.preventDefault();
      lastTap = 0;
      if (tapReset !== undefined) {
        window.clearTimeout(tapReset);
        tapReset = undefined;
      }
      handlers.sendKey("tab");
      handlers.focusInput();
      return;
    }
    lastTap = now;
    event.preventDefault();
    handlers.focusInput();
    if (tapReset !== undefined) window.clearTimeout(tapReset);
    tapReset = window.setTimeout(() => {
      lastTap = 0;
      tapReset = undefined;
    }, 320);
  };

  const onCancel = (): void => {
    clearHold();
    arrows = false;
    moved = false;
    pinching = false;
    suppressTap = false;
  };

  screen.addEventListener("touchstart", onStart, { passive: false });
  screen.addEventListener("touchmove", onMove, { passive: false });
  screen.addEventListener("touchend", onEnd, { passive: false });
  screen.addEventListener("touchcancel", onCancel, { passive: false });
  return () => {
    clearHold();
    if (tapReset !== undefined) window.clearTimeout(tapReset);
    screen.removeEventListener("touchstart", onStart);
    screen.removeEventListener("touchmove", onMove);
    screen.removeEventListener("touchend", onEnd);
    screen.removeEventListener("touchcancel", onCancel);
  };
}
