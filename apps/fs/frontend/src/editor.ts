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

import { call } from "./api";
import { bytes, detectEol, extensionOf, imageLabel, joinEol, languageLabel, text } from "./format";
import { IMAGES, iconMarkup } from "./icons";
import { languageFor, suggestionsFor } from "./language";
import { baseName, join, normalizePath, parent } from "./paths";
import { styles } from "./styles";
import { cell, encoder, escape, normalizeNewlines, reason } from "./util";
import type { FrameContext } from "./types";

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

export function snapshot() {
  return editorBridge?.snapshot?.() ?? null;
}

export function mountEditorView(root: HTMLElement, context: FrameContext) {
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

  const pathInput = box.querySelector(".editor-path") as any;
  const tabsElement = box.querySelector(".editor-tabs") as any;
  const treeElement = box.querySelector(".editor-tree") as any;
  const content = box.querySelector(".editor-content") as any;
  const imageWrap = box.querySelector("[data-image]") as any;
  const imagePreview = box.querySelector("[data-image-preview]") as any;
  const imageEmpty = box.querySelector("[data-image-empty]") as any;
  const empty = box.querySelector(".editor-empty") as any;
  const info = box.querySelector("[data-info]") as any;
  const language = box.querySelector("[data-language]") as any;
  const eolSelect = box.querySelector("[data-eol]") as any;
  const eolMixed = eolSelect.querySelector('option[value="Mixed"]') as any;
  const indentSelect = box.querySelector("[data-indent]") as any;
  const errorElement = box.querySelector("[data-error]") as any;
  const position = box.querySelector("[data-position]") as any;
  const selection = box.querySelector("[data-selection]") as any;
  const saveButton = box.querySelector("[data-save]") as any;
  const treeToggle = box.querySelector("[data-tree-toggle]") as any;
  const treeBackdrop = box.querySelector("[data-tree-backdrop]") as any;
  const dialog = box.querySelector("[data-close-dialog]") as any;
  const dialogTitle = dialog.querySelector("[data-dialog-title]") as any;
  const dialogText = dialog.querySelector("[data-dialog-text]") as any;
  const dialogFiles = dialog.querySelector("[data-dialog-files]") as any;
  const dialogError = dialog.querySelector("[data-dialog-error]") as any;
  const dialogSave = dialog.querySelector("[data-dialog-save]") as any;
  const dialogDiscard = dialog.querySelector("[data-dialog-discard]") as any;
  const dialogCancel = dialog.querySelector("[data-dialog-cancel]") as any;
  const mobileQuery = window.matchMedia("(max-width: 700px)");

  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const tabs: any[] = [];
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
  const readFile = (path) =>
    call(context, `/file?path=${encodeURIComponent(path)}`);

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

  let publishTimer = null;

  function sessionPayload() {
    return {
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
    };
  }

  function flushSession() {
    if (publishTimer) {
      window.clearTimeout(publishTimer);
      publishTimer = null;
    }
    if (typeof context.updateParams === "function") {
      context.updateParams(sessionPayload());
    }
  }

  function publishSession() {
    if (publishTimer) window.clearTimeout(publishTimer);
    publishTimer = window.setTimeout(flushSession, 150);
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
    const tabOption = indentSelect.querySelector('option[value="tab"]') as any;
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
      const data = await readFile(tab.path);
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
    if (!tabs.some((candidate) => !candidate.image) && view) {
      view.destroy();
      view = null;
      mountedTab = null;
      content.replaceChildren();
      content.hidden = true;
    }
    if (active === index) active = Math.min(index, tabs.length - 1);
    else if (active > index) active -= 1;
    activate(active);
  }

  async function openFile(path, options: any = {}) {
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
        const data = await readFile(target);
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
        const data = await readFile(target);
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
    snapshot: () => sessionPayload(),
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
    if (publishTimer) {
      window.clearTimeout(publishTimer);
      publishTimer = null;
    }
    mobileQuery.removeEventListener("change", treeModeChanged);
    for (const tab of tabs) {
      if (tab.objectUrl) URL.revokeObjectURL(tab.objectUrl);
    }
    view?.destroy();
    view = null;
    root.replaceChildren();
  };
}
