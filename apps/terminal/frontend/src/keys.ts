/// The special keys the mobile key bar can send. The sequences are the ones a
/// VT100-style terminal expects; the service receives raw bytes.
export type KeyName =
  | "esc"
  | "tab"
  | "up"
  | "down"
  | "left"
  | "right"
  | "home"
  | "end"
  | "pgup"
  | "pgdn"
  | "ctrlc"
  | "ctrld"
  | "ctrll"
  | "ctrlz";

export const KEY_SEQUENCES: Record<KeyName, string> = {
  esc: "\x1b",
  tab: "\t",
  up: "\x1b[A",
  down: "\x1b[B",
  right: "\x1b[C",
  left: "\x1b[D",
  home: "\x1b[H",
  end: "\x1b[F",
  pgup: "\x1b[5~",
  pgdn: "\x1b[6~",
  ctrlc: "\x03",
  ctrld: "\x04",
  ctrll: "\x0c",
  ctrlz: "\x1a",
};

/// Symbols that are awkward to reach on the mobile keyboard.
export const BAR_SYMBOLS = ["|", "~", "/", "-", "_", "="];

const encoder = new TextEncoder();

export function encodeText(value: string): number[] {
  return Array.from(encoder.encode(value));
}

/// Applies sticky Ctrl/Alt to the next single character typed on the soft
/// keyboard or the key bar. Non-single-character values pass through.
export function applyModifiers(
  value: string,
  ctrl: boolean,
  alt: boolean,
): string {
  if ((!ctrl && !alt) || [...value].length !== 1) return value;
  const code = value.codePointAt(0) ?? 0;
  if (code < 0x20 || code === 0x7f) return value;
  if (ctrl) {
    const upper = value.toUpperCase().codePointAt(0) ?? 0;
    if (upper >= 0x40 && upper <= 0x5f) {
      return String.fromCharCode(upper & 0x1f);
    }
    if (value === "?") return "\x7f";
    if (value === " ") return "\x00";
  }
  return alt ? `\x1b${value}` : value;
}
