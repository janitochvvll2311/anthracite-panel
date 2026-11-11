import { call } from "./api";
import { mountEditorView } from "./editor";
import { bytes, extensionOf, kindLabel, modified, text } from "./format";
import { IMAGES, iconMarkup, mimeFor } from "./icons";
import { baseName, join, normalizePath, parent } from "./paths";
import { formattedPreview } from "./preview";
import { styles } from "./styles";
import type { FrameContext } from "./types";
import { cell, escape, reason } from "./util";

export function mount(root: HTMLElement, context: FrameContext) {
  if (context.contribution.tag === "anth-fs-editor") {
    return mountEditorView(root, context);
  }

  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "fs";
  box.innerHTML = `
    <header class="fs-bar">
      <button type="button" data-up title="Parent directory" aria-label="Parent directory"><span class="fs-btn-glyph" aria-hidden="true"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3v10M4.5 6.5 8 3l3.5 3.5"/></svg></span><span class="fs-btn-label">Up</span></button>
      <input class="fs-address" list="fs-paths" aria-label="Path" spellcheck="false" autocomplete="off" />
      <datalist id="fs-paths"></datalist>
      <button type="button" data-refresh title="Refresh" aria-label="Refresh"><span class="fs-btn-glyph" aria-hidden="true"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M13 8a5 5 0 1 1-1.6-3.7"/><path d="M13 3v3h-3"/></svg></span><span class="fs-btn-label">Refresh</span></button>
      <span class="fs-break" aria-hidden="true"></span>
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

  const address = box.querySelector(".fs-address") as any;
  const paths = box.querySelector("#fs-paths") as any;
  const list = box.querySelector(".fs-list") as any;
  const preview = box.querySelector(".fs-preview") as any;
  const status = box.querySelector(".fs-status") as any;
  const up = box.querySelector("[data-up]") as any;
  const more = box.querySelector("[data-more]") as any;
  const newForm = box.querySelector(".fs-new") as any;
  const newKind = newForm.querySelector("select") as any;
  const newInput = newForm.querySelector("[name=name]") as any;
  const newTarget = newForm.querySelector("[name=target]") as any;
  const renameForm = box.querySelector(".fs-rename") as any;
  const renameInput = renameForm.querySelector("input") as any;
  const uploadInput = box.querySelector("[data-file]") as any;
  const trashButton = box.querySelector("[data-trash]") as any;
  const newButton = box.querySelector("[data-new]") as any;
  const uploadButton = box.querySelector("[data-upload]") as any;
  const queueBox = box.querySelector("[data-queue]") as any;
  const queueList = box.querySelector("[data-queue-list]") as any;
  const queueClear = box.querySelector("[data-queue-clear]") as any;
  const dropOverlay = box.querySelector("[data-drop]") as any;

  let current = "/";
  let nextCursor = null;
  let confirmPath = null;
  let renamePath = null;
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

  const request = (route: string, init: RequestInit = {}) => call(context, route, init);
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

    const rename = button("fs-tool", "", `Rename ${entry.name}`);
    rename.innerHTML =
      '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="m3 11 .5-2.5L10.8 1.2l2 2L5.5 10.5 3 11Z"/><path d="M9.5 2.5l2 2"/></svg>';
    rename.setAttribute("aria-label", `Rename ${entry.name}`);
    rename.addEventListener("click", () => showRename(full));

    const remove = button("fs-tool delete", "", `Delete ${entry.name}`);
    remove.innerHTML =
      '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="m4 4 8 8M12 4l-8 8"/></svg>';
    remove.setAttribute("aria-label", `Delete ${entry.name}`);
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

  function button(className, textValue, title = "") {
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
        empty.className = "fs-empty";
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
    confirmPath = null;
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
      const decoded = IMAGES[extensionOf(path)] ? null : text(data);
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
    const back = button("", "Files");
    back.innerHTML =
      '<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m10 3-5 5 5 5"/></svg><span>Files</span>';
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
    const entry = row?.querySelector(".fs-entry") as any;
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
    if ((row?.querySelector(".fs-entry") as any)?.dataset.kind === "dir") {
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
  window.addEventListener("dragleave", (event) => {
    if (!event.relatedTarget) hideDrop();
  });
  window.addEventListener("dragover", (event) => {
    if (hasFiles(event)) event.preventDefault();
  });
  window.addEventListener("drop", (event) => {
    if (hasFiles(event)) event.preventDefault();
    hideDrop();
  });

  renderPaths();
  void load("/");

  return () => {
    disposed = true;
    if (previewState?.objectUrl) URL.revokeObjectURL(previewState.objectUrl);
    root.replaceChildren();
  };
}
