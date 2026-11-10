export const styles = `
  .fs { position: relative; display: flex; flex-direction: column; gap: 12px; min-height: 380px; color: #e8ebf2; }
  .fs-bar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .fs-bar button, .fs-form button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fs-bar button:hover, .fs-form button:hover { border-color: #3a4560; color: #fff; }
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
  .fs-new[hidden], .fs-rename[hidden] { display: none; }
  .fs-new input, .fs-rename input {
    flex: 1; min-width: 120px; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12px;
  }
  .fs-new select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12px;
  }
  .fs-new input[hidden] { display: none; }
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
