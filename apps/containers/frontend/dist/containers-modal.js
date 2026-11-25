// src/api.ts
function route(context, path) {
  return `/api/apps/${encodeURIComponent(context.app.id)}${path}`;
}
async function call(context, path, init = {}) {
  const { headers, ...rest } = init;
  const response = await context.fetch(route(context, path), {
    ...rest,
    headers: { accept: "application/json", ...headers ?? {} }
  });
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

// src/state.ts
function parseEnv(lines) {
  return lines.split(`
`).map((line) => line.trim()).filter(Boolean).map((line) => {
    const index = line.indexOf("=");
    if (index <= 0)
      throw new Error(`environment line \`${line}\` needs KEY=value`);
    return { name: line.slice(0, index), value: line.slice(index + 1) };
  });
}
function parseVolumes(lines) {
  return lines.split(`
`).map((line) => line.trim()).filter(Boolean).map((line) => {
    const index = line.indexOf(":");
    if (index <= 0)
      throw new Error(`volume line \`${line}\` needs name:/path`);
    const name = line.slice(0, index);
    const path = line.slice(index + 1);
    if (name.includes("/"))
      throw new Error(`bind mounts are not supported: ${name}`);
    if (!path.startsWith("/"))
      throw new Error(`\`${path}\` is not an absolute path`);
    return { name, path };
  });
}
function parseLabels(lines) {
  return lines.split(`
`).map((line) => line.trim()).filter(Boolean).map((line) => {
    const index = line.indexOf("=");
    if (index <= 0)
      throw new Error(`label line \`${line}\` needs name=value`);
    return { name: line.slice(0, index), value: line.slice(index + 1) };
  });
}
function parseCommand(value) {
  const parts = [];
  let current = "";
  let quote = null;
  let started = false;
  for (const character of value) {
    if (quote) {
      if (character === quote)
        quote = null;
      else
        current += character;
      started = true;
    } else if (character === '"' || character === "'") {
      quote = character;
      started = true;
    } else if (/\s/.test(character)) {
      if (started) {
        parts.push(current);
        current = "";
        started = false;
      }
    } else {
      current += character;
      started = true;
    }
  }
  if (quote)
    throw new Error("unterminated quote in the command");
  if (started)
    parts.push(current);
  return parts;
}

// src/styles.ts
var styles = `
  .cn { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .cn-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .cn-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .cn button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn button:hover { border-color: var(--accent); }
  .cn button.primary { border-color: var(--accent); background: var(--panel-3); }
  .cn button.danger { border-color: var(--danger); color: var(--danger); }
  .cn button:disabled { opacity: .55; cursor: default; }
  .cn input[type="text"], .cn input[type="search"], .cn select {
    padding: 5px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 12px;
  }
  .cn-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .cn-status.bad { color: var(--danger); }
  .cn-tabs { display: flex; gap: 4px; flex-wrap: wrap; }
  .cn-tab {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--muted); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cn-tab.active { border-color: var(--accent); background: var(--panel-2); color: var(--text); }
  .cn-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(290px, 1fr)); gap: 10px; }
  .cn-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
    cursor: pointer;
  }
  .cn-card:hover { border-color: var(--line); }
  .cn-card.selected { border-color: var(--accent); }
  .cn-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .cn-card-head strong {
    font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cn-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .cn-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .cn-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .cn-badge.ok { border-color: var(--good); color: var(--good); }
  .cn-badge.warn { border-color: var(--warn); color: var(--warn); }
  .cn-badge.bad { border-color: var(--danger); color: var(--danger); }
  .cn-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .cn-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .cn-note { font-size: 12px; color: var(--muted); }
  .cn-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .cn-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .cn-detail-head strong { font-size: 13px; color: var(--text); }
  .cn-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .cn-facts { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }
  .cn-strip { display: flex; gap: 10px; flex-wrap: wrap; }
  .cn-strip span { font-size: 12px; color: var(--muted); }
  .cn-strip strong { color: var(--text); font-weight: 600; }
  .cn-log {
    margin: 0; padding: 10px; border-radius: 8px; border: 1px solid var(--line-soft);
    background: var(--bg); color: var(--text); font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px; line-height: 1.45; max-height: 320px; overflow: auto; white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cn-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(560px, calc(100vw - 32px));
  }
  .cn-dialog::backdrop { background: #05070bb3; }
  .cn-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .cn-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .cn-dialog .cn-actions { justify-content: flex-end; margin-top: 12px; }
  .cn-exec-out {
    margin: 0 0 8px; padding: 8px; min-height: 180px; max-height: 300px; overflow: auto;
    border-radius: 8px; border: 1px solid var(--line-soft); background: var(--bg);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .cn-exec-line { display: flex; gap: 6px; }
  .cn-exec-line input { flex: 1; }

  html[data-viewport="tablet"] .cn-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="mobile"] .cn-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .cn-actions { width: 100%; }
  html[data-viewport="mobile"] .cn-actions button { flex: 1 1 auto; }

  @container (max-width: 640px) {
    .cn-grid { grid-template-columns: minmax(0, 1fr); }
    .cn-actions button { flex: 1 1 auto; }
  }
`;

// src/modal.ts
var extra = `
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
function mount(root, context) {
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
  const setStatus = (message, bad = false) => {
    const node = box.querySelector("[data-status]");
    node.textContent = message;
    node.className = bad ? "cnm-status bad" : "cnm-status";
  };
  const text = (name) => box.querySelector(`[name="${name}"]`)?.value.trim() ?? "";
  const area = (name) => box.querySelector(`[name="${name}"]`)?.value ?? "";
  async function submit() {
    const button = box.querySelector("[data-submit]");
    button.disabled = true;
    try {
      const image = text("image");
      if (!image) {
        setStatus("An image reference is required.", true);
        return;
      }
      const command = parseCommand(text("command"));
      const ports = text("ports").split(/[,\s]+/).filter(Boolean);
      const network = text("network") || null;
      const request = {
        image,
        name: text("name") || null,
        command,
        env: parseEnv(area("env")),
        ports,
        volumes: parseVolumes(area("volumes")),
        network,
        restart: box.querySelector('[name="restart"]').value,
        labels: parseLabels(area("labels"))
      };
      setStatus("Creating…");
      await call(context, "/containers", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request)
      });
      const opened = await context.openView?.({
        tag: "anth-containers",
        params: { refresh: true }
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
    if (event.key === "Enter")
      submit();
  });
}
export {
  mount
};
