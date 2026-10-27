/**
 * Applies sticky `Ctrl`/`Alt` to one character typed on the soft keyboard.
 *
 * `Ctrl` maps letters and `@[\]^_` to their control codes and `?` to `DEL`,
 * mirroring what xterm does for a physical keyboard. Anything that is not a
 * single printable character is returned untouched.
 */
export function applyStickyModifiers(
  data: string,
  ctrl: boolean,
  alt: boolean,
): string {
  if ((!ctrl && !alt) || data.length !== 1) return data
  const code = data.charCodeAt(0)
  if (code < 0x20 || code === 0x7f) return data
  if (ctrl) {
    const upper = data.toUpperCase().charCodeAt(0)
    if (upper >= 0x40 && upper <= 0x5f) return String.fromCharCode(upper & 0x1f)
    if (data === '?') return '\x7f'
  }
  return alt ? `\x1b${data}` : data
}
