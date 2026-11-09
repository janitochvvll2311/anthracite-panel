// The demo Files view: a deterministic tree from the fs service.
//
// The module only knows the app's own routes. Every call goes through the
// frame's scoped fetch, so the panel origin and the cookie stay out of reach.

import { baseKeymap } from "prosemirror-commands";
import { history, redo, undo } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { schema as basicSchema } from "prosemirror-schema-basic";
import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";

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
  .pm-host { min-height: 220px; display: flex; flex-direction: column; padding: 8px; border: 1px solid #2a3142; border-radius: 8px; background: #0f1219; }
  .pm-host .ProseMirror {
    flex: 1; min-height: 220px; outline: none; white-space: pre-wrap; overflow-wrap: anywhere;
    font: 12px/1.6 ui-monospace, monospace; color: #e8ebf2; tab-size: 4;
  }
  .pm-host .ProseMirror p { margin: 0; min-height: 1.6em; }
  .fs-editor-float {
    position: absolute; top: 58px; left: 24px; width: min(760px, calc(100% - 48px));
    height: 440px; min-width: 360px; min-height: 260px; max-width: calc(100% - 16px);
    max-height: calc(100% - 16px); resize: both; overflow: hidden; display: flex;
    flex-direction: column; z-index: 5; border: 1px solid #333d54; border-radius: 12px;
    background: #151a24; box-shadow: 0 24px 70px #00000073;
  }
  .fs-editor-head {
    display: flex; align-items: center; gap: 8px; padding: 7px 8px 7px 12px;
    border-bottom: 1px solid #202634; background: linear-gradient(180deg, #1b2130, #161b26);
    cursor: grab; user-select: none;
  }
  .fs-editor-head strong { font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fs-editor-head .hint { color: #667087; font-size: 10.5px; }
  .fs-editor-head button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .fs-editor-head button:hover { border-color: #3a4560; color: #fff; }
  .fs-editor-head .tools { margin-left: auto; display: flex; gap: 4px; }
  .fs-editor-body { flex: 1; min-height: 0; overflow: auto; padding: 10px 12px; background: #0d1016; }
  .fs-editor-body .ProseMirror { outline: none; white-space: pre-wrap; font: 12px/1.6 ui-monospace, monospace; color: #e8ebf2; }
  .fs-editor-body .ProseMirror p { margin: 0; min-height: 1.6em; }
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
  .fs-entry .glyph { width: 14px; display: inline-flex; align-items: center; }
  .fs-entry .glyph.dir::before {
    content: ""; width: 0; height: 0; border-left: 5px solid #7aa2ff;
    border-top: 4px solid transparent; border-bottom: 4px solid transparent;
  }
  .fs-entry .glyph.file::before {
    content: ""; width: 7px; height: 9px; border: 1px solid #667087; border-radius: 1px;
  }
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
  .fs-preview textarea {
    flex: 1; min-height: 220px; width: 100%; resize: vertical; padding: 9px; border-radius: 7px;
    border: 1px solid #2a3142; background: #0f1219; color: #e8ebf2; font: 12px/1.55 ui-monospace, monospace;
  }
  .fs-preview img { max-width: 100%; max-height: 280px; object-fit: contain; border-radius: 6px; }
  .fs-preview .hint { color: #667087; font-size: 12px; margin: 0; }
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
  .fs-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .fs-status.bad { color: #ffb3ba; }
`;

const IMAGES = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
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

export function mount(root, context) {
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
      <button type="button" data-new>New folder</button>
      <input type="file" data-file hidden />
    </header>
    <form class="fs-new" hidden>
      <input name="name" placeholder="Folder name" aria-label="Folder name" autocomplete="off" />
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
    <p class="fs-status" role="status"></p>
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
  const newInput = newForm.querySelector("input");
  const renameForm = box.querySelector(".fs-rename");
  const renameInput = renameForm.querySelector("input");
  const confirmBox = box.querySelector(".fs-confirm");
  const confirmText = box.querySelector("[data-confirm-text]");
  const uploadInput = box.querySelector("[data-file]");
  const trashButton = box.querySelector("[data-trash]");
  const newButton = box.querySelector("[data-new]");
  const uploadButton = box.querySelector("[data-upload]");

  let current = "/";
  let nextCursor = null;
  let confirmPath = null;
  let renamePath = null;
  let pendingUpload = null;
  let view = "files";
  let disposed = false;
  let previewState = null;
  let editor = null;
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

  function normalizePath(input) {
    let path = input.trim();
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

  function navigate(input) {
    const path = normalizePath(input);
    if (path === null) {
      setStatus("The path cannot escape the root.", true);
      return;
    }
    address.value = path;
    void load(path);
  }

  function destroyEditor() {
    if (!editor) return;
    editor.view.destroy();
    editor.overlay?.remove();
    editor = null;
  }

  function resetPreview() {
    destroyEditor();
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
    entryButton.innerHTML = `<span class="glyph ${entry.kind === "dir" ? "dir" : "file"}"></span><span class="name">${escape(entry.name)}</span><span class="size">${entry.kind === "dir" ? "" : bytes(entry.size)}</span>`;
    entryButton.addEventListener("click", () => {
      if (entry.kind === "dir") void load(full);
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
        copy.innerHTML = `<span class="glyph ${entry.kind === "dir" ? "dir" : "file"}"></span><span class="name">${escape(entry.original)}</span><span class="size">${entry.kind === "dir" ? "" : bytes(entry.size)}</span>`;
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

  async function open(path, entry) {
    setStatus("Reading…");
    destroyEditor();
    if (previewState) previewState.editing = false;
    try {
      const data = await request(`/file?path=${encodeURIComponent(path)}`);
      if (disposed) return;
      if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
      const decoded = text(data);
      previewState = {
        path,
        entry,
        bytes: data,
        text: decoded,
        editing: false,
        objectUrl: null,
      };
      renderPreview();
      setStatus(`${path} · ${data.length} bytes`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  function download() {
    if (!previewState) return;
    const name = baseName(previewState.path);
    const extension = name.split(".").pop()?.toLowerCase() ?? "";
    const blob = new Blob([new Uint8Array(previewState.bytes)], {
      type: IMAGES[extension] ?? "application/octet-stream",
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function editorDoc(text) {
    const lines = text.split("\n");
    return basicSchema.node(
      "doc",
      null,
      lines.map((line) =>
        basicSchema.node(
          "paragraph",
          null,
          line ? [basicSchema.text(line)] : [],
        ),
      ),
    );
  }

  function editorText(doc) {
    const lines = [];
    doc.forEach((node) => lines.push(node.textContent));
    return lines.join("\n");
  }

  function startEditor() {
    if (!previewState || previewState.text === null) return;
    if (editor) return;
    const state = EditorState.create({
      schema: basicSchema,
      doc: editorDoc(previewState.text),
      plugins: [
        history(),
        keymap({
          "Mod-z": undo,
          "Mod-y": redo,
          "Shift-Mod-z": redo,
          ...baseKeymap,
        }),
      ],
    });
    const mountPoint = document.createElement("div");
    const view = new EditorView(mountPoint, { state });
    editor = { view, floating: false, overlay: null };
    previewState.editing = true;
    renderPreview();
  }

  function destroyEditor() {
    if (!editor) return;
    editor.overlay?.remove();
    editor.view.destroy();
    editor = null;
  }

  function cancelEditor() {
    if (previewState) previewState.editing = false;
    destroyEditor();
    renderPreview();
  }

  async function saveEditor() {
    if (!editor || !previewState) return;
    await saveEdit(editorText(editor.view.state.doc));
  }

  function floatEditor() {
    if (!editor || editor.floating) return;
    const overlay = document.createElement("section");
    overlay.className = "fs-editor-float";
    const header = document.createElement("header");
    header.className = "fs-editor-head";
    const title = document.createElement("strong");
    title.textContent = baseName(previewState.path);
    const hint = document.createElement("span");
    hint.className = "hint";
    hint.textContent = "ProseMirror · floating";
    const tools = document.createElement("div");
    tools.className = "tools";
    const save = button("", "Save");
    save.addEventListener("click", () => void saveEditor());
    const dock = button("", "Dock");
    dock.addEventListener("click", () => dockEditor());
    const close = button("", "Close");
    close.addEventListener("click", () => cancelEditor());
    tools.append(save, dock, close);
    header.append(title, hint, tools);
    const body = document.createElement("div");
    body.className = "fs-editor-body";
    body.append(editor.view.dom);
    overlay.append(header, body);
    box.append(overlay);

    let dragging = null;
    header.addEventListener("pointerdown", (event) => {
      if (event.target.closest("button")) return;
      dragging = {
        pointer: event.pointerId,
        x: event.clientX - overlay.offsetLeft,
        y: event.clientY - overlay.offsetTop,
      };
      header.setPointerCapture(event.pointerId);
    });
    header.addEventListener("pointermove", (event) => {
      if (!dragging || dragging.pointer !== event.pointerId) return;
      const left = Math.min(
        Math.max(0, event.clientX - dragging.x),
        Math.max(0, box.clientWidth - 120),
      );
      const top = Math.min(
        Math.max(0, event.clientY - dragging.y),
        Math.max(0, box.scrollHeight - 80),
      );
      overlay.style.left = `${left}px`;
      overlay.style.top = `${top}px`;
    });
    const stop = () => {
      dragging = null;
    };
    header.addEventListener("pointerup", stop);
    header.addEventListener("pointercancel", stop);

    editor.floating = true;
    editor.overlay = overlay;
    renderPreview();
  }

  function dockEditor() {
    if (!editor || !editor.floating) return;
    editor.overlay?.remove();
    editor.overlay = null;
    editor.floating = false;
    renderPreview();
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
    const { path, entry, text: decoded, editing } = previewState;
    const extension = path.split(".").pop()?.toLowerCase() ?? "";
    const meta = document.createElement("div");
    meta.className = "fs-meta";
    meta.append(
      cell("", kindLabel(entry.kind)),
      cell("", bytes(entry.size)),
      cell("", `modified ${modified(entry["modified-ms"])}`),
    );

    const actions = document.createElement("div");
    actions.className = "fs-actions";
    if (!editing) {
      if (decoded !== null) {
        const edit = button("", "Edit");
        edit.addEventListener("click", startEditor);
        actions.append(edit);
      }
      const save = button("", "Download");
      save.addEventListener("click", download);
      actions.append(save);
    }

    const pane = document.createElement("div");
    pane.className = "pane";
    if (editing) {
      if (editor?.floating) {
        const hint = document.createElement("p");
        hint.className = "hint";
        hint.textContent = "Editing in the floating ProseMirror window.";
        pane.append(hint);
        const dock = button("", "Dock");
        dock.addEventListener("click", dockEditor);
        actions.append(dock);
      } else {
        const host = document.createElement("div");
        host.className = "pm-host";
        if (editor) host.append(editor.view.dom);
        pane.append(host);
        const save = button("", "Save");
        save.addEventListener("click", () => void saveEditor());
        const cancel = button("", "Cancel");
        cancel.addEventListener("click", cancelEditor);
        const float = button("", "Float");
        float.addEventListener("click", floatEditor);
        actions.append(save, cancel, float);
      }
    } else if (decoded !== null) {
      const pre = document.createElement("pre");
      pre.textContent = decoded;
      pane.append(pre);
    } else if (IMAGES[extension]) {
      if (previewState.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
      previewState.objectUrl = URL.createObjectURL(
        new Blob([new Uint8Array(previewState.bytes)], { type: IMAGES[extension] }),
      );
      const image = document.createElement("img");
      image.src = previewState.objectUrl;
      image.alt = baseName(path);
      pane.append(image);
    } else {
      const hint = document.createElement("p");
      hint.className = "hint";
      hint.textContent = `Binary file · ${previewState.bytes.length} bytes`;
      pane.append(hint);
    }

    preview.append(meta, actions, pane);
  }

  async function saveEdit(value) {
    if (!previewState) return;
    const data = Array.from(encoder.encode(value));
    setStatus("Saving…");
    try {
      await request("/file", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path: previewState.path, bytes: data }),
      });
      previewState.bytes = data;
      previewState.text = value;
      previewState.editing = false;
      destroyEditor();
      renderPreview();
      setStatus(`Saved ${previewState.path}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
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
    if (!newForm.hidden) newInput.focus();
  });
  box.querySelector("[data-cancel-new]").addEventListener("click", () => {
    newForm.hidden = true;
  });
  box.querySelector("[data-cancel-rename]").addEventListener("click", clearRename);
  newForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = newInput.value.trim();
    if (!name || name.includes("/") || name === "." || name === "..") {
      setStatus("A folder name cannot be empty or contain `/`.", true);
      return;
    }
    try {
      await request("/dir", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path: join(current, name) }),
      });
      newForm.hidden = true;
      newInput.value = "";
      await load(current);
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
  uploadInput.addEventListener("change", () => {
    const file = uploadInput.files?.[0];
    uploadInput.value = "";
    if (file) void upload(file, false);
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
    destroyEditor();
    if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
    root.replaceChildren();
  };
}

function reason(error) {
  return error instanceof Error ? error.message : String(error);
}
