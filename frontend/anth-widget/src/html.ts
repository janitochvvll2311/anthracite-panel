/**
 * Escaping for the templates the shell builds as text.
 *
 * Every one of those templates carries text the daemon supplied — a hostname,
 * an error message — so a new template cannot be allowed to invent a weaker
 * rule. The same function is safe in element content and in a quoted attribute.
 */
export function escape(text: string): string {
  return text.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case '&':
        return '&amp;'
      case '<':
        return '&lt;'
      case '>':
        return '&gt;'
      case '"':
        return '&quot;'
      default:
        return '&#39;'
    }
  })
}
