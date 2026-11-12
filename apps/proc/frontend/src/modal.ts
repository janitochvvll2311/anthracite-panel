import { spawnProcess } from "./api";
import type { ModalContext } from "./types";

const STYLES = `
  .proc-modal { display: flex; flex-direction: column; gap: 14px; }
  .proc-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .proc-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .proc-modal form { display: grid; gap: 10px; }
  .proc-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .proc-modal input {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12.5px ui-monospace, monospace;
  }
  .proc-modal input:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .proc-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .proc-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .proc-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .proc-modal button:disabled { opacity: .5; cursor: default; }
  .proc-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .proc-modal-status.bad { color: #ffb3ba; }
`;

export function mount(root: HTMLElement, context: ModalContext): () => void {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);

  const box = document.createElement("div");
  box.className = "proc-modal";
  box.innerHTML = `
    <div>
      <h3>Spawn a process</h3>
      <p>The command runs detached from the process service. Shell syntax is not interpreted.</p>
    </div>
    <form>
      <label>Command
        <input data-command placeholder="sleep 30" aria-label="Detached command" autocomplete="off" />
      </label>
      <div class="proc-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Spawn</button>
      </div>
    </form>
    <p class="proc-modal-status" role="status"></p>
  `;
  root.append(box);

  const form = box.querySelector("form") as HTMLFormElement;
  const command = box.querySelector("[data-command]") as HTMLInputElement;
  const submit = box.querySelector("[data-submit]") as HTMLButtonElement;
  const cancel = box.querySelector("[data-cancel]") as HTMLButtonElement;
  const status = box.querySelector(".proc-modal-status") as HTMLElement;
  let disposed = false;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-modal-status bad" : "proc-modal-status";
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = command.value.trim();
    if (!value) {
      setStatus("Write a command first, for example `sleep 30`.", true);
      return;
    }
    const argv = value.split(/\s+/);
    submit.disabled = true;
    setStatus("Spawning…");
    try {
      await spawnProcess(context, argv);
      if (!disposed) context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed) setStatus(String(error), true);
    }
  });

  cancel.addEventListener("click", () => context.close());
  command.focus();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
