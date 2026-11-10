export const LANGUAGE_LABELS: Record<string, string> = {
  javascript: "JavaScript",
  typescript: "TypeScript",
  json: "JSON",
  shell: "Shell",
  markdown: "Markdown",
  toml: "TOML",
  python: "Python",
  rust: "Rust",
  css: "CSS",
  plain: "Plain text",
};

export function extensionOf(path: string): string {
  const name = String(path).split("/").pop() ?? "";
  const index = name.lastIndexOf(".");
  return index <= 0 ? "" : name.slice(index + 1).toLowerCase();
}

export function languageLabel(language: string): string {
  return LANGUAGE_LABELS[language] ?? "Plain text";
}

export function imageLabel(path: string): string {
  const extension = extensionOf(path).toUpperCase();
  return extension ? `${extension} image` : "Image";
}

export function detectEol(value: string): string {
  const crlf = (value.match(/\r\n/g) || []).length;
  const cr = (value.match(/\r(?!\n)/g) || []).length;
  const lf = (value.match(/(?<!\r)\n/g) || []).length;
  if (crlf && !cr && !lf) return "CRLF";
  if (!crlf && !cr) return "LF";
  return "Mixed";
}

export function splitLines(value: string): string[] {
  return value.split(/\r\n|\r|\n/);
}

export function joinEol(value: string, eol: string): string {
  return splitLines(value).join(eol === "CRLF" ? "\r\n" : "\n");
}

export function bytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
}

export function modified(ms?: number): string {
  if (!ms) return "—";
  const date = new Date(ms);
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}

export function kindLabel(kind: string): string {
  return kind === "dir" ? "directory" : kind === "link" ? "symlink" : "file";
}

export function text(bytes: ArrayLike<number>): string | null {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(bytes));
  } catch {
    return null;
  }
}
