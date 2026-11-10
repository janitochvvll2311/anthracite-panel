export const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
  json: "json",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  md: "markdown",
  markdown: "markdown",
  toml: "toml",
  py: "python",
  rs: "rust",
  css: "css",
};

export const COMPLETION_WORDS: Record<string, string[]> = {
  javascript: [
    "async", "await", "break", "case", "catch", "class", "const", "continue",
    "default", "delete", "do", "else", "export", "extends", "false", "finally",
    "for", "from", "function", "if", "import", "in", "instanceof", "let", "new",
    "null", "of", "return", "static", "super", "switch", "this", "throw", "true",
    "try", "typeof", "undefined", "var", "void", "while", "with", "yield",
  ],
  python: [
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
    "del", "elif", "else", "except", "False", "finally", "for", "from", "global",
    "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass",
    "raise", "return", "True", "try", "while", "with", "yield",
  ],
  rust: [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
    "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while",
  ],
  shell: [
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case",
    "esac", "function", "in", "return", "export", "local", "echo", "cd", "set",
  ],
  toml: ["true", "false"],
  css: [
    "color", "background", "margin", "padding", "display", "font", "border",
    "width", "height", "flex", "grid", "position", "top", "left", "right",
    "bottom", "gap",
  ],
  json: ["true", "false", "null"],
};

export function languageFor(path: string): string {
  const extension = String(path).split(".").pop()?.toLowerCase() ?? "";
  return LANGUAGE_BY_EXTENSION[extension] ?? "plain";
}

export function suggestionsFor(
  path: string,
  textValue: string,
  prefix: string,
): string[] {
  const language = languageFor(path);
  const keywordLanguage = language === "typescript" ? "javascript" : language;
  const words = new Set<string>(COMPLETION_WORDS[keywordLanguage] ?? []);
  if (textValue.length <= 200_000) {
    for (const word of textValue.match(/[A-Za-z_$][A-Za-z0-9_$]*/g) ?? []) {
      if (word.length <= 40) words.add(word);
    }
  }
  const lower = prefix.toLowerCase();
  return [...words]
    .filter((word) => word !== prefix && word.toLowerCase().startsWith(lower))
    .sort((left, right) => left.localeCompare(right))
    .slice(0, 100);
}
