// The demo Files view: a deterministic tree from the fs service.
//
// The module only knows the app's own routes. Every call goes through the
// frame's scoped fetch, so the panel origin and the cookie stay out of reach.

import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  toggleComment,
} from "@codemirror/commands";
import {
  StreamLanguage,
  ensureSyntaxTree,
  indentUnit,
  syntaxTree,
} from "@codemirror/language";
import {
  autocompletion,
  completionKeymap,
  startCompletion,
} from "@codemirror/autocomplete";
import { forEachDiagnostic, linter, lintGutter } from "@codemirror/lint";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { css } from "@codemirror/lang-css";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { oneDark } from "@codemirror/theme-one-dark";
import hljs from "highlight.js/lib/common";
import { marked } from "marked";

const LANGUAGE_BY_EXTENSION = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "javascript",
  tsx: "javascript",
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

const COMPLETION_WORDS = {
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

function languageFor(path) {
  const extension = String(path).split(".").pop()?.toLowerCase() ?? "";
  return LANGUAGE_BY_EXTENSION[extension] ?? "plain";
}

function suggestionsFor(path, textValue, prefix) {
  const words = new Set(COMPLETION_WORDS[languageFor(path)] ?? []);
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

const styles = `
  .fs { position: relative; display: flex; flex-direction: column; gap: 12px; min-height: 380px; color: #e8ebf2; }
  .fs-bar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fs-bar button, .fs-form button, .fs-confirm button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fs-bar button:hover, .fs-form button:hover, .fs-confirm button:hover { border-color: #3a4560; color: #fff; }
  .fs-bar button:disabled { opacity: .4; cursor: default; border-color: #2a3142; color: #667087; }
  .fs-bar .grow { flex: 1; }
  .fs-address {
    flex: 1; min-width: 160px; max-width: 46%; padding: 6px 9px; border-radius: 7px;
    border: 1px solid #2a3142; background: #0f1219; color: #c8d0df;
    font: 12px ui-monospace, monospace;
  }
  .fs-address:focus { outline: 2px solid #7aa2ff66; outline-offset: 1px; border-color: #3a4560; }
  .fs-address:disabled { opacity: .6; }
  .hljs-keyword, .hljs-selector-tag, .hljs-literal { color: #c792ea; }
  .hljs-string, .hljs-template-tag, .hljs-template-variable { color: #c3e88d; }
  .hljs-number, .hljs-symbol, .hljs-bullet { color: #f78c6c; }
  .hljs-comment, .hljs-quote { color: #667087; font-style: italic; }
  .hljs-title, .hljs-section, .hljs-name, .hljs-selector-id { color: #7aa2ff; font-weight: 600; }
  .hljs-attr, .hljs-attribute, .hljs-variable { color: #89ddff; }
  .hljs-built_in, .hljs-type, .hljs-class .hljs-title { color: #ffcb6b; }
  .hljs-meta { color: #8f98ab; }
  .fs-body { display: grid; grid-template-columns: minmax(220px, 1fr) minmax(240px, 1.25fr); gap: 12px; min-height: 300px; }
  .fs-list { list-style: none; margin: 0; padding: 0; border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .fs-row { display: flex; align-items: center; gap: 2px; border-bottom: 1px solid #171c26; }
  .fs-row:last-child { border-bottom: 0; }
  .fs-entry {
    flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; padding: 8px 10px;
    border: 0; background: transparent; color: #c8d0df; font: inherit; font-size: 12.5px;
    text-align: left; cursor: pointer;
  }
  .fs-entry:hover { background: #151a24; color: #fff; }
  .fs-entry .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fs-entry .size { margin-left: auto; color: #667087; font-size: 11px; white-space: nowrap; }
  .file-icon {
    --icon-color: #8f98ab;
    position: relative; display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px; flex: 0 0 22px; color: var(--icon-color);
  }
  .file-icon svg { display: block; width: 22px; height: 22px; }
  .file-icon-badge {
    position: absolute; left: 3px; right: 3px; bottom: 1px;
    padding: 0 1px; border-radius: 2px; background: var(--icon-color);
    color: #0b0e14; font: 700 6.5px/1.1 ui-monospace, monospace;
    letter-spacing: -.4px; text-align: center; overflow: hidden;
  }
  .file-icon.dir, .file-icon.link { --icon-color: #7aa2ff; }
  .file-icon.code { --icon-color: #f7c948; }
  .file-icon.code-strong { --icon-color: #7aa2ff; }
  .file-icon.data { --icon-color: #f78c6c; }
  .file-icon.doc { --icon-color: #7aa2ff; }
  .file-icon.web { --icon-color: #c792ea; }
  .file-icon.shell { --icon-color: #89ddff; }
  .file-icon.config { --icon-color: #4dd0c8; }
  .file-icon.database { --icon-color: #6ee7b7; }
  .file-icon.image { --icon-color: #ff8fbf; }
  .file-icon.audio { --icon-color: #c792ea; }
  .file-icon.video { --icon-color: #ff9e64; }
  .file-icon.archive { --icon-color: #c3a06b; }
  .file-icon.pdf { --icon-color: #ff7d8a; }
  .file-icon.text { --icon-color: #aab4c8; }
  .file-icon.binary { --icon-color: #ff7d8a; }
  .file-icon.unknown { --icon-color: #8f98ab; }
  .fs-tool {
    border: 0; background: transparent; color: #667087; cursor: pointer; font-size: 11px;
    padding: 4px 6px; border-radius: 5px;
  }
  .fs-tool:hover { background: #2b3344; color: #e8ebf2; }
  .fs-tool.delete:hover { color: #ff7d8a; }
  .fs-tool.confirm { border: 1px solid #ff7d8a66; color: #ffb3ba; }
  .fs-new, .fs-rename { display: flex; gap: 6px; flex: 1; }
  .fs-new[hidden], .fs-rename[hidden], .fs-confirm[hidden] { display: none; }
  .fs-new input, .fs-rename input {
    flex: 1; min-width: 120px; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12px;
  }
  .fs-new select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12px;
  }
  .fs-new input[hidden] { display: none; }
  .fs-confirm {
    display: flex; align-items: center; gap: 8px; padding: 7px 10px; border: 1px dashed #ff7d8a66;
    border-radius: 8px; background: #1d1218; color: #ffb3ba; font-size: 12px;
  }
  .fs-confirm button { padding: 4px 8px; font-size: 11px; }
  .fs-preview {
    border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 10px 12px;
    overflow: auto; min-height: 300px; display: flex; flex-direction: column; gap: 8px;
  }
  .fs-preview .pane { display: flex; flex-direction: column; gap: 8px; min-height: 0; flex: 1; }
  .fs-meta { color: #8f98ab; font-size: 11px; display: flex; gap: 10px; flex-wrap: wrap; }
  .fs-meta b { color: #c8d0df; font-weight: 500; }
  .fs-preview pre {
    margin: 0; flex: 1; font: 12px/1.55 ui-monospace, monospace; white-space: pre-wrap;
    overflow-wrap: anywhere; color: #c8d0df;
  }
  .fs-preview img { max-width: 100%; max-height: 280px; object-fit: contain; border-radius: 6px; }
  .fs-preview .hint { color: #667087; font-size: 12px; margin: 0; }
  .fs-preview-head { display: none; align-items: center; gap: 8px; }
  .fs-preview-head button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .fs-preview-head button:hover { border-color: #3a4560; color: #fff; }
  .fs-preview-head strong {
    min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 13px; font-weight: 600;
  }
  .fs-formatted { flex: 1; min-height: 0; overflow: auto; border: 1px solid #202632; border-radius: 8px; background: #0d1016; padding: 10px 12px; }
  .fs-code { margin: 0; font: 12px/1.6 ui-monospace, monospace; white-space: pre; color: #c8d0df; tab-size: 4; }
  .fs-code code { background: transparent; border: 0; padding: 0; font: inherit; color: inherit; white-space: pre; }
  .fs-markdown { font-size: 13px; line-height: 1.6; color: #d5dbea; }
  .fs-markdown h1, .fs-markdown h2, .fs-markdown h3, .fs-markdown h4, .fs-markdown h5, .fs-markdown h6 { margin: 14px 0 6px; color: #fff; line-height: 1.25; }
  .fs-markdown h1 { font-size: 22px; }
  .fs-markdown h2 { font-size: 18px; }
  .fs-markdown h3 { font-size: 15px; }
  .fs-markdown p { margin: 0 0 8px; }
  .fs-markdown ul, .fs-markdown ol { margin: 0 0 8px; padding-left: 22px; }
  .fs-markdown li { margin: 2px 0; }
  .fs-markdown blockquote { margin: 8px 0; padding: 4px 10px; border-left: 3px solid #3a4560; color: #aab4c8; }
  .fs-markdown code { background: #1b2130; border: 1px solid #2a3142; border-radius: 4px; padding: 0 4px; font: 12px ui-monospace, monospace; color: #c3e88d; }
  .fs-markdown .fs-code code { background: transparent; border: 0; padding: 0; }
  .fs-markdown hr { border: 0; border-top: 1px solid #2a3142; margin: 12px 0; }
  .md-link { color: #7aa2ff; text-decoration: underline; text-decoration-style: dotted; }
  .fs-actions { display: flex; gap: 5px; flex-wrap: wrap; }
  .fs-actions button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .fs-actions button:hover { border-color: #3a4560; color: #fff; }
  .fs-actions button:disabled { opacity: .4; cursor: default; }
  .fs-more { display: flex; justify-content: center; }
  .fs-more button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fs-more button:hover { border-color: #3a4560; color: #fff; }
  .fs-drop {
    position: absolute; inset: 0; z-index: 30; display: flex; align-items: center;
    justify-content: center; border: 2px dashed #7aa2ff; border-radius: 10px;
    background: #0b1220d9; color: #cfe0ff; font-size: 14px; pointer-events: none;
  }
  .fs-drop[hidden] { display: none; }
  .fs-row.drop-target { outline: 2px solid #7aa2ff66; outline-offset: -2px; background: #13203a; }
  .fs-queue { border: 1px solid #202632; border-radius: 9px; background: #0b0e14; overflow: hidden; }
  .fs-queue[hidden] { display: none; }
  .fs-queue header {
    display: flex; align-items: center; gap: 8px; padding: 6px 10px;
    border-bottom: 1px solid #202632; background: #12161f;
  }
  .fs-queue header strong { font-size: 12px; color: #fff; }
  .fs-queue header button {
    margin-left: auto; padding: 3px 7px; border-radius: 5px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 10.5px; cursor: pointer;
  }
  .fs-queue header button:hover { border-color: #3a4560; color: #fff; }
  .fs-queue ul { list-style: none; margin: 0; padding: 0; max-height: 180px; overflow: auto; }
  .fs-queue-item {
    display: grid; grid-template-columns: 16px minmax(0, 1fr) auto; gap: 2px 8px;
    align-items: center; padding: 6px 10px; border-bottom: 1px solid #171c26;
    font-size: 11.5px;
  }
  .fs-queue-item:last-child { border-bottom: 0; }
  .fs-queue-dir { color: #7aa2ff; font-size: 12px; }
  .fs-queue-main { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .fs-queue-name { color: #c8d0df; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fs-queue-meta { color: #667087; font-size: 10.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fs-queue-progress { grid-column: 2 / 3; height: 3px; border-radius: 2px; background: #1b2130; overflow: hidden; }
  .fs-queue-progress span { display: block; height: 100%; background: #7aa2ff; transition: width .2s; }
  .fs-queue-actions { grid-column: 3; grid-row: 1 / span 2; display: flex; gap: 3px; }
  .fs-queue-actions .fs-tool { padding: 2px 5px; font-size: 10.5px; }
  .fs-queue-done { color: #6ee7b7; font-size: 12px; }
  .editor-app {
    display: grid; grid-template-rows: 38px minmax(0, 1fr) 28px;
    min-height: 480px; height: 100%; border: 1px solid #202632; border-radius: 10px;
    background: #0d1016; color: #e8ebf2; overflow: hidden;
  }
  .editor-toolbar, .editor-status {
    display: flex; align-items: center; gap: 8px; padding: 0 8px;
    background: #12161f; min-width: 0;
  }
  .editor-toolbar { border-bottom: 1px solid #202634; }
  .editor-status { border-top: 1px solid #202634; color: #8f98ab; font: 11px ui-monospace, monospace; white-space: nowrap; overflow: hidden; }
  .editor-toolbar .grow, .editor-status .grow { flex: 1; }
  .editor-toolbar button {
    padding: 5px 9px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11.5px; cursor: pointer;
  }
  .editor-toolbar button:hover { border-color: #3a4560; color: #fff; }
  .editor-toolbar button.editor-tree-toggle {
    width: 28px; padding: 0; display: inline-flex; align-items: center; justify-content: center;
  }
  .editor-tree-toggle svg { width: 15px; height: 15px; display: block; }
  .editor-path {
    flex: 1; min-width: 120px; padding: 5px 8px; border-radius: 6px;
    border: 1px solid #2a3142; background: #0f1219; color: #c8d0df;
    font: 12px ui-monospace, monospace;
  }
  .editor-path:focus { outline: 2px solid #7aa2ff66; outline-offset: 1px; border-color: #3a4560; }
  .editor-info { color: #8f98ab; font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 34%; }
  .editor-info.bad { color: #ffb3ba; }
  .editor-tabs {
    display: flex; align-items: stretch; gap: 1px; overflow-x: auto;
    flex: 0 0 34px; border-bottom: 1px solid #202634; background: #0f1219;
  }
  .editor-tab { display: flex; align-items: center; border-right: 1px solid #202634; background: #12161f; }
  .editor-tab.active { background: #1b2130; box-shadow: inset 0 2px 0 #7aa2ff; }
  .editor-tab-open {
    border: 0; background: transparent; color: #c8d0df; padding: 0 8px; height: 33px;
    font: 12px ui-monospace, monospace; cursor: pointer; display: flex; align-items: center; gap: 6px;
    white-space: nowrap;
  }
  .editor-tab.active .editor-tab-open { color: #fff; }
  .editor-tab-icon { display: inline-flex; flex: 0 0 14px; }
  .editor-tab-icon .file-icon { width: 14px; height: 14px; flex-basis: 14px; }
  .editor-tab-icon .file-icon svg { width: 14px; height: 14px; }
  .editor-tab-icon .file-icon-badge { left: 2px; right: 2px; bottom: 0; font-size: 5px; }
  .editor-dirty { color: #f78c6c; font-size: 8px; }
  .editor-tab-close {
    border: 0; background: transparent; color: #667087; width: 22px; height: 33px;
    cursor: pointer; font-size: 14px;
  }
  .editor-tab-close:hover { color: #ff8f9a; }
  .editor-main { display: flex; min-height: 0; position: relative; }
  .editor-tree {
    width: 212px; min-width: 150px; overflow: auto; border-right: 1px solid #202634;
    padding: 6px 0; background: #0b0e14;
  }
  .tree-hidden .editor-tree { display: none; }
  .tree-hidden .editor-tree-backdrop { display: none !important; }
  .editor-tree-backdrop {
    position: absolute; inset: 0; z-index: 4; background: #00000073;
    border: 0; padding: 0; margin: 0;
  }
  .editor-tree-backdrop[hidden] { display: none; }
  .editor-tree-head { display: flex; align-items: center; gap: 4px; padding: 0 8px 6px; }
  .editor-tree-root, .editor-tree-up {
    border: 0; background: transparent; color: #8f98ab; font: 11.5px ui-monospace, monospace;
    text-align: left; padding: 3px 4px; border-radius: 4px; cursor: pointer;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .editor-tree-root { display: flex; align-items: center; gap: 5px; }
  .editor-tree-root .file-icon { width: 18px; height: 18px; flex-basis: 18px; }
  .editor-tree-root .file-icon svg { width: 18px; height: 18px; }
  .editor-tree-root:hover, .editor-tree-up:hover { background: #1b2130; color: #fff; }
  .editor-tree-row { display: flex; min-width: 0; }
  .editor-tree-label {
    border: 0; background: transparent; color: #c8d0df; padding: 3px 6px;
    font: 12px ui-monospace, monospace; text-align: left; cursor: pointer; width: 100%;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border-radius: 4px;
    display: flex; align-items: center; gap: 5px;
  }
  .editor-tree-label .file-icon { width: 18px; height: 18px; flex-basis: 18px; }
  .editor-tree-label .file-icon svg { width: 18px; height: 18px; }
  .editor-tree-label .file-icon-badge { left: 2px; right: 2px; bottom: 0; font-size: 6px; }
  .editor-tree-arrow { width: 10px; flex: 0 0 10px; color: #8f98ab; text-align: center; }
  .editor-tree-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .editor-tree-label:hover { background: #151a24; color: #fff; }
  .editor-tree-label.dir { color: #aab4c8; }
  .editor-tree-label.link { color: #7aa2ff; }
  .editor-tree-label.active { background: #1b2a4a; color: #fff; }
  .editor-workspace { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .editor-empty { color: #667087; font-size: 12px; padding: 18px; }
  .editor-content { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
  .editor-content[hidden] { display: none; }
  .editor-content .cm-editor { height: 100%; }
  .editor-content .cm-scroller { overflow: auto; font: 12px/1.6 ui-monospace, monospace; }
  .editor-content .cm-content { padding: 0; }
  .editor-content .cm-gutters { background: #0b0e14; border-right: 1px solid #171c26; }
  .editor-content .cm-activeLine { background: #ffffff08; }
  .editor-content .cm-activeLineGutter { background: #1b2130; }
  .editor-content .cm-tooltip-autocomplete {
    border: 1px solid #333d54; border-radius: 8px; background: #151a24; color: #c8d0df;
    box-shadow: 0 16px 44px #00000099;
  }
  .editor-content .cm-tooltip-autocomplete > ul > li[aria-selected] { background: #1b2a4a; color: #fff; }
  .editor-content .cm-lintRange-error { text-decoration: underline wavy #ff7d8a; text-underline-offset: 2px; }
  .editor-content .cm-lint-marker-error { content: "●"; color: #ff7d8a; }
  .editor-image {
    flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center;
    overflow: auto; padding: 12px;
    background: repeating-conic-gradient(#0f1219 0% 25%, #0b0e14 0% 50%) 50% / 20px 20px;
  }
  .editor-image[hidden] { display: none; }
  .editor-image img {
    max-width: 100%; max-height: 100%; object-fit: contain;
    background: #0b0e14; box-shadow: 0 12px 44px #00000073;
  }
  .editor-image .hint { color: #667087; font-size: 12px; }
  .editor-eol-label, .editor-indent-label { display: flex; align-items: center; gap: 4px; }
  .editor-eol-label select, .editor-indent-label select {
    background: #0f1219; color: #c8d0df; border: 1px solid #2a3142; border-radius: 4px;
    font: 11px ui-monospace, monospace; padding: 1px 2px;
  }
  .editor-error { overflow: hidden; text-overflow: ellipsis; }
  .editor-error.bad { color: #ffb3ba; }
  .editor-language { color: #7aa2ff; }
  .editor-tree-close {
    display: none; margin-left: auto; border: 0; background: transparent; color: #8f98ab;
    width: 24px; height: 24px; cursor: pointer; border-radius: 4px; font-size: 15px;
  }
  .editor-tree-close:hover { background: #1b2130; color: #fff; }
  .editor-dialog {
    width: min(520px, calc(100vw - 32px)); padding: 0; border: 1px solid #333d54;
    border-radius: 12px; background: #151a24; color: #e8ebf2;
    box-shadow: 0 30px 90px #000000b3;
  }
  .editor-dialog::backdrop { background: #00000080; }
  .editor-dialog-card { display: flex; flex-direction: column; gap: 10px; padding: 16px 18px; }
  .editor-dialog-card strong { font-size: 14px; font-weight: 600; }
  .editor-dialog-card p { margin: 0; color: #aab4c8; font-size: 12px; }
  .editor-dialog-card ul {
    margin: 0; padding-left: 18px; max-height: 160px; overflow: auto;
    color: #c8d0df; font: 12px ui-monospace, monospace;
  }
  .editor-dialog-error { color: #ffb3ba; min-height: 16px; }
  .editor-dialog-actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 4px; }
  .editor-dialog-actions button {
    padding: 6px 10px; border-radius: 6px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .editor-dialog-actions button:hover { border-color: #3a4560; color: #fff; }
  .editor-dialog-actions button[data-dialog-save] {
    background: #24406e; border-color: #3f63a0; color: #fff;
  }
  .editor-dialog-actions button:disabled { opacity: .5; cursor: default; }
  @media (max-width: 700px) {
    .fs { min-height: 0; }
    .fs-body { grid-template-columns: 1fr; min-height: 0; }
    .fs-preview { display: none; }
    .fs.preview-open .fs-list { display: none; }
    .fs.preview-open .fs-preview {
      display: flex; position: absolute; inset: 0; z-index: 20; margin: 0;
      border: 0; border-radius: 0; padding: 12px; background: #0d1016;
      min-height: 0; overflow: auto;
    }
    .fs-preview-head { display: flex; }
    .fs.preview-open .fs-preview img { max-height: calc(100vh - 160px); }
  }
  @media (max-width: 700px) {
    .editor-tree {
      position: absolute; top: 0; bottom: 0; left: 0; width: min(78%, 260px);
      z-index: 5; border-right: 1px solid #333d54; box-shadow: 0 18px 50px #000000a6;
    }
    .editor-tree-close { display: inline-flex; align-items: center; justify-content: center; }
    .editor-tree-backdrop { display: block; }
  }
  .fs-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .fs-status.bad { color: #ffb3ba; }
`;

const IMAGES = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  bmp: "image/bmp",
  ico: "image/x-icon",
  svg: "image/svg+xml",
};

const encoder = new TextEncoder();

const escape = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[character],
  );

const MIME_BY_EXTENSION = {
  js: "text/javascript",
  mjs: "text/javascript",
  cjs: "text/javascript",
  jsx: "text/javascript",
  ts: "text/typescript",
  tsx: "text/typescript",
  json: "application/json",
  md: "text/markdown",
  markdown: "text/markdown",
  txt: "text/plain",
  log: "text/plain",
  html: "text/html",
  htm: "text/html",
  css: "text/css",
  scss: "text/x-scss",
  sh: "application/x-sh",
  bash: "application/x-sh",
  zsh: "application/x-sh",
  py: "text/x-python",
  rs: "text/x-rust",
  toml: "application/toml",
  yaml: "application/yaml",
  yml: "application/yaml",
  ini: "text/plain",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  avif: "image/avif",
  bmp: "image/bmp",
  ico: "image/x-icon",
  pdf: "application/pdf",
  zip: "application/zip",
  gz: "application/gzip",
  tgz: "application/gzip",
  tar: "application/x-tar",
  "7z": "application/x-7z-compressed",
  rar: "application/vnd.rar",
  bz2: "application/x-bzip2",
  tbz2: "application/x-bzip2",
  xz: "application/x-xz",
  zst: "application/zstd",
  mp3: "audio/mpeg",
  wav: "audio/wav",
  ogg: "audio/ogg",
  flac: "audio/flac",
  mp4: "video/mp4",
  webm: "video/webm",
  mkv: "video/x-matroska",
  mov: "video/quicktime",
  wasm: "application/wasm",
  db: "application/x-sqlite3",
  sqlite: "application/vnd.sqlite3",
  sqlite3: "application/vnd.sqlite3",
  db3: "application/x-sqlite3",
  sql: "text/x-sql",
  duckdb: "application/x-duckdb",
  mdb: "application/x-msaccess",
  accdb: "application/x-msaccess",
  dbf: "application/dbase",
};

const ICON_LABELS = {
  js: "JS",
  mjs: "JS",
  cjs: "JS",
  jsx: "JSX",
  ts: "TS",
  tsx: "TSX",
  json: "JSON",
  md: "MD",
  markdown: "MD",
  txt: "TXT",
  log: "LOG",
  html: "HTML",
  htm: "HTML",
  css: "CSS",
  scss: "SCSS",
  sh: "SH",
  bash: "SH",
  zsh: "SH",
  py: "PY",
  rs: "RS",
  toml: "TOML",
  yaml: "YML",
  yml: "YML",
  ini: "INI",
  png: "PNG",
  jpg: "JPG",
  jpeg: "JPG",
  gif: "GIF",
  webp: "WEBP",
  svg: "SVG",
  avif: "AVIF",
  bmp: "BMP",
  ico: "ICO",
  pdf: "PDF",
  zip: "ZIP",
  gz: "GZ",
  tgz: "TGZ",
  tar: "TAR",
  "7z": "7Z",
  rar: "RAR",
  bz2: "BZ2",
  tbz2: "TBZ2",
  xz: "XZ",
  zst: "ZST",
  mp3: "MP3",
  wav: "WAV",
  ogg: "OGG",
  flac: "FLAC",
  mp4: "MP4",
  webm: "WEBM",
  mkv: "MKV",
  mov: "MOV",
  wasm: "WASM",
  db: "DB",
  sqlite: "SQL",
  sqlite3: "SQL",
  db3: "DB",
  sql: "SQL",
  duckdb: "DUCK",
  mdb: "MDB",
  accdb: "ACCD",
  dbf: "DBF",
};

const FOLDER_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M2.5 5.5A1.5 1.5 0 0 1 4 4h4l1.7 2h6.3A1.5 1.5 0 0 1 17.5 7.5v7A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z" fill="var(--icon-color)" fill-opacity=".18" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/></svg>';
const FILE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5 2.5h6.5L15 6v11.5H5z" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/><path d="M11.5 2.5V6H15" fill="none" stroke="var(--icon-color)" stroke-width="1.1" stroke-linejoin="round"/></svg>';
const IMAGE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="3.5" width="15" height="12" rx="1.5" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><circle cx="7" cy="7.5" r="1.3" fill="var(--icon-color)"/><path d="m4.5 13.5 4-4 2.5 2.5 2-2 2.5 2.5" fill="none" stroke="var(--icon-color)" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const VIDEO_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="4" width="15" height="11.5" rx="1.5" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M8.2 7.2v5.1l4.2-2.55z" fill="var(--icon-color)"/></svg>';
const AUDIO_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M8 13.2V6.1l7-1.8v2.4L10 8v6.2" fill="none" stroke="var(--icon-color)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><circle cx="6.1" cy="14" r="1.9" fill="var(--icon-color)" fill-opacity=".22" stroke="var(--icon-color)" stroke-width="1.1"/><circle cx="13.1" cy="12.6" r="1.9" fill="var(--icon-color)" fill-opacity=".22" stroke="var(--icon-color)" stroke-width="1.1"/></svg>';
const DATABASE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><ellipse cx="10" cy="5" rx="6" ry="2.5" fill="var(--icon-color)" fill-opacity=".18" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M4 5v9.5c0 1.4 2.7 2.5 6 2.5s6-1.1 6-2.5V5" fill="none" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M4 9.5c0 1.4 2.7 2.5 6 2.5s6-1.1 6-2.5" fill="none" stroke="var(--icon-color)" stroke-width="1.1"/></svg>';
const ARCHIVE_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3.5" width="14" height="13" rx="1.8" fill="var(--icon-color)" fill-opacity=".14" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M10 3.5v10" stroke="var(--icon-color)" stroke-width="1.1"/><path d="M8.4 5.5h3.2M8.4 8h3.2M8.4 10.5h3.2" stroke="var(--icon-color)" stroke-width="1.05" stroke-linecap="round"/><path d="M9 13.5h2v2.2H9z" fill="var(--icon-color)"/></svg>';
const LINK_ICON =
  '<svg viewBox="0 0 20 20" aria-hidden="true" fill="none" stroke="var(--icon-color)" stroke-width="1.3" stroke-linecap="round"><path d="m8.2 11.8 3.6-3.6"/><path d="M6.8 9.9 5.4 11.3a3 3 0 0 0 4.2 4.2l1.4-1.4"/><path d="m13.2 10.1 1.4-1.4a3 3 0 0 0-4.2-4.2L9 5.9"/></svg>';

function mimeFor(path) {
  return MIME_BY_EXTENSION[extensionOf(path)] ?? "application/octet-stream";
}

function iconLabel(path) {
  const extension = extensionOf(path);
  if (!extension) return "FILE";
  return ICON_LABELS[extension] ?? extension.toUpperCase().slice(0, 4);
}

function iconCategory(mime, extension) {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("audio/")) return "audio";
  if (mime.startsWith("video/")) return "video";
  if (mime === "application/pdf") return "pdf";
  if (
    [
      "zip",
      "gzip",
      "x-tar",
      "x-7z-compressed",
      "vnd.rar",
      "x-bzip2",
      "x-xz",
      "zstd",
    ].some((part) => mime.includes(part))
  ) {
    return "archive";
  }
  if (mime === "application/wasm") return "binary";
  if (
    [
      "db",
      "sqlite",
      "sqlite3",
      "db3",
      "sql",
      "duckdb",
      "mdb",
      "accdb",
      "dbf",
    ].includes(extension)
  ) {
    return "database";
  }
  if (
    mime === "application/json" ||
    mime.endsWith("+json") ||
    mime.includes("xml")
  ) {
    return "data";
  }
  if (["js", "mjs", "cjs", "jsx", "py", "rs"].includes(extension)) {
    return "code";
  }
  if (["ts", "tsx"].includes(extension)) return "code-strong";
  if (["md", "markdown"].includes(extension)) return "doc";
  if (["html", "htm", "css", "scss"].includes(extension)) return "web";
  if (["sh", "bash", "zsh"].includes(extension)) return "shell";
  if (["toml", "yaml", "yml", "ini"].includes(extension)) return "config";
  if (mime.startsWith("text/")) return "text";
  return "unknown";
}

function iconMarkup(kind, path) {
  if (kind === "dir") {
    return `<span class="file-icon dir" title="Directory">${FOLDER_ICON}</span>`;
  }
  if (kind === "link") {
    return `<span class="file-icon link" title="Symbolic link">${LINK_ICON}</span>`;
  }
  const mime = mimeFor(path);
  const extension = extensionOf(path);
  const category = iconCategory(mime, extension);
  const shape =
    category === "image"
      ? IMAGE_ICON
      : category === "video"
        ? VIDEO_ICON
        : category === "audio"
          ? AUDIO_ICON
          : category === "database"
            ? DATABASE_ICON
            : category === "archive"
              ? ARCHIVE_ICON
              : FILE_ICON;
  return `<span class="file-icon ${category}" title="${escape(mime)}">${shape}<span class="file-icon-badge">${escape(iconLabel(path))}</span></span>`;
}

function join(path, name) {
  return path === "/" ? `/${name}` : `${path}/${name}`;
}

function parent(path) {
  if (path === "/") return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}

function baseName(path) {
  return path === "/" ? "/" : path.substring(path.lastIndexOf("/") + 1);
}

function normalizePath(input) {
  let path = String(input).trim();
  if (!path) path = "/";
  if (!path.startsWith("/")) path = `/${path}`;
  const parts = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!parts.length) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return `/${parts.join("/")}`;
}

const LANGUAGE_LABELS = {
  javascript: "JavaScript",
  json: "JSON",
  shell: "Shell",
  markdown: "Markdown",
  toml: "TOML",
  python: "Python",
  rust: "Rust",
  css: "CSS",
  plain: "Plain text",
};

function extensionOf(path) {
  const name = String(path).split("/").pop() ?? "";
  const index = name.lastIndexOf(".");
  return index <= 0 ? "" : name.slice(index + 1).toLowerCase();
}

function languageLabel(language) {
  return LANGUAGE_LABELS[language] ?? "Plain text";
}

function imageLabel(path) {
  const extension = extensionOf(path).toUpperCase();
  return extension ? `${extension} image` : "Image";
}

function detectEol(value) {
  const crlf = (value.match(/\r\n/g) || []).length;
  const cr = (value.match(/\r(?!\n)/g) || []).length;
  const lf = (value.match(/(?<!\r)\n/g) || []).length;
  if (crlf && !cr && !lf) return "CRLF";
  if (!crlf && !cr) return "LF";
  return "Mixed";
}

function splitLines(value) {
  return value.split(/\r\n|\r|\n/);
}

function joinEol(value, eol) {
  return splitLines(value).join(eol === "CRLF" ? "\r\n" : "\n");
}

function bytes(size) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
}

function modified(ms) {
  if (!ms) return "—";
  const date = new Date(ms);
  return `${date.toISOString().slice(0, 10)} ${date.toISOString().slice(11, 16)}`;
}

function kindLabel(kind) {
  return kind === "dir" ? "directory" : kind === "link" ? "symlink" : "file";
}

function text(bytes) {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(bytes));
  } catch {
    return null;
  }
}

async function call(context, route, init) {
  const { headers, ...rest } = init ?? {};
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    { ...rest, headers: { accept: "application/json", ...(headers ?? {}) } },
  );
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message =
      value && typeof value === "object" && value.error
        ? `${value.error.code}: ${value.error.message}`
        : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

function cell(className, textValue) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = textValue;
  return node;
}

function normalizeNewlines(value) {
  return String(value).replace(/\r\n?/g, "\n");
}

const HLJS_LANGUAGE_BY_EXTENSION = {
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
      const language =
        lang && hljs.getLanguage(lang) ? lang : "plaintext";
      const highlighted = hljs.getLanguage(language)
        ? hljs.highlight(source, { language }).value
        : escape(source);
      return `<pre class="fs-code hljs"><code class="language-${language}">${highlighted}</code></pre>`;
    },
  },
});

function formattedPreview(value, path) {
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

const indentCompartment = new Compartment();

function indentExtensions(settings) {
  return [
    EditorState.tabSize.of(settings.size),
    indentUnit.of(settings.useTabs ? "\t" : " ".repeat(settings.size)),
  ];
}

function languageExtension(path) {
  const extension = extensionOf(path);
  if (["js", "mjs", "cjs"].includes(extension)) return javascript();
  if (extension === "jsx") return javascript({ jsx: true });
  if (extension === "ts") return javascript({ typescript: true });
  if (extension === "tsx") return javascript({ typescript: true, jsx: true });
  if (extension === "json") return json();
  if (extension === "css") return css();
  if (extension === "py") return python();
  if (extension === "rs") return rust();
  if (extension === "md" || extension === "markdown") return markdown();
  if (["sh", "bash", "zsh"].includes(extension)) {
    return StreamLanguage.define(shell);
  }
  if (extension === "toml") return StreamLanguage.define(toml);
  return [];
}

function completionSource(path) {
  return (context) => {
    const word = context.matchBefore(/[A-Za-z_$][A-Za-z0-9_$]*/);
    if (!word && !context.explicit) return null;
    const from = word ? word.from : context.pos;
    const prefix = word ? context.state.sliceDoc(word.from, context.pos) : "";
    const items = suggestionsFor(path, context.state.doc.toString(), prefix);
    if (!items.length) return null;
    return {
      from,
      options: items.map((label) => ({ label, type: "keyword" })),
    };
  };
}

function syntaxDiagnostics(view) {
  const diagnostics = [];
  const tree =
    ensureSyntaxTree(view.state, view.state.doc.length, 500) ??
    syntaxTree(view.state);
  tree.iterate({
    enter(node) {
      if (!node.type.isError) return;
      const from = node.from;
      const to = Math.max(node.to, from + 1);
      diagnostics.push({
        from,
        to: Math.min(to, view.state.doc.length),
        severity: "error",
        message: "Syntax error",
      });
      if (diagnostics.length >= 20) return false;
    },
  });
  return diagnostics;
}

let editorBridge = null;

export function onParams(params) {
  if (!params || typeof params !== "object") return;
  if (typeof params.path === "string") {
    void editorBridge?.openPath(params.path);
  }
}

export function beforeClose() {
  return editorBridge?.confirmClose?.() ?? true;
}

function mountEditorView(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "editor-app";
  box.innerHTML = `
    <header class="editor-toolbar">
      <button type="button" class="editor-tree-toggle" data-tree-toggle aria-label="Toggle file tree" aria-expanded="true" title="Toggle file tree">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M2.5 4h11M2.5 8h11M2.5 12h7"/></svg>
      </button>
      <input class="editor-path" aria-label="Path" spellcheck="false" autocomplete="off" />
      <button type="button" data-open>Open</button>
      <button type="button" data-save>Save</button>
      <span class="editor-info" data-info></span>
      <span class="grow"></span>
      <button type="button" data-refresh>Refresh tree</button>
    </header>
    <div class="editor-main">
      <aside class="editor-tree" aria-label="Files"></aside>
      <div class="editor-tree-backdrop" data-tree-backdrop hidden></div>
      <section class="editor-workspace">
        <div class="editor-tabs" role="tablist" aria-label="Open files"></div>
        <div class="editor-empty">Open a file from the tree or enter a path.</div>
        <div class="editor-content" hidden></div>
        <div class="editor-image" data-image hidden>
          <p class="hint" data-image-empty>Loading image…</p>
          <img data-image-preview alt="" />
        </div>
      </section>
    </div>
    <footer class="editor-status">
      <span class="editor-language" data-language>Plain text</span>
      <label class="editor-eol-label">EOL
        <select data-eol>
          <option value="LF">LF</option>
          <option value="CRLF">CRLF</option>
          <option value="Mixed" hidden>Mixed</option>
        </select>
      </label>
      <label class="editor-indent-label">Indent
        <select data-indent>
          <option value="2">2 spaces</option>
          <option value="4">4 spaces</option>
          <option value="8">8 spaces</option>
          <option value="tab">Tab</option>
        </select>
      </label>
      <span class="editor-error" data-error></span>
      <span class="grow"></span>
      <span data-position>Ln 1, Col 1</span>
      <span data-selection>Sel 0</span>
    </footer>
    <dialog class="editor-dialog" data-close-dialog>
      <div class="editor-dialog-card">
        <strong data-dialog-title>Unsaved changes</strong>
        <p data-dialog-text>This file has unsaved changes.</p>
        <ul data-dialog-files></ul>
        <p class="editor-dialog-error" data-dialog-error></p>
        <div class="editor-dialog-actions">
          <button type="button" data-dialog-save>Save</button>
          <button type="button" data-dialog-discard>Discard</button>
          <button type="button" data-dialog-cancel>Cancel</button>
        </div>
      </div>
    </dialog>
  `;
  root.append(box);

  const pathInput = box.querySelector(".editor-path");
  const tabsElement = box.querySelector(".editor-tabs");
  const treeElement = box.querySelector(".editor-tree");
  const content = box.querySelector(".editor-content");
  const imageWrap = box.querySelector("[data-image]");
  const imagePreview = box.querySelector("[data-image-preview]");
  const imageEmpty = box.querySelector("[data-image-empty]");
  const empty = box.querySelector(".editor-empty");
  const info = box.querySelector("[data-info]");
  const language = box.querySelector("[data-language]");
  const eolSelect = box.querySelector("[data-eol]");
  const eolMixed = eolSelect.querySelector('option[value="Mixed"]');
  const indentSelect = box.querySelector("[data-indent]");
  const errorElement = box.querySelector("[data-error]");
  const position = box.querySelector("[data-position]");
  const selection = box.querySelector("[data-selection]");
  const saveButton = box.querySelector("[data-save]");
  const treeToggle = box.querySelector("[data-tree-toggle]");
  const treeBackdrop = box.querySelector("[data-tree-backdrop]");
  const dialog = box.querySelector("[data-close-dialog]");
  const dialogTitle = dialog.querySelector("[data-dialog-title]");
  const dialogText = dialog.querySelector("[data-dialog-text]");
  const dialogFiles = dialog.querySelector("[data-dialog-files]");
  const dialogError = dialog.querySelector("[data-dialog-error]");
  const dialogSave = dialog.querySelector("[data-dialog-save]");
  const dialogDiscard = dialog.querySelector("[data-dialog-discard]");
  const dialogCancel = dialog.querySelector("[data-dialog-cancel]");
  const mobileQuery = window.matchMedia("(max-width: 700px)");

  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const tabs = [];
  let active = -1;
  let view = null;
  let mountedTab = null;
  let disposed = false;
  let switching = false;
  let treeRoot = "/";
  let treeOpen = !mobileQuery.matches;
  let indentSetting = { size: 4, useTabs: false };
  let dialogTabs = [];
  let dialogResolve = null;
  let dialogPromise = null;
  let dialogBusy = false;
  const treeData = new Map();

  const activeTab = () => (active >= 0 ? tabs[active] : null);

  function setInfo(message, bad = false) {
    info.textContent = message ?? "";
    info.className = bad ? "editor-info bad" : "editor-info";
  }

  function indentLabel() {
    return indentSetting.useTabs
      ? `Tab (${indentSetting.size})`
      : `${indentSetting.size} spaces`;
  }

  function applyTreeState(publish = false) {
    const mobile = mobileQuery.matches;
    box.classList.toggle("tree-hidden", !treeOpen);
    treeBackdrop.hidden = !(treeOpen && mobile);
    treeToggle.setAttribute("aria-expanded", String(treeOpen));
    treeToggle.title = treeOpen ? "Hide file tree" : "Show file tree";
    if (publish) publishSession();
  }

  function setTreeOpen(open, publish = true) {
    treeOpen = open;
    applyTreeState(publish);
  }

  function publishSession() {
    if (typeof context.updateParams !== "function") return;
    context.updateParams({
      session: {
        active: activeTab()?.path ?? null,
        tree: treeOpen,
        indent: { ...indentSetting },
        tabs: tabs.map((tab) =>
          tab.image
            ? {
                path: tab.path,
                image: true,
                mime: tab.mime,
                dirty: false,
              }
            : {
                path: tab.path,
                text: tab.state.doc.toString(),
                eol: tab.eol,
                dirty: tab.dirty,
              },
        ),
      },
    });
  }

  function renderTabs() {
    tabsElement.replaceChildren();
    tabs.forEach((tab, index) => {
      const node = document.createElement("div");
      node.className = `editor-tab${index === active ? " active" : ""}`;
      const open = document.createElement("button");
      open.type = "button";
      open.className = "editor-tab-open";
      open.title = tab.path;
      const tabIcon = document.createElement("span");
      tabIcon.className = "editor-tab-icon";
      tabIcon.innerHTML = iconMarkup("file", tab.path);
      open.append(tabIcon, document.createTextNode(baseName(tab.path)));
      if (tab.dirty) {
        const dot = document.createElement("span");
        dot.className = "editor-dirty";
        dot.textContent = "●";
        dot.title = "Unsaved changes";
        open.append(dot);
      }
      open.addEventListener("click", () => activate(index));
      const close = document.createElement("button");
      close.type = "button";
      close.className = "editor-tab-close";
      close.textContent = "×";
      close.title = `Close ${tab.path}`;
      close.addEventListener("click", (event) => {
        event.stopPropagation();
        void requestCloseTab(index);
      });
      node.append(open, close);
      tabsElement.append(node);
    });
  }

  function refreshStatus() {
    const tab = activeTab();
    if (!tab) {
      language.textContent = "Plain text";
      eolSelect.disabled = true;
      eolMixed.hidden = true;
      eolSelect.value = "LF";
      indentSelect.value = "4";
      indentSelect.disabled = true;
      saveButton.disabled = true;
      errorElement.textContent = "";
      errorElement.title = "";
      errorElement.className = "editor-error";
      position.textContent = "Ln 1, Col 1";
      selection.textContent = "Sel 0";
      return;
    }
    if (tab.image) {
      language.textContent = imageLabel(tab.path);
      eolSelect.disabled = true;
      eolMixed.hidden = true;
      indentSelect.disabled = true;
      saveButton.disabled = true;
      errorElement.textContent = tab.error ?? "";
      errorElement.title = tab.error ?? "";
      errorElement.className = tab.error ? "editor-error bad" : "editor-error";
      position.textContent =
        tab.width && tab.height ? `${tab.width} × ${tab.height}` : "—";
      selection.textContent = tab.bytes ? bytes(tab.bytes.length) : "";
      return;
    }
    saveButton.disabled = false;
    eolSelect.disabled = false;
    indentSelect.disabled = false;
    language.textContent = languageLabel(languageFor(tab.path));
    if (tab.eol === "Mixed") {
      eolMixed.hidden = false;
      eolSelect.value = "Mixed";
    } else {
      eolMixed.hidden = true;
      eolSelect.value = tab.eol === "CRLF" ? "CRLF" : "LF";
    }
    indentSelect.value = indentSetting.useTabs
      ? "tab"
      : String(indentSetting.size);
    const tabOption = indentSelect.querySelector('option[value="tab"]');
    if (tabOption) tabOption.textContent = `Tab (${indentSetting.size})`;
    indentSelect.title = `Indentation: ${indentLabel()}`;
    if (!view) {
      position.textContent = "Ln 1, Col 1";
      selection.textContent = "Sel 0";
      return;
    }
    const state = view.state;
    const main = state.selection.main;
    const line = state.doc.lineAt(main.head);
    position.textContent = `Ln ${line.number}, Col ${main.head - line.from + 1}`;
    selection.textContent = `Sel ${Math.max(0, main.to - main.from)}`;
    let diagnostic = null;
    forEachDiagnostic(state, (entry) => {
      if (!diagnostic && entry.severity === "error") diagnostic = entry;
    });
    if (diagnostic) {
      const at = state.doc.lineAt(diagnostic.from);
      errorElement.textContent = `Syntax · Ln ${at.number}: ${diagnostic.message}`;
      errorElement.title = diagnostic.message;
      errorElement.className = "editor-error bad";
    } else {
      errorElement.textContent = "";
      errorElement.title = "";
      errorElement.className = "editor-error";
    }
  }

  const updateListener = EditorView.updateListener.of((update) => {
    if (switching) return;
    const tab = mountedTab;
    if (!tab || tab.image || !tabs.includes(tab)) return;
    tab.state = update.state;
    if (update.docChanged) {
      tab.dirty = true;
      renderTabs();
      publishSession();
    }
    if (activeTab() === tab) refreshStatus();
  });

  function createState(path, text) {
    return EditorState.create({
      doc: normalizeNewlines(text),
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        rectangularSelection(),
        highlightActiveLine(),
        indentCompartment.of(indentExtensions(indentSetting)),
        keymap.of([
          { key: "Mod-/", run: toggleComment },
          { key: "Ctrl-Space", run: startCompletion },
          ...completionKeymap,
          indentWithTab,
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        languageExtension(path),
        autocompletion({
          override: [completionSource(path)],
          activateOnTyping: true,
        }),
        linter(syntaxDiagnostics),
        lintGutter(),
        oneDark,
        updateListener,
        EditorView.theme({
          "&": { height: "100%", backgroundColor: "#0d1016" },
          ".cm-scroller": {
            fontFamily: "ui-monospace, monospace",
            fontSize: "12px",
            lineHeight: "1.6",
            padding: "0",
          },
          ".cm-content": { padding: "0" },
          ".cm-gutters": {
            backgroundColor: "#0b0e14",
            borderRight: "1px solid #171c26",
            padding: "0",
          },
          ".cm-activeLine": { backgroundColor: "#ffffff08" },
          ".cm-activeLineGutter": { backgroundColor: "#1b2130" },
        }),
      ],
    });
  }

  function ensureView() {
    if (view) return view;
    view = new EditorView({ state: activeTab().state, parent: content });
    return view;
  }

  function renderImage(tab) {
    imagePreview.onload = () => {
      tab.width = imagePreview.naturalWidth;
      tab.height = imagePreview.naturalHeight;
      if (activeTab() === tab) refreshStatus();
    };
    imagePreview.onerror = () => {
      tab.error = "The image could not be decoded.";
      imagePreview.hidden = true;
      imageEmpty.hidden = false;
      imageEmpty.textContent = `Could not preview: ${tab.error}`;
      if (activeTab() === tab) refreshStatus();
    };
    if (!tab.bytes) {
      imagePreview.hidden = true;
      imagePreview.removeAttribute("src");
      imageEmpty.hidden = false;
      imageEmpty.textContent = tab.error
        ? `Could not preview: ${tab.error}`
        : "Loading image…";
      return;
    }
    if (!tab.objectUrl) {
      tab.objectUrl = URL.createObjectURL(
        new Blob([new Uint8Array(tab.bytes)], { type: tab.mime }),
      );
    }
    imageEmpty.hidden = true;
    imagePreview.hidden = false;
    imagePreview.alt = baseName(tab.path);
    imagePreview.src = tab.objectUrl;
  }

  async function loadImageTab(tab) {
    try {
      const data = await call(
        context,
        `/file?path=${encodeURIComponent(tab.path)}`,
      );
      if (disposed || !tabs.includes(tab)) return;
      if (!Array.isArray(data)) throw new Error("invalid image data");
      if (tab.objectUrl) {
        URL.revokeObjectURL(tab.objectUrl);
        tab.objectUrl = null;
      }
      tab.bytes = data;
      tab.error = null;
      if (activeTab() === tab) {
        renderImage(tab);
        refreshStatus();
      }
    } catch (error) {
      if (disposed || !tabs.includes(tab)) return;
      tab.error = reason(error);
      if (activeTab() === tab) {
        renderImage(tab);
        refreshStatus();
      }
    }
  }

  function activate(index) {
    const tab = tabs[index];
    if (!tab) return;
    active = index;
    pathInput.value = tab.path;
    empty.hidden = true;
    if (tab.image) {
      content.hidden = true;
      imageWrap.hidden = false;
      renderImage(tab);
    } else {
      imageWrap.hidden = true;
      content.hidden = false;
      ensureView();
      mountedTab = tab;
      switching = true;
      view.setState(tab.state);
      switching = false;
    }
    renderTabs();
    refreshStatus();
    renderTree();
    if (mobileQuery.matches && treeOpen) setTreeOpen(false, false);
    publishSession();
    if (!tab.image) view.focus();
  }

  function closeTab(index) {
    const tab = tabs[index];
    if (!tab) return;
    if (tab.objectUrl) URL.revokeObjectURL(tab.objectUrl);
    if (mountedTab === tab) mountedTab = null;
    tabs.splice(index, 1);
    if (tabs.length === 0) {
      active = -1;
      view?.destroy();
      view = null;
      content.replaceChildren();
      content.hidden = true;
      imageWrap.hidden = true;
      imagePreview.removeAttribute("src");
      imageEmpty.hidden = false;
      imageEmpty.textContent = "Loading image…";
      empty.hidden = false;
      pathInput.value = "";
      refreshStatus();
      renderTabs();
      renderTree();
      publishSession();
      return;
    }
    if (active === index) active = Math.min(index, tabs.length - 1);
    else if (active > index) active -= 1;
    activate(active);
  }

  async function openFile(path, options = {}) {
    const target = normalizePath(path);
    if (target === null) {
      setInfo("The path cannot escape the root.", true);
      return;
    }
    const existing = tabs.findIndex((tab) => tab.path === target);
    if (existing >= 0 && typeof options.text !== "string") {
      activate(existing);
      return;
    }
    const mime = IMAGES[extensionOf(target)];
    if (mime) {
      setInfo(`Reading ${target}…`);
      try {
        const data = await call(
          context,
          `/file?path=${encodeURIComponent(target)}`,
        );
        if (disposed) return;
        if (!Array.isArray(data)) throw new Error("invalid image data");
        const tab = {
          path: target,
          image: true,
          mime,
          bytes: data,
          error: null,
          width: 0,
          height: 0,
          objectUrl: null,
          dirty: false,
          eol: "LF",
          state: null,
        };
        if (existing >= 0) tabs[existing] = tab;
        else tabs.push(tab);
        activate(tabs.indexOf(tab));
        void ensureTreePath(target);
        setInfo(`${target} · ${imageLabel(target)}`);
      } catch (error) {
        if (!disposed) setInfo(reason(error), true);
      }
      return;
    }
    let value = typeof options.text === "string" ? options.text : null;
    if (value === null) {
      setInfo(`Reading ${target}…`);
      try {
        const data = await call(
          context,
          `/file?path=${encodeURIComponent(target)}`,
        );
        if (disposed) return;
        value = text(data);
        if (value === null) {
          setInfo(`${target} is not UTF-8 text.`, true);
          return;
        }
      } catch (error) {
        if (!disposed) setInfo(reason(error), true);
        return;
      }
    }
    const tab = {
      path: target,
      state: createState(target, value),
      eol: options.eol ?? detectEol(value),
      dirty: options.dirty === true,
    };
    if (existing >= 0) tabs[existing] = tab;
    else tabs.push(tab);
    activate(tabs.indexOf(tab));
    void ensureTreePath(target);
    setInfo(`${target} · ${languageLabel(languageFor(target))}`);
  }

  async function saveTab(tab) {
    if (!tab || tab.image) return false;
    const eol = tab.eol === "CRLF" ? "CRLF" : "LF";
    const data = Array.from(
      encoder.encode(joinEol(tab.state.doc.toString(), eol)),
    );
    setInfo(`Saving ${tab.path}…`);
    try {
      await call(context, "/file", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path: tab.path, bytes: data }),
      });
      if (disposed) return false;
      tab.dirty = false;
      tab.eol = eol;
      renderTabs();
      refreshStatus();
      publishSession();
      setInfo(`Saved ${tab.path} · ${data.length} bytes`);
      return true;
    } catch (error) {
      if (!disposed) setInfo(reason(error), true);
      return false;
    }
  }

  async function saveTabs(list) {
    for (const tab of list) {
      if (!(await saveTab(tab))) return false;
    }
    return true;
  }

  async function saveActive() {
    const tab = activeTab();
    if (tab) await saveTab(tab);
  }

  function settleDialog(choice) {
    if (!dialogResolve) return;
    const resolve = dialogResolve;
    dialogResolve = null;
    dialogPromise = null;
    dialogTabs = [];
    dialogBusy = false;
    if (dialog.open) dialog.close();
    resolve(choice);
  }

  function renderDialogFiles() {
    dialogFiles.replaceChildren();
    for (const tab of dialogTabs) {
      const item = document.createElement("li");
      item.textContent = tab.path;
      dialogFiles.append(item);
    }
    dialogTitle.textContent =
      dialogTabs.length === 1
        ? `Save changes to ${baseName(dialogTabs[0].path)}?`
        : `Save ${dialogTabs.length} files?`;
    dialogText.textContent =
      dialogTabs.length === 1
        ? "This file has unsaved changes."
        : "These files have unsaved changes.";
    dialogSave.textContent = dialogTabs.length === 1 ? "Save" : "Save all";
  }

  function showCloseDialog(list) {
    if (dialogPromise) {
      for (const tab of list) {
        if (!dialogTabs.includes(tab)) dialogTabs.push(tab);
      }
      renderDialogFiles();
      return dialogPromise;
    }
    dialogTabs = [...list];
    renderDialogFiles();
    dialogError.textContent = "";
    dialogSave.disabled = false;
    dialogDiscard.disabled = false;
    dialogCancel.disabled = false;
    dialogPromise = new Promise((resolve) => {
      dialogResolve = resolve;
    });
    dialog.showModal();
    dialogSave.focus();
    return dialogPromise;
  }

  async function requestCloseTab(index) {
    const tab = tabs[index];
    if (!tab) return;
    if (!tab.dirty) {
      closeTab(index);
      return;
    }
    const choice = await showCloseDialog([tab]);
    if (choice === "cancel") return;
    closeTab(index);
  }

  async function confirmClose() {
    const dirty = tabs.filter((tab) => tab.dirty);
    if (!dirty.length) return true;
    const choice = await showCloseDialog(dirty);
    return choice !== "cancel";
  }

  function applyIndent(next) {
    indentSetting = next;
    for (const tab of tabs) {
      if (!tab.state) continue;
      tab.state = tab.state.update({
        effects: indentCompartment.reconfigure(indentExtensions(indentSetting)),
      }).state;
    }
    if (view && activeTab() && !activeTab().image) {
      switching = true;
      view.setState(activeTab().state);
      switching = false;
    }
    refreshStatus();
    publishSession();
    setInfo(`Indentation: ${indentLabel()}`);
  }

  async function loadDirectory(path) {
    const page = await call(
      context,
      `/files?path=${encodeURIComponent(path)}&limit=500`,
    );
    treeData.set(path, {
      open: treeData.get(path)?.open ?? true,
      entries: page.entries,
    });
  }

  async function ensureTreePath(path) {
    if (treeRoot !== "/") {
      treeRoot = "/";
      treeData.clear();
    }
    const parts = path.split("/").filter(Boolean);
    if (!treeData.has("/")) {
      try {
        await loadDirectory("/");
      } catch {
        return;
      }
    }
    let current = "/";
    for (let index = 0; index < parts.length - 1; index += 1) {
      current = join(current, parts[index]);
      if (!treeData.has(current)) {
        try {
          await loadDirectory(current);
        } catch {
          break;
        }
      }
      const state = treeData.get(current);
      if (state) state.open = true;
    }
    if (!disposed) renderTree();
  }

  async function loadTree(path) {
    treeRoot = path;
    treeData.clear();
    treeElement.textContent = "Loading…";
    try {
      await loadDirectory(path);
      if (!disposed) renderTree();
    } catch (error) {
      if (!disposed) treeElement.textContent = reason(error);
    }
  }

  function renderTree() {
    treeElement.replaceChildren();
    const root = treeData.get(treeRoot);
    const header = document.createElement("div");
    header.className = "editor-tree-head";
    const rootButton = document.createElement("button");
    rootButton.type = "button";
    rootButton.className = "editor-tree-root";
    rootButton.innerHTML = `${iconMarkup("dir", treeRoot)}<span class="editor-tree-name">${escape(treeRoot)}</span>`;
    rootButton.title = "Refresh this directory";
    rootButton.addEventListener("click", () => void loadTree(treeRoot));
    header.append(rootButton);
    if (treeRoot !== "/") {
      const up = document.createElement("button");
      up.type = "button";
      up.className = "editor-tree-up";
      up.textContent = "↑ /";
      up.title = "Back to the filesystem root";
      up.addEventListener("click", () => void loadTree("/"));
      header.append(up);
    }
    const closeTree = document.createElement("button");
    closeTree.type = "button";
    closeTree.className = "editor-tree-close";
    closeTree.textContent = "×";
    closeTree.title = "Close file tree";
    closeTree.setAttribute("aria-label", "Close file tree");
    closeTree.addEventListener("click", () => setTreeOpen(false));
    header.append(closeTree);
    treeElement.append(header);
    if (!root) {
      treeElement.append(cell("hint", "Loading…"));
      return;
    }
    const list = document.createElement("div");
    list.className = "editor-tree-list";
    renderTreeEntries(treeRoot, root.entries, list, 0);
    treeElement.append(list);
  }

  function renderTreeEntries(base, entries, container, depth) {
    for (const entry of entries) {
      const path = join(base, entry.name);
      const row = document.createElement("div");
      row.className = "editor-tree-row";
      row.style.paddingLeft = `${8 + depth * 14}px`;
      if (entry.kind === "dir") {
        const state = treeData.get(path);
        const open = state?.open === true;
        const toggle = document.createElement("button");
        toggle.type = "button";
        toggle.className = "editor-tree-label dir";
        toggle.innerHTML = `<span class="editor-tree-arrow">${open ? "▾" : "▸"}</span>${iconMarkup("dir", path)}<span class="editor-tree-name">${escape(entry.name)}</span>`;
        toggle.addEventListener("click", () => void toggleDirectory(path));
        row.append(toggle);
        container.append(row);
        if (open && state?.entries) {
          renderTreeEntries(path, state.entries, container, depth + 1);
        }
      } else if (entry.kind === "link") {
        const link = document.createElement("button");
        link.type = "button";
        link.className = "editor-tree-label link";
        link.innerHTML = `${iconMarkup("link", path)}<span class="editor-tree-name">${escape(entry.name)}</span>`;
        link.title = `Open symbolic link ${path}`;
        link.addEventListener("click", () => void openLink(path));
        row.append(link);
        container.append(row);
      } else {
        const file = document.createElement("button");
        file.type = "button";
        file.className = "editor-tree-label file";
        if (activeTab()?.path === path) file.classList.add("active");
        file.innerHTML = `${iconMarkup("file", path)}<span class="editor-tree-name">${escape(entry.name)}</span>`;
        file.addEventListener("click", () => void openFile(path));
        row.append(file);
        container.append(row);
      }
    }
  }

  async function toggleDirectory(path) {
    const state = treeData.get(path);
    if (state?.open) {
      state.open = false;
      renderTree();
      return;
    }
    if (!state) {
      try {
        await loadDirectory(path);
      } catch (error) {
        setInfo(reason(error), true);
        return;
      }
    }
    const next = treeData.get(path);
    if (next) next.open = true;
    renderTree();
  }

  async function openLink(path) {
    setInfo(`Resolving ${path}…`);
    try {
      const target = await call(
        context,
        `/target?path=${encodeURIComponent(path)}`,
      );
      const stat = await call(
        context,
        `/stat?path=${encodeURIComponent(target)}`,
      );
      if (disposed) return;
      if (stat.kind === "dir") {
        await loadTree(target);
        setInfo(`${path} → ${target}`);
      } else {
        await openFile(target);
        setInfo(`${path} → ${target}`);
      }
    } catch (error) {
      if (!disposed) setInfo(reason(error), true);
    }
  }

  async function openPathFromInput() {
    const target = normalizePath(pathInput.value);
    if (target === null) {
      setInfo("The path cannot escape the root.", true);
      return;
    }
    try {
      const stat = await call(
        context,
        `/stat?path=${encodeURIComponent(target)}`,
      );
      if (disposed) return;
      if (stat.kind === "dir") {
        await loadTree(target);
        setInfo(`${target} · directory`);
      } else {
        await openFile(target);
      }
    } catch (error) {
      if (!disposed) setInfo(reason(error), true);
    }
  }

  box.querySelector("[data-open]").addEventListener("click", () => {
    void openPathFromInput();
  });
  box.querySelector("[data-save]").addEventListener("click", () => {
    void saveActive();
  });
  box.querySelector("[data-refresh]").addEventListener("click", () => {
    void loadTree(treeRoot);
  });
  treeToggle.addEventListener("click", () => setTreeOpen(!treeOpen));
  treeBackdrop.addEventListener("click", () => setTreeOpen(false));
  const treeModeChanged = () => applyTreeState();
  mobileQuery.addEventListener("change", treeModeChanged);
  pathInput.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void openPathFromInput();
  });
  pathInput.addEventListener("focus", () => pathInput.select());
  eolSelect.addEventListener("change", () => {
    const tab = activeTab();
    if (!tab) return;
    tab.eol = eolSelect.value === "CRLF" ? "CRLF" : "LF";
    tab.dirty = true;
    renderTabs();
    publishSession();
    setInfo(`EOL ${tab.eol} · ${tab.path}`);
  });
  indentSelect.addEventListener("change", () => {
    applyIndent(
      indentSelect.value === "tab"
        ? { size: Number(indentSetting.size) || 4, useTabs: true }
        : { size: Number(indentSelect.value), useTabs: false },
    );
  });
  dialogSave.addEventListener("click", async () => {
    if (dialogBusy || !dialogTabs.length) return;
    dialogBusy = true;
    dialogSave.disabled = true;
    dialogDiscard.disabled = true;
    dialogCancel.disabled = true;
    dialogError.textContent = "";
    const files = [...dialogTabs];
    const ok = await saveTabs(files);
    if (ok) {
      settleDialog("save");
      return;
    }
    dialogBusy = false;
    dialogSave.disabled = false;
    dialogDiscard.disabled = false;
    dialogCancel.disabled = false;
    dialogError.textContent =
      "Could not save every file. Check the status and try again.";
  });
  dialogDiscard.addEventListener("click", () => {
    if (!dialogBusy) settleDialog("discard");
  });
  dialogCancel.addEventListener("click", () => {
    if (!dialogBusy) settleDialog("cancel");
  });
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    if (!dialogBusy) settleDialog("cancel");
  });
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog && !dialogBusy) settleDialog("cancel");
  });
  box.addEventListener("keydown", (event) => {
    if (dialog.open) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveActive();
    }
    if (
      event.key === "Escape" &&
      mobileQuery.matches &&
      treeOpen
    ) {
      event.preventDefault();
      setTreeOpen(false);
    }
  });

  const bridge = {
    openPath: (path) => openFile(path),
    confirmClose: () => confirmClose(),
  };
  editorBridge = bridge;

  const params = context.params ?? {};
  const session = params && typeof params === "object" ? params.session : null;
  if (session && session.indent && typeof session.indent === "object") {
    indentSetting = {
      size: Number(session.indent.size) || 4,
      useTabs: session.indent.useTabs === true,
    };
  }
  if (session && typeof session.tree === "boolean") {
    treeOpen = session.tree;
  }
  if (session && Array.isArray(session.tabs)) {
    for (const entry of session.tabs) {
      if (!entry || typeof entry.path !== "string") continue;
      if (entry.image === true) {
        const mime = entry.mime ?? IMAGES[extensionOf(entry.path)];
        if (!mime) continue;
        const tab = {
          path: entry.path,
          image: true,
          mime,
          bytes: null,
          error: null,
          width: 0,
          height: 0,
          objectUrl: null,
          dirty: false,
          eol: "LF",
          state: null,
        };
        tabs.push(tab);
        void loadImageTab(tab);
        continue;
      }
      if (typeof entry.text !== "string") continue;
      tabs.push({
        path: entry.path,
        state: createState(entry.path, entry.text),
        eol:
          entry.eol === "CRLF" || entry.eol === "LF"
            ? entry.eol
            : detectEol(entry.text),
        dirty: entry.dirty === true,
      });
    }
    if (tabs.length) {
      const index = tabs.findIndex((tab) => tab.path === session.active);
      activate(index >= 0 ? index : 0);
    }
  } else if (params && typeof params.path === "string") {
    if (typeof params.text === "string") {
      void openFile(params.path, {
        text: params.text,
        dirty: params.dirty !== false,
        eol: detectEol(params.text),
      });
    } else {
      void openFile(params.path);
    }
  }
  if (!tabs.length) {
    refreshStatus();
    renderTabs();
  }
  applyTreeState(false);
  void loadTree("/");

  return () => {
    disposed = true;
    if (editorBridge === bridge) editorBridge = null;
    mobileQuery.removeEventListener("change", treeModeChanged);
    view?.destroy();
    view = null;
    root.replaceChildren();
  };
}

export function mount(root, context) {
  if (context.contribution.tag === "anth-fs-editor") {
    return mountEditorView(root, context);
  }

  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "fs";
  box.innerHTML = `
    <header class="fs-bar">
      <button type="button" data-up title="Parent directory">Up</button>
      <input class="fs-address" list="fs-paths" aria-label="Path" spellcheck="false" autocomplete="off" />
      <datalist id="fs-paths"></datalist>
      <button type="button" data-refresh>Refresh</button>
      <button type="button" data-trash>Trash</button>
      <span class="grow"></span>
      <button type="button" data-upload>Upload</button>
      <button type="button" data-new>New</button>
      <input type="file" data-file multiple hidden />
    </header>
    <form class="fs-new" hidden>
      <select name="kind" aria-label="New entry type">
        <option value="file">File</option>
        <option value="dir">Folder</option>
        <option value="link">Symlink</option>
      </select>
      <input name="name" placeholder="Name" aria-label="Name" autocomplete="off" />
      <input name="target" placeholder="Target path (absolute)" aria-label="Symlink target" autocomplete="off" hidden />
      <button type="submit">Create</button>
      <button type="button" data-cancel-new>Cancel</button>
    </form>
    <form class="fs-rename" hidden>
      <input name="name" placeholder="New name" aria-label="New name" autocomplete="off" />
      <button type="submit">Rename</button>
      <button type="button" data-cancel-rename>Cancel</button>
    </form>
    <div class="fs-confirm" hidden>
      <span data-confirm-text></span>
      <button type="button" data-confirm-yes>Overwrite</button>
      <button type="button" data-confirm-no>Cancel</button>
    </div>
    <div class="fs-body">
      <ul class="fs-list" aria-label="Directory entries"></ul>
      <section class="fs-preview"><p class="hint">Select a file to preview it.</p></section>
    </div>
    <div class="fs-more"><button type="button" data-more hidden>Load more</button></div>
    <section class="fs-queue" data-queue hidden>
      <header>
        <strong>Transfers</strong>
        <button type="button" data-queue-clear>Clear finished</button>
      </header>
      <ul data-queue-list></ul>
    </section>
    <p class="fs-status" role="status"></p>
    <div class="fs-drop" data-drop hidden>Drop files to upload</div>
  `;
  root.append(box);

  const address = box.querySelector(".fs-address");
  const paths = box.querySelector("#fs-paths");
  const list = box.querySelector(".fs-list");
  const preview = box.querySelector(".fs-preview");
  const status = box.querySelector(".fs-status");
  const up = box.querySelector("[data-up]");
  const more = box.querySelector("[data-more]");
  const newForm = box.querySelector(".fs-new");
  const newKind = newForm.querySelector("select");
  const newInput = newForm.querySelector("[name=name]");
  const newTarget = newForm.querySelector("[name=target]");
  const renameForm = box.querySelector(".fs-rename");
  const renameInput = renameForm.querySelector("input");
  const confirmBox = box.querySelector(".fs-confirm");
  const confirmText = box.querySelector("[data-confirm-text]");
  const uploadInput = box.querySelector("[data-file]");
  const trashButton = box.querySelector("[data-trash]");
  const newButton = box.querySelector("[data-new]");
  const uploadButton = box.querySelector("[data-upload]");
  const queueBox = box.querySelector("[data-queue]");
  const queueList = box.querySelector("[data-queue-list]");
  const queueClear = box.querySelector("[data-queue-clear]");
  const dropOverlay = box.querySelector("[data-drop]");

  let current = "/";
  let nextCursor = null;
  let confirmPath = null;
  let renamePath = null;
  let pendingUpload = null;
  let view = "files";
  let disposed = false;
  let previewState = null;
  const queue = [];
  let queueNext = 0;
  let queueBusy = false;
  let dragDepth = 0;
  const knownPaths = new Set([
    "/",
    "/etc",
    "/home",
    "/home/operator",
    "/tmp",
    "/var",
    "/var/log",
  ]);

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "fs-status bad" : "fs-status";
  };

  const request = (route, init) => call(context, route, init);
  const rows = () => list.querySelectorAll(".fs-row").length;

  function renderPaths() {
    paths.replaceChildren();
    for (const path of [...knownPaths].sort()) {
      const option = document.createElement("option");
      option.value = path;
      paths.append(option);
    }
  }

  function rememberPath(path) {
    knownPaths.add(path);
    renderPaths();
  }

  function syncNewKind() {
    const link = newKind.value === "link";
    newTarget.hidden = !link;
    newInput.placeholder = link
      ? "Link name"
      : newKind.value === "dir"
        ? "Folder name"
        : "File name";
    if (!link) newTarget.value = "";
  }

  function navigate(input) {
    const path = normalizePath(input);
    if (path === null) {
      setStatus("The path cannot escape the root.", true);
      return;
    }
    address.value = path;
    void load(path);
  }

  function resetPreview() {
    box.classList.remove("preview-open");
    if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
    previewState = null;
    preview.replaceChildren();
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = "Select a file to preview it.";
    preview.append(hint);
  }

  function clearRename() {
    renamePath = null;
    renameForm.hidden = true;
  }

  function showRename(path) {
    clearRename();
    newForm.hidden = true;
    renamePath = path;
    renameInput.value = baseName(path);
    renameForm.hidden = false;
    renameInput.focus();
    renameInput.select();
  }

  function cancelConfirms() {
    confirmPath = null;
    pendingUpload = null;
    confirmBox.hidden = true;
  }

  function askOverwrite(path) {
    pendingUpload = { path };
    confirmText.textContent = `Overwrite ${path}?`;
    box.querySelector("[data-confirm-yes]").textContent = "Overwrite";
    confirmBox.hidden = false;
  }

  function confirmDelete(row, path, remove) {
    remove.hidden = true;
    const yes = document.createElement("button");
    yes.type = "button";
    yes.className = "fs-tool confirm";
    yes.textContent = "Delete";
    const no = document.createElement("button");
    no.type = "button";
    no.className = "fs-tool";
    no.textContent = "Cancel";
    yes.addEventListener("click", () => void drop(path));
    no.addEventListener("click", () => {
      yes.remove();
      no.remove();
      remove.hidden = false;
      confirmPath = null;
    });
    row.append(yes, no);
  }

  function appendRow(entry, base) {
    const full = join(base, entry.name);
    const row = document.createElement("li");
    row.className = "fs-row";
    const entryButton = document.createElement("button");
    entryButton.type = "button";
    entryButton.className = "fs-entry";
    entryButton.dataset.name = entry.name;
    entryButton.dataset.kind = entry.kind;
    entryButton.dataset.path = full;
    entryButton.innerHTML = `${iconMarkup(entry.kind, full)}<span class="name">${escape(entry.name)}</span><span class="size">${entry.kind === "dir" ? "" : bytes(entry.size)}</span>`;
    entryButton.addEventListener("click", () => {
      if (entry.kind === "dir") void load(full);
      else if (entry.kind === "link") void openLink(full);
      else void open(full, entry);
    });

    const rename = button("fs-tool", "R", `Rename ${entry.name}`);
    rename.addEventListener("click", () => showRename(full));

    const remove = button("fs-tool delete", "×", `Delete ${entry.name}`);
    remove.textContent = "×";
    remove.addEventListener("click", () => {
      if (confirmPath === full) {
        void drop(full);
        return;
      }
      confirmPath = full;
      confirmDelete(row, full, remove);
    });
    row.append(entryButton, rename, remove);
    list.append(row);
  }

  function button(className, textValue, title) {
    const node = document.createElement("button");
    node.type = "button";
    node.className = className;
    node.textContent = textValue;
    if (title) node.title = title;
    return node;
  }

  async function loadPage(path, cursor, append) {
    if (view !== "files") return;
    setStatus(append ? "Loading more…" : "Loading…");
    try {
      const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
      const page = await request(
        `/files?path=${encodeURIComponent(path)}&limit=200${suffix}`,
      );
      if (disposed || view !== "files") return;
      if (!append) {
        current = path;
        confirmPath = null;
        clearRename();
        address.value = path;
        address.disabled = false;
        rememberPath(path);
        up.disabled = path === "/";
        list.replaceChildren();
        resetPreview();
      }
      for (const entry of page.entries) appendRow(entry, path);
      nextCursor = page.cursor;
      more.hidden = Boolean(page.done) || !page.cursor;
      if (rows() === 0) {
        const empty = document.createElement("li");
        empty.className = "fs-status";
        empty.textContent = "This directory is empty.";
        list.append(empty);
      }
      setStatus(
        `${rows()} entr${rows() === 1 ? "y" : "ies"}${page.done ? "" : " · more available"}`,
      );
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  function load(path) {
    if (view !== "files") {
      view = "files";
      trashButton.textContent = "Trash";
      newButton.hidden = false;
      uploadButton.hidden = false;
      up.hidden = false;
      more.hidden = false;
    }
    return loadPage(path, null, false);
  }

  async function loadTrash() {
    view = "trash";
    current = "/";
    nextCursor = null;
    more.hidden = true;
    address.value = "Trash";
    address.disabled = true;
    up.disabled = true;
    up.hidden = true;
    newForm.hidden = true;
    clearRename();
    cancelConfirms();
    trashButton.textContent = "Files";
    newButton.hidden = true;
    uploadButton.hidden = true;
    list.replaceChildren();
    resetPreview();
    setStatus("Loading the trash…");
    try {
      const entries = await request("/trash");
      if (disposed) return;
      for (const entry of entries) {
        const row = document.createElement("li");
        row.className = "fs-row";
        const copy = document.createElement("span");
        copy.className = "fs-entry";
        copy.innerHTML = `${iconMarkup(entry.kind, entry.original)}<span class="name">${escape(entry.original)}</span><span class="size">${entry.kind === "dir" ? "" : bytes(entry.size)}</span>`;
        const restoreButton = button("fs-tool", "Restore", `Restore ${entry.original}`);
        restoreButton.addEventListener("click", () => void restore(entry.original));
        row.append(copy, restoreButton);
        list.append(row);
      }
      if (entries.length === 0) {
        const empty = document.createElement("li");
        empty.className = "fs-status";
        empty.textContent = "The trash is empty.";
        list.append(empty);
      }
      setStatus(`${entries.length} item${entries.length === 1 ? "" : "s"} in the trash`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function restore(original) {
    try {
      await request("/restore", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ original }),
      });
      await loadTrash();
      setStatus(`Restored ${original}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function openLink(path) {
    setStatus("Resolving…");
    try {
      const target = await request(`/target?path=${encodeURIComponent(path)}`);
      const stat = await request(`/stat?path=${encodeURIComponent(target)}`);
      if (disposed) return;
      if (stat.kind === "dir") {
        await load(target);
        if (!disposed) setStatus(`${path} → ${target}`);
        return;
      }
      await open(target, { ...stat, name: baseName(target) }, path);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function open(path, entry, link = null) {
    setStatus("Reading…");
    try {
      const data = await request(`/file?path=${encodeURIComponent(path)}`);
      if (disposed) return;
      if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
      const decoded = text(data);
      previewState = {
        path,
        entry,
        link,
        bytes: data,
        text: decoded,
        objectUrl: null,
      };
      renderPreview();
      box.classList.add("preview-open");
      setStatus(`${path} · ${data.length} bytes`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  function triggerDownload(path, data) {
    const name = baseName(path);
    const extension = extensionOf(path);
    const blob = new Blob([new Uint8Array(data)], {
      type: IMAGES[extension] ?? "application/octet-stream",
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function enqueueDownload(path) {
    queue.push({
      id: ++queueNext,
      type: "download",
      name: baseName(path),
      path,
      status: "queued",
      progress: 0,
      message: "Queued",
    });
    renderQueue();
    void pumpQueue();
  }

  function enqueueUploads(files, target) {
    if (view !== "files") {
      setStatus("Return to Files to upload.", true);
      return;
    }
    let added = 0;
    for (const file of files) {
      const path = join(target, file.name);
      const active = queue.some(
        (item) =>
          item.type === "upload" &&
          item.path === path &&
          ["queued", "uploading", "conflict"].includes(item.status),
      );
      if (active) continue;
      queue.push({
        id: ++queueNext,
        type: "upload",
        name: file.name,
        path,
        file,
        status: "queued",
        progress: 0,
        message: `→ ${target}`,
        cancelled: false,
        overwrite: false,
      });
      added += 1;
    }
    if (!added) {
      setStatus("Those files are already queued.", true);
      return;
    }
    setStatus(`Queued ${added} upload${added === 1 ? "" : "s"}`);
    renderQueue();
    void pumpQueue();
  }

  function queueStatus(item) {
    if (item.status === "queued") {
      return item.type === "upload" ? `Queued → ${parent(item.path)}` : "Queued";
    }
    if (item.status === "uploading") return `Uploading ${item.progress}%`;
    if (item.status === "downloading") return `Downloading ${item.progress}%`;
    if (item.status === "done") return "Done";
    if (item.status === "conflict") return "Already exists";
    if (item.status === "error") return item.message || "Failed";
    if (item.status === "cancelled") return "Cancelled";
    return item.status;
  }

  function renderQueue() {
    queueBox.hidden = queue.length === 0;
    queueList.replaceChildren();
    for (const item of queue) {
      const row = document.createElement("li");
      row.className = "fs-queue-item";
      const dir = document.createElement("span");
      dir.className = "fs-queue-dir";
      dir.textContent = item.type === "upload" ? "↑" : "↓";
      const main = document.createElement("div");
      main.className = "fs-queue-main";
      const name = document.createElement("span");
      name.className = "fs-queue-name";
      name.textContent = item.name;
      name.title = item.path;
      const meta = document.createElement("span");
      meta.className = "fs-queue-meta";
      meta.textContent = queueStatus(item);
      main.append(name, meta);
      const progress = document.createElement("div");
      progress.className = "fs-queue-progress";
      const fill = document.createElement("span");
      fill.style.width = `${Math.max(0, Math.min(100, item.progress ?? 0))}%`;
      progress.append(fill);
      const actions = document.createElement("div");
      actions.className = "fs-queue-actions";
      if (item.status === "conflict") {
        const overwrite = button("fs-tool", "Overwrite");
        overwrite.addEventListener("click", () => {
          item.overwrite = true;
          item.status = "queued";
          item.message = "";
          renderQueue();
          void pumpQueue();
        });
        const remove = button("fs-tool", "Remove");
        remove.addEventListener("click", () => removeQueueItem(item));
        actions.append(overwrite, remove);
      } else if (item.status === "error") {
        const retry = button("fs-tool", "Retry");
        retry.addEventListener("click", () => {
          item.status = "queued";
          item.message = "";
          renderQueue();
          void pumpQueue();
        });
        const remove = button("fs-tool", "Remove");
        remove.addEventListener("click", () => removeQueueItem(item));
        actions.append(retry, remove);
      } else if (
        item.status === "queued" ||
        item.status === "uploading" ||
        item.status === "downloading"
      ) {
        const cancel = button("fs-tool", "Cancel");
        cancel.addEventListener("click", () => {
          item.cancelled = true;
          if (item.status === "queued") item.status = "cancelled";
          renderQueue();
        });
        actions.append(cancel);
      } else if (item.status === "done") {
        const done = document.createElement("span");
        done.className = "fs-queue-done";
        done.textContent = "✓";
        actions.append(done);
      } else if (item.status === "cancelled") {
        const remove = button("fs-tool", "Remove");
        remove.addEventListener("click", () => removeQueueItem(item));
        actions.append(remove);
      }
      row.append(dir, main, progress, actions);
      queueList.append(row);
    }
  }

  function removeQueueItem(item) {
    const index = queue.indexOf(item);
    if (index >= 0) queue.splice(index, 1);
    renderQueue();
  }

  async function runUpload(item) {
    item.status = "uploading";
    item.progress = 10;
    renderQueue();
    if (!item.overwrite) {
      try {
        await request(`/stat?path=${encodeURIComponent(item.path)}`);
        item.status = "conflict";
        item.message = "Already exists";
        return;
      } catch (error) {
        if (!reason(error).includes("not-found")) {
          item.status = "error";
          item.message = reason(error);
          return;
        }
      }
    }
    try {
      const buffer = await item.file.arrayBuffer();
      item.progress = 55;
      renderQueue();
      if (item.cancelled) {
        item.status = "cancelled";
        return;
      }
      await request("/file", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          path: item.path,
          bytes: Array.from(new Uint8Array(buffer)),
        }),
      });
      item.progress = 100;
      item.status = "done";
      item.message = "Uploaded";
      if (parent(item.path) === current) void load(current);
    } catch (error) {
      item.status = "error";
      item.message = reason(error);
    }
  }

  async function runDownload(item) {
    item.status = "downloading";
    item.progress = 25;
    renderQueue();
    try {
      const data = await request(`/file?path=${encodeURIComponent(item.path)}`);
      item.progress = 80;
      renderQueue();
      if (item.cancelled) {
        item.status = "cancelled";
        return;
      }
      triggerDownload(item.path, data);
      item.progress = 100;
      item.status = "done";
      item.message = "Downloaded";
    } catch (error) {
      item.status = "error";
      item.message = reason(error);
    }
  }

  async function pumpQueue() {
    if (queueBusy) return;
    const item = queue.find((entry) => entry.status === "queued");
    if (!item) return;
    queueBusy = true;
    try {
      if (item.type === "upload") await runUpload(item);
      else await runDownload(item);
    } finally {
      queueBusy = false;
      renderQueue();
      if (queue.some((entry) => entry.status === "queued")) void pumpQueue();
    }
  }

  function download() {
    if (!previewState) return;
    enqueueDownload(previewState.path);
  }

  async function openInEditor() {
    if (!previewState) return;
    if (typeof context.openView !== "function") {
      setStatus(
        "This panel build has no global-view bridge. Reload the panel (Ctrl/Cmd+R) and try again.",
        true,
      );
      return;
    }
    setStatus("Opening the panel Editor…");
    let result;
    try {
      result = await context.openView({
        tag: "anth-fs-editor",
        mode: "float",
        size: { width: 860, height: 580 },
        params: { path: previewState.path },
      });
    } catch {
      result = false;
    }
    if (result === true) {
      setStatus(`Opened ${previewState.path} in the Editor`);
      return;
    }
    if (result === undefined) {
      // The pre-ack bridge opens the view but returns nothing, and ignores
      // `mode`. The Editor is a tab; Float detaches it to the global layer.
      setStatus(
        `Opened ${previewState.path} in the Editor tab. Use Float to detach it, or reload the panel for direct Pop out.`,
      );
      return;
    }
    setStatus(
      "This panel tab is running an older frontend. Reload it (Ctrl/Cmd+R or Cmd+Shift+R) and try again.",
      true,
    );
  }

  function renderPreview() {
    preview.replaceChildren();
    if (!previewState) {
      const hint = document.createElement("p");
      hint.className = "hint";
      hint.textContent = "Select a file to preview it.";
      preview.append(hint);
      return;
    }
    const { path, entry, link, text: decoded } = previewState;
    const extension = extensionOf(path);
    const imageType = IMAGES[extension];
    const meta = document.createElement("div");
    meta.className = "fs-meta";
    meta.append(
      cell("", kindLabel(entry.kind)),
      cell("", bytes(entry.size)),
      cell("", `modified ${modified(entry["modified-ms"])}`),
      cell("", mimeFor(path)),
    );
    if (link) meta.append(cell("", `via ${link}`));

    const actions = document.createElement("div");
    actions.className = "fs-actions";
    if (decoded !== null || imageType) {
      const edit = button("", "Open in Editor");
      edit.title = imageType
        ? "Open this image in the Editor"
        : "Edit this file in the floating Editor window";
      edit.addEventListener("click", () => void openInEditor());
      actions.append(edit);
    }
    const downloadButton = button("", "Download");
    downloadButton.addEventListener("click", download);
    actions.append(downloadButton);

    const pane = document.createElement("div");
    pane.className = "pane";
    if (imageType) {
      if (previewState.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
      previewState.objectUrl = URL.createObjectURL(
        new Blob([new Uint8Array(previewState.bytes)], { type: imageType }),
      );
      const image = document.createElement("img");
      image.src = previewState.objectUrl;
      image.alt = baseName(path);
      image.addEventListener("error", () => {
        if (!disposed && previewState?.path === path) {
          setStatus(`Could not render ${path} as ${imageType}.`, true);
        }
      });
      pane.append(image);
    } else if (decoded !== null) {
      const formatted = document.createElement("div");
      formatted.className = "fs-formatted";
      formatted.innerHTML = formattedPreview(decoded, path);
      pane.append(formatted);
    } else {
      const hint = document.createElement("p");
      hint.className = "hint";
      hint.textContent = `Binary file · ${previewState.bytes.length} bytes`;
      pane.append(hint);
    }

    const head = document.createElement("div");
    head.className = "fs-preview-head";
    const back = button("", "← Files");
    back.title = "Back to the file list";
    back.addEventListener("click", resetPreview);
    const name = document.createElement("strong");
    name.textContent = baseName(path);
    head.append(back, name);

    preview.append(head, meta, actions, pane);
  }

  async function drop(path) {
    try {
      await request(`/entry?path=${encodeURIComponent(path)}&trash=true`, {
        method: "DELETE",
      });
      confirmPath = null;
      if (previewState?.path === path) resetPreview();
      await load(current);
      setStatus(`Moved ${path} to trash`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function upload(file, overwrite) {
    if (!file) return;
    const target = join(current, file.name);
    if (!overwrite) {
      try {
        await request(`/stat?path=${encodeURIComponent(target)}`);
        pendingUpload = { path: target, file };
        askOverwrite(target);
        return;
      } catch (error) {
        if (!reason(error).includes("not-found")) {
          if (!disposed) setStatus(reason(error), true);
          return;
        }
      }
    }
    try {
      const buffer = await file.arrayBuffer();
      await request("/file", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          path: target,
          bytes: Array.from(new Uint8Array(buffer)),
        }),
      });
      pendingUpload = null;
      confirmBox.hidden = true;
      await load(current);
      setStatus(`Uploaded ${target}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function renameTo(source, destination) {
    const previousEntry =
      previewState?.path === source ? previewState.entry : null;
    try {
      await request("/rename", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ source, destination }),
      });
      clearRename();
      if (previousEntry) {
        await load(parent(source));
        await open(destination, {
          ...previousEntry,
          name: baseName(destination),
        });
      } else if (
        current === source ||
        current.startsWith(`${source}/`)
      ) {
        await load(`${destination}${current.slice(source.length)}`);
      } else {
        await load(current);
      }
      setStatus(`Renamed to ${destination}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  up.addEventListener("click", () => {
    if (view === "files" && current !== "/") void load(parent(current));
  });
  address.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    navigate(address.value);
  });
  address.addEventListener("focus", () => address.select());
  box.querySelector("[data-refresh]").addEventListener("click", () => {
    if (view === "trash") void loadTrash();
    else void load(current);
  });
  trashButton.addEventListener("click", () => {
    if (view === "trash") void load(current);
    else void loadTrash();
  });
  more.addEventListener("click", () => {
    if (nextCursor && view === "files") void loadPage(current, nextCursor, true);
  });
  newButton.addEventListener("click", () => {
    newForm.hidden = !newForm.hidden;
    clearRename();
    if (!newForm.hidden) {
      syncNewKind();
      newInput.focus();
    }
  });
  newKind.addEventListener("change", syncNewKind);
  box.querySelector("[data-cancel-new]").addEventListener("click", () => {
    newForm.hidden = true;
  });
  box.querySelector("[data-cancel-rename]").addEventListener("click", clearRename);
  newForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const kind = newKind.value;
    const name = newInput.value.trim();
    if (!name || name.includes("/") || name === "." || name === "..") {
      setStatus("A name cannot be empty or contain `/`.", true);
      return;
    }
    const path = join(current, name);
    try {
      if (kind === "dir") {
        await request("/dir", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path }),
        });
      } else if (kind === "link") {
        const raw = newTarget.value.trim();
        if (!raw.startsWith("/")) {
          setStatus("The symlink target must be an absolute path.", true);
          return;
        }
        const target = normalizePath(raw);
        if (target === null) {
          setStatus("The path cannot escape the root.", true);
          return;
        }
        await request("/symlink", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path, target }),
        });
      } else {
        try {
          await request(`/stat?path=${encodeURIComponent(path)}`);
          setStatus(`${path} already exists.`, true);
          return;
        } catch (error) {
          if (!reason(error).includes("not-found")) {
            setStatus(reason(error), true);
            return;
          }
        }
        await request("/file", {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path, bytes: [] }),
        });
      }
      newForm.hidden = true;
      newInput.value = "";
      newTarget.value = "";
      await load(current);
      setStatus(`Created ${path}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  });
  renameForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!renamePath) return;
    const name = renameInput.value.trim();
    if (!name || name.includes("/") || name === "." || name === "..") {
      setStatus("A name cannot be empty or contain `/`.", true);
      return;
    }
    await renameTo(renamePath, join(parent(renamePath), name));
  });
  uploadButton.addEventListener("click", () => uploadInput.click());
  queueClear.addEventListener("click", () => {
    for (let index = queue.length - 1; index >= 0; index -= 1) {
      if (["done", "cancelled", "error"].includes(queue[index].status)) {
        queue.splice(index, 1);
      }
    }
    renderQueue();
  });
  uploadInput.addEventListener("change", () => {
    const files = [...(uploadInput.files ?? [])];
    uploadInput.value = "";
    if (files.length) enqueueUploads(files, current);
  });
  const hasFiles = (event) =>
    Array.from(event.dataTransfer?.types ?? []).includes("Files");
  const dropTarget = (event) => {
    const row =
      event.target instanceof Element
        ? event.target.closest(".fs-row")
        : null;
    const entry = row?.querySelector(".fs-entry");
    return entry?.dataset.kind === "dir" ? entry.dataset.path : current;
  };
  const clearDropRows = () => {
    list.querySelectorAll(".fs-row.drop-target").forEach((row) => {
      row.classList.remove("drop-target");
    });
  };
  const showDrop = (target) => {
    dropOverlay.hidden = false;
    dropOverlay.textContent = `Drop to upload to ${target}`;
    box.classList.add("dragging");
  };
  const hideDrop = () => {
    dragDepth = 0;
    dropOverlay.hidden = true;
    box.classList.remove("dragging");
    clearDropRows();
  };
  box.addEventListener("dragenter", (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    dragDepth += 1;
    showDrop(dropTarget(event));
  });
  box.addEventListener("dragover", (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    const target = dropTarget(event);
    dropOverlay.textContent = `Drop to upload to ${target}`;
    clearDropRows();
    const row =
      event.target instanceof Element
        ? event.target.closest(".fs-row")
        : null;
    if (row?.querySelector(".fs-entry")?.dataset.kind === "dir") {
      row.classList.add("drop-target");
    }
  });
  box.addEventListener("dragleave", (event) => {
    if (!hasFiles(event)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) hideDrop();
  });
  box.addEventListener("drop", (event) => {
    if (!hasFiles(event)) return;
    event.preventDefault();
    const target = dropTarget(event);
    const files = Array.from(event.dataTransfer?.files ?? []);
    hideDrop();
    if (files.length) enqueueUploads(files, target);
  });
  box.querySelector("[data-confirm-yes]").addEventListener("click", () => {
    if (!pendingUpload) return;
    const uploadTask = pendingUpload;
    pendingUpload = null;
    confirmBox.hidden = true;
    if (uploadTask.file) void upload(uploadTask.file, true);
  });
  box.querySelector("[data-confirm-no]").addEventListener("click", cancelConfirms);

  renderPaths();
  void load("/");

  return () => {
    disposed = true;
    if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
    root.replaceChildren();
  };
}

function reason(error) {
  return error instanceof Error ? error.message : String(error);
}
