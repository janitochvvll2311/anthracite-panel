import hljs from "highlight.js/lib/core";
import hljsBash from "highlight.js/lib/languages/bash";
import hljsCss from "highlight.js/lib/languages/css";
import hljsIni from "highlight.js/lib/languages/ini";
import hljsJavascript from "highlight.js/lib/languages/javascript";
import hljsJson from "highlight.js/lib/languages/json";
import hljsMarkdown from "highlight.js/lib/languages/markdown";
import hljsPlaintext from "highlight.js/lib/languages/plaintext";
import hljsPython from "highlight.js/lib/languages/python";
import hljsRust from "highlight.js/lib/languages/rust";
import hljsTypescript from "highlight.js/lib/languages/typescript";
import { marked } from "marked";

import { extensionOf } from "./format";
import { escape, normalizeNewlines } from "./util";

hljs.registerLanguage("bash", hljsBash);
hljs.registerLanguage("css", hljsCss);
hljs.registerLanguage("ini", hljsIni);
hljs.registerLanguage("javascript", hljsJavascript);
hljs.registerLanguage("json", hljsJson);
hljs.registerLanguage("markdown", hljsMarkdown);
hljs.registerLanguage("plaintext", hljsPlaintext);
hljs.registerLanguage("python", hljsPython);
hljs.registerLanguage("rust", hljsRust);
hljs.registerLanguage("typescript", hljsTypescript);

const HLJS_LANGUAGE_BY_EXTENSION: Record<string, string> = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
  json: "json",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  md: "markdown",
  markdown: "markdown",
  toml: "ini",
  py: "python",
  rs: "rust",
  css: "css",
};

function hljsLanguageFor(path) {
  return HLJS_LANGUAGE_BY_EXTENSION[extensionOf(path)] ?? "plaintext";
}

function highlightCode(value, path) {
  const source = normalizeNewlines(value);
  const language = hljsLanguageFor(path);
  if (!hljs.getLanguage(language)) {
    return `<pre class="fs-code hljs"><code>${escape(source)}</code></pre>`;
  }
  return `<pre class="fs-code hljs"><code class="language-${language}">${hljs.highlight(source, { language }).value}</code></pre>`;
}

marked.use({
  gfm: true,
  breaks: false,
  renderer: {
    code({ text, lang }) {
      const source = normalizeNewlines(text);
      const language = lang
        ? hljsLanguageFor(`file.${lang}`)
        : "plaintext";
      const highlighted = hljs.getLanguage(language)
        ? hljs.highlight(source, { language }).value
        : escape(source);
      return `<pre class="fs-code hljs"><code class="language-${language}">${highlighted}</code></pre>`;
    },
  },
});

export function formattedPreview(value: string, path: string): string {
  const limit = 200_000;
  const truncated = value.length > limit;
  const textValue = truncated ? value.slice(0, limit) : value;
  const extension = extensionOf(path);
  let html;
  if (extension === "md" || extension === "markdown") {
    html = `<div class="fs-markdown">${marked.parse(textValue)}</div>`;
  } else if (extension === "json") {
    let pretty = textValue;
    try {
      pretty = JSON.stringify(JSON.parse(textValue), null, 2);
    } catch {
      // Keep the original text so the preview still shows the bad JSON.
    }
    html = highlightCode(pretty, path);
  } else {
    html = highlightCode(textValue, path);
  }
  return `${html}${truncated ? '<p class="hint">Preview truncated at 200 KB.</p>' : ""}`;
}
