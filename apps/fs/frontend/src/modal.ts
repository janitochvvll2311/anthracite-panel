import { call } from "./api";
import { baseName, join, normalizePath, parent } from "./paths";
import type { ModalContext } from "./types";
import { escape, reason } from "./util";

const STYLES = `
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

function setStatus(node: HTMLElement, message: string, bad = false): void {
  node.textContent = message ?? "";
  node.className = bad ? "fs-modal-status bad" : "fs-modal-status";
}

function newEntry(box: HTMLElement, context: ModalContext): () => void {
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
  const form = box.querySelector("form") as HTMLFormElement;
  const kind = box.querySelector("[data-kind]") as HTMLSelectElement;
  const name = box.querySelector("[data-name]") as HTMLInputElement;
  const targetRow = box.querySelector("[data-target-row]") as HTMLElement;
  const target = box.querySelector("[data-target]") as HTMLInputElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  const cancel = box.querySelector("[data-cancel]") as HTMLButtonElement;
  const status = box.querySelector(".fs-modal-status") as HTMLElement;

  const sync = (): void => {
    const link = kind.value === "link";
    targetRow.hidden = !link;
    name.placeholder = link ? "Link name" : kind.value === "dir" ? "Folder name" : "File name";
    if (!link) target.value = "";
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
          body: JSON.stringify({ path }),
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
          body: JSON.stringify({ path, target: resolved }),
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
          body: JSON.stringify({ path, bytes: [] }),
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

function renameEntry(box: HTMLElement, context: ModalContext): () => void {
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
  const form = box.querySelector("form") as HTMLFormElement;
  const name = box.querySelector("[data-name]") as HTMLInputElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  const cancel = box.querySelector("[data-cancel]") as HTMLButtonElement;
  const status = box.querySelector(".fs-modal-status") as HTMLElement;
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
          destination: join(parent(path), value),
        }),
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

export function mount(root: HTMLElement, context: ModalContext): () => void {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "fs-modal";
  root.append(box);
  let disposeInner: () => void;
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
