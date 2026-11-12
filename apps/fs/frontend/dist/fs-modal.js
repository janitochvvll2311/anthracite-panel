// src/api.ts
async function call(context, route, init = {}) {
  const { headers, ...rest } = init;
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}${route}`, { ...rest, headers: { accept: "application/json", ...headers ?? {} } });
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message = value && typeof value === "object" && value.error ? `${value.error.code}: ${value.error.message}` : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

// src/paths.ts
function join(path, name) {
  return path === "/" ? `/${name}` : `${path}/${name}`;
}
function parent(path) {
  if (path === "/")
    return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}
function baseName(path) {
  return path === "/" ? "/" : path.substring(path.lastIndexOf("/") + 1);
}
function normalizePath(input) {
  let path = String(input).trim();
  if (!path)
    path = "/";
  if (!path.startsWith("/"))
    path = `/${path}`;
  const parts = [];
  for (const part of path.split("/")) {
    if (!part || part === ".")
      continue;
    if (part === "..") {
      if (!parts.length)
        return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return `/${parts.join("/")}`;
}

// src/util.ts
var encoder = new TextEncoder;
var escape = (value) => String(value).replace(/[&<>"']/g, (character) => ({
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;"
})[character]);
function reason(error) {
  return error instanceof Error ? error.message : String(error);
}

// src/modal.ts
var STYLES = `
  .fs-modal { display: flex; flex-direction: column; gap: 14px; }
  .fs-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .fs-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; overflow-wrap: anywhere; }
  .fs-modal form { display: grid; gap: 10px; }
  .fs-modal .row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .fs-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .fs-modal input, .fs-modal select {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .fs-modal input:focus, .fs-modal select:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .fs-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .fs-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .fs-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .fs-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .fs-modal button:disabled { opacity: .5; cursor: default; }
  .fs-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .fs-modal-status.bad { color: #ffb3ba; }
  @media (max-width: 520px) { .fs-modal .row { grid-template-columns: 1fr; } }
`;
function setStatus(node, message, bad = false) {
  node.textContent = message ?? "";
  node.className = bad ? "fs-modal-status bad" : "fs-modal-status";
}
function newEntry(box, context) {
  const base = typeof context.params?.path === "string" ? context.params.path : "/";
  box.innerHTML = `<div><h3>New entry</h3><p>Create an entry in <strong>${escape(base)}</strong>.</p></div>
    <form>
      <div class="row">
        <label>Type
          <select data-kind aria-label="New entry type">
            <option value="file">File</option>
            <option value="dir">Folder</option>
            <option value="link">Symlink</option>
          </select>
        </label>
        <label>Name
          <input data-name placeholder="name" aria-label="Name" autocomplete="off" />
        </label>
      </div>
      <label data-target-row hidden>Target path
        <input data-target placeholder="/absolute/path" aria-label="Symlink target" autocomplete="off" />
      </label>
      <div class="fs-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Create</button>
      </div>
    </form>
    <p class="fs-modal-status" role="status"></p>`;
  const form = box.querySelector("form");
  const kind = box.querySelector("[data-kind]");
  const name = box.querySelector("[data-name]");
  const targetRow = box.querySelector("[data-target-row]");
  const target = box.querySelector("[data-target]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".fs-modal-status");
  const sync = () => {
    const link = kind.value === "link";
    targetRow.hidden = !link;
    name.placeholder = link ? "Link name" : kind.value === "dir" ? "Folder name" : "File name";
    if (!link)
      target.value = "";
  };
  kind.addEventListener("change", sync);
  sync();
  name.focus();
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = name.value.trim();
    if (!value || value.includes("/") || value === "." || value === "..") {
      setStatus(status, "A name cannot be empty or contain `/`.", true);
      return;
    }
    const path = join(base, value);
    submit.disabled = true;
    try {
      if (kind.value === "dir") {
        await call(context, "/dir", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path })
        });
      } else if (kind.value === "link") {
        const raw = target.value.trim();
        if (!raw.startsWith("/")) {
          setStatus(status, "The symlink target must be an absolute path.", true);
          submit.disabled = false;
          return;
        }
        const resolved = normalizePath(raw);
        if (resolved === null) {
          setStatus(status, "The path cannot escape the root.", true);
          submit.disabled = false;
          return;
        }
        await call(context, "/symlink", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path, target: resolved })
        });
      } else {
        try {
          await call(context, `/stat?path=${encodeURIComponent(path)}`);
          setStatus(status, `${path} already exists.`, true);
          submit.disabled = false;
          return;
        } catch (error) {
          if (!reason(error).includes("not-found")) {
            setStatus(status, reason(error), true);
            submit.disabled = false;
            return;
          }
        }
        await call(context, "/file", {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ path, bytes: [] })
        });
      }
      context.close();
    } catch (error) {
      submit.disabled = false;
      setStatus(status, reason(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  return () => box.replaceChildren();
}
function renameEntry(box, context) {
  const path = typeof context.params?.path === "string" ? context.params.path : "";
  box.innerHTML = `<div><h3>Rename</h3><p>${escape(path)}</p></div>
    <form>
      <label>New name
        <input data-name value="${escape(baseName(path))}" aria-label="New name" autocomplete="off" />
      </label>
      <div class="fs-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Rename</button>
      </div>
    </form>
    <p class="fs-modal-status" role="status"></p>`;
  const form = box.querySelector("form");
  const name = box.querySelector("[data-name]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".fs-modal-status");
  submit.disabled = !path;
  name.focus();
  name.select();
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!path) {
      setStatus(status, "The entry is missing.", true);
      return;
    }
    const value = name.value.trim();
    if (!value || value.includes("/") || value === "." || value === "..") {
      setStatus(status, "A name cannot be empty or contain `/`.", true);
      return;
    }
    submit.disabled = true;
    try {
      await call(context, "/rename", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          source: path,
          destination: join(parent(path), value)
        })
      });
      context.close();
    } catch (error) {
      submit.disabled = false;
      setStatus(status, reason(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  return () => box.replaceChildren();
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "fs-modal";
  root.append(box);
  let disposeInner;
  switch (context.contribution.tag) {
    case "fs-rename-modal":
      disposeInner = renameEntry(box, context);
      break;
    default:
      disposeInner = newEntry(box, context);
  }
  return () => {
    disposeInner();
    root.replaceChildren();
  };
}
export {
  mount
};
