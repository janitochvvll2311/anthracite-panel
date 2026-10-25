/**
 * Escaping for the templates the shell builds as text.
 *
 * The shell and its panels compose `innerHTML` from strings because that is
 * cheaper to read than a tree of `createElement` calls, and every one of those
 * templates carries host-supplied text: a hostname, a plugin's label, a
 * daemon's error message. This is the one place that decides what a dangerous
 * character becomes, so a new template cannot invent a weaker rule.
 */

/**
 * Escapes text for element content and for double-quoted attributes alike.
 *
 * Quotes are escaped too: a value interpolated into `title="${...}"` is the same
 * hazard as one interpolated into an element, and a single function that is safe
 * in both places cannot be used in the wrong one.
 */
export function escape(text: string): string {
  return text.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}
