import { call } from "./api";
import { parseEnv, parseLabels, parseVolumes } from "./state";
import { styles } from "./styles";
import type { ModalContext } from "./types";

const extra = `
  .cnm { display: flex; flex-direction: column; gap: 12px; }
  .cnm label { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: 12px; }
  .cnm textarea, .cnm input[type="text"], .cnm select {
    width: 100%; padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .cnm textarea { min-height: 56px; resize: vertical; }
  .cnm-row { display: flex; gap: 12px; flex-wrap: wrap; }
  .cnm-row > label { flex: 1 1 180px; }
  .cnm-note { font-size: 11.5px; color: var(--muted); }
  .cnm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .cnm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cnm button.primary { border-color: var(--accent); background: var(--panel-3); }
  .cnm-status { margin: 0; font-size: 12px; color: var(--muted); }
  .cnm-status.bad { color: var(--danger); }
  @media (max-width: 720px) { .cnm-actions { flex-direction: column-reverse; } .cnm-actions button { width: 100%; } }
`;

export function mount(root: HTMLElement, context: ModalContext) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);

  const box = document.createElement("div");
  box.className = "cnm";
  box.dataset.testid = "containers-create-form";
  box.innerHTML = `
    <div class="cnm-row">
      <label>Image<input type="text" name="image" data-testid="containers-create-image" placeholder="nginx:1.27" /></label>
      <label>Name (optional)<input type="text" name="name" data-testid="containers-create-name" placeholder="web" /></label>
    </div>
    <label>Command<input type="text" name="command" data-testid="containers-create-command" placeholder="nginx -g 'daemon off;'" /></label>
    <div class="cnm-row">
      <label>Ports<input type="text" name="ports" data-testid="containers-create-ports" placeholder="8080:80" /></label>
      <label>Network<input type="text" name="network" data-testid="containers-create-network" placeholder="app" /></label>
      <label>Restart<select name="restart" data-testid="containers-create-restart">
        <option value="no">no</option>
        <option value="unless-stopped">unless-stopped</option>
        <option value="always">always</option>
        <option value="on-failure">on-failure</option>
      </select></label>
    </div>
    <label>Environment (KEY=value, one per line)<textarea name="env" data-testid="containers-create-env" placeholder="TZ=UTC"></textarea></label>
    <label>Named volumes (name:/container/path, one per line)<textarea name="volumes" data-testid="containers-create-volumes" placeholder="site:/usr/share/nginx/html"></textarea></label>
    <label>Labels (name=value, one per line)<textarea name="labels" data-testid="containers-create-labels" placeholder="com.example=web"></textarea></label>
    <p class="cnm-note">Bind mounts and privileged mode are not offered; the service refuses both.</p>
    <div class="cnm-actions">
      <button type="button" data-cancel data-testid="containers-modal-cancel">Cancel</button>
      <button type="button" class="primary" data-submit data-testid="containers-modal-submit">Create container</button>
    </div>
    <p class="cnm-status" role="status" data-status data-testid="containers-modal-status"></p>
  `;
  root.append(box);

  const setStatus = (message: string, bad = false): void => {
    const node = box.querySelector("[data-status]") as HTMLElement;
    node.textContent = message;
    node.className = bad ? "cnm-status bad" : "cnm-status";
  };

  const text = (name: string): string =>
    box.querySelector<HTMLInputElement>(`[name="${name}"]`)?.value.trim() ?? "";
  const area = (name: string): string =>
    box.querySelector<HTMLTextAreaElement>(`[name="${name}"]`)?.value ?? "";

  async function submit(): Promise<void> {
    const button = box.querySelector("[data-submit]") as HTMLButtonElement;
    button.disabled = true;
    try {
      const image = text("image");
      if (!image) {
        setStatus("An image reference is required.", true);
        return;
      }
      const command = text("command")
        .split(/\s+/)
        .filter(Boolean);
      const ports = text("ports")
        .split(/[,\s]+/)
        .filter(Boolean);
      const network = text("network") || null;
      const request = {
        image,
        name: text("name") || null,
        command,
        env: parseEnv(area("env")),
        ports,
        volumes: parseVolumes(area("volumes")),
        network,
        restart: (
          box.querySelector('[name="restart"]') as HTMLSelectElement
        ).value,
        labels: parseLabels(area("labels")),
      };
      setStatus("Creating…");
      await call(context, "/containers", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request),
      });
      const opened = await context.openView?.({
        tag: "anth-containers",
        params: { refresh: true },
      });
      if (!opened) {
        setStatus("The panel did not open the container view.", true);
        return;
      }
      context.close();
    } catch (error) {
      setStatus(String(error), true);
    } finally {
      button.disabled = false;
    }
  }

  box.querySelector("[data-cancel]")?.addEventListener("click", () => context.close());
  box.querySelector("[data-submit]")?.addEventListener("click", () => void submit());
  box.querySelector('[name="image"]')?.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Enter") void submit();
  });
}
