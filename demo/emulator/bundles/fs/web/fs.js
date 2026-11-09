// The demo Files view: a deterministic tree from the fs service.
//
// The module only knows the app's own routes. Every call goes through the
// frame's scoped fetch, so the panel origin and the cookie stay out of reach.

const styles = `
  .fs { display: flex; flex-direction: column; gap: 12px; min-height: 360px; color: #e8ebf2; }
  .fs-bar { display: flex; align-items: center; gap: 8px; }
  .fs-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fs-bar button:hover { border-color: #3a4560; color: #fff; }
  .fs-path {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-family: ui-monospace, monospace; font-size: 12px; color: #8f98ab;
  }
  .fs-body { display: grid; grid-template-columns: minmax(200px, 1fr) minmax(220px, 1.2fr); gap: 12px; min-height: 280px; }
  .fs-list { list-style: none; margin: 0; padding: 0; border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .fs-row { display: flex; align-items: center; gap: 6px; border-bottom: 1px solid #171c26; }
  .fs-row:last-child { border-bottom: 0; }
  .fs-entry {
    flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; padding: 8px 10px;
    border: 0; background: transparent; color: #c8d0df; font: inherit; font-size: 12.5px;
    text-align: left; cursor: pointer;
  }
  .fs-entry:hover { background: #151a24; color: #fff; }
  .fs-entry .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fs-entry .size { margin-left: auto; color: #667087; font-size: 11px; }
  .fs-entry .glyph { width: 14px; color: #7aa2ff; }
  .fs-delete {
    margin-right: 6px; border: 0; background: transparent; color: #667087; cursor: pointer;
    font-size: 14px; line-height: 1; padding: 4px;
  }
  .fs-delete:hover { color: #ff7d8a; }
  .fs-new { display: flex; gap: 6px; }
  .fs-new[hidden] { display: none; }
  .fs-new input {
    flex: 1; padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12px;
  }
  .fs-preview {
    border: 1px solid #202632; border-radius: 9px; background: #0d1016; padding: 10px 12px;
    overflow: auto; min-height: 280px;
  }
  .fs-preview pre { margin: 0; font: 12px/1.5 ui-monospace, monospace; white-space: pre-wrap; overflow-wrap: anywhere; color: #c8d0df; }
  .fs-preview .hint, .fs-status { color: #667087; font-size: 12px; }
  .fs-status { margin: 0; min-height: 18px; }
  .fs-status.bad { color: #ffb3ba; }
  .fs-count { color: #667087; font-size: 11px; }
`;

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

function bytes(size) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
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
    {
      ...rest,
      headers: { accept: "application/json", ...(headers ?? {}) },
    },
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

export function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "fs";
  box.innerHTML = `
    <header class="fs-bar">
      <button type="button" data-up title="Parent directory">↑</button>
      <span class="fs-path">/</span>
      <button type="button" data-new>New folder</button>
    </header>
    <form class="fs-new" hidden>
      <input name="name" placeholder="Folder name" aria-label="Folder name" autocomplete="off" />
      <button type="submit">Create</button>
    </form>
    <div class="fs-body">
      <ul class="fs-list" aria-label="Directory entries"></ul>
      <section class="fs-preview"><p class="hint">Select a file to preview it.</p></section>
    </div>
    <p class="fs-status" role="status"></p>
  `;
  root.append(box);

  const pathLabel = box.querySelector(".fs-path");
  const list = box.querySelector(".fs-list");
  const preview = box.querySelector(".fs-preview");
  const status = box.querySelector(".fs-status");
  const form = box.querySelector(".fs-new");
  const input = form.querySelector("input");
  let current = "/";
  let disposed = false;

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "fs-status bad" : "fs-status";
  };

  const request = (route, init) => call(context, route, init);

  async function load(path) {
    setStatus("Loading…");
    try {
      const page = await request(
        `/files?path=${encodeURIComponent(path)}&limit=200`,
      );
      if (disposed) return;
      current = path;
      pathLabel.textContent = path;
      list.replaceChildren();
      for (const entry of page.entries) {
        const row = document.createElement("li");
        row.className = "fs-row";
        const button = document.createElement("button");
        button.type = "button";
        button.className = "fs-entry";
        button.dataset.name = entry.name;
        button.dataset.kind = entry.kind;
        button.innerHTML = `<span class="glyph">${entry.kind === "dir" ? "▸" : "·"}</span><span class="name">${escape(entry.name)}</span><span class="size">${entry.kind === "dir" ? "" : bytes(entry.size)}</span>`;
        button.addEventListener("click", () => {
          if (entry.kind === "dir") {
            void load(join(path, entry.name));
          } else {
            void open(join(path, entry.name));
          }
        });
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "fs-delete";
        remove.title = `Move ${entry.name} to trash`;
        remove.textContent = "×";
        remove.addEventListener("click", () => {
          void drop(join(path, entry.name));
        });
        row.append(button, remove);
        list.append(row);
      }
      if (page.entries.length === 0) {
        const empty = document.createElement("li");
        empty.className = "fs-status";
        empty.textContent = "This directory is empty.";
        list.append(empty);
      }
      setStatus(`${page.entries.length} entr${page.entries.length === 1 ? "y" : "ies"}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function open(path) {
    setStatus("Reading…");
    try {
      const data = await request(`/file?path=${encodeURIComponent(path)}`);
      if (disposed) return;
      const decoded = text(data);
      preview.replaceChildren();
      if (decoded === null) {
        const hint = document.createElement("p");
        hint.className = "hint";
        hint.textContent = `Binary file · ${data.length} bytes`;
        preview.append(hint);
      } else {
        const pre = document.createElement("pre");
        pre.textContent = decoded;
        preview.append(pre);
      }
      setStatus(`${path} · ${data.length} bytes`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function drop(path) {
    try {
      await request(`/entry?path=${encodeURIComponent(path)}&trash=true`, {
        method: "DELETE",
      });
      await load(current);
      setStatus(`Moved ${path} to trash`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  box.querySelector("[data-up]").addEventListener("click", () => {
    if (current !== "/") void load(parent(current));
  });
  box.querySelector("[data-new]").addEventListener("click", () => {
    form.hidden = !form.hidden;
    if (!form.hidden) input.focus();
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = input.value.trim();
    if (!name || name.includes("/")) {
      setStatus("A folder name cannot be empty or contain `/`.", true);
      return;
    }
    try {
      await request("/dir", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path: join(current, name) }),
      });
      form.hidden = true;
      input.value = "";
      await load(current);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  });

  void load("/");

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
