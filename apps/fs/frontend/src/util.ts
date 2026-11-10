export const encoder = new TextEncoder();

export const escape = (value: unknown): string =>
  String(value).replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[character] as string,
  );

export function cell(className: string, textValue: string): HTMLSpanElement {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = textValue;
  return node;
}

export function normalizeNewlines(value: unknown): string {
  return String(value).replace(/\r\n?/g, "\n");
}

export function reason(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
