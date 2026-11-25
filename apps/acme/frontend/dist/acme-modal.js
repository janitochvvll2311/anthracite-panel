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
function directoryLabel(url) {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
function splitNames(value) {
  return value.split(/[,\s]+/).map((name) => name.trim()).filter(Boolean);
}
function isStaging(capabilities, url) {
  return capabilities.some((capability) => (capability["staging-directories"] ?? []).includes(url));
}

// src/styles.ts
var styles = `
  .ac { display: flex; flex-direction: column; gap: 14px; max-width: 100%; container-type: inline-size; }
  .ac-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-title { font-size: 14px; font-weight: 600; color: var(--text); }
  .ac-mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; color: var(--muted); }
  .grow { flex: 1; }
  .ac button {
    padding: 5px 10px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .ac button:hover { border-color: var(--accent); }
  .ac button.primary { border-color: var(--accent); background: var(--panel-3); }
  .ac button.danger { border-color: var(--danger); color: var(--danger); }
  .ac button:disabled { opacity: .55; cursor: default; }
  .ac-status { margin: 0; font-size: 12px; color: var(--muted); min-height: 16px; }
  .ac-status.bad { color: var(--danger); }
  .ac-section { display: flex; flex-direction: column; gap: 8px; }
  .ac-section-head { display: flex; align-items: center; gap: 8px; }
  .ac-section-head strong { font-size: 12px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .ac-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; }
  .ac-card {
    border: 1px solid var(--line-soft); border-radius: 10px; padding: 10px 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 6px; min-width: 0;
  }
  .ac-card.selectable { cursor: pointer; }
  .ac-card.selectable:hover { border-color: var(--line); }
  .ac-card.selected { border-color: var(--accent); }
  .ac-card-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ac-card-head strong {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px;
    color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ac-field { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .ac-field-name { font-size: 10px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted-2); }
  .ac-badge {
    padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line);
    font-size: 11px; color: var(--muted); white-space: nowrap;
  }
  .ac-badge.ok { border-color: var(--good); color: var(--good); }
  .ac-badge.warn { border-color: var(--warn); color: var(--warn); }
  .ac-badge.bad { border-color: var(--danger); color: var(--danger); }
  .ac-empty {
    padding: 14px; border: 1px dashed var(--line); border-radius: 10px;
    color: var(--muted); font-size: 12px;
  }
  .ac-banner {
    padding: 8px 10px; border-radius: 8px; border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, var(--panel)); color: var(--danger); font-size: 12px;
  }
  .ac-note { font-size: 12px; color: var(--muted); }
  .ac-detail {
    border: 1px solid var(--line); border-radius: 10px; padding: 12px;
    background: var(--panel); display: flex; flex-direction: column; gap: 10px;
  }
  .ac-detail-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ac-detail-head strong { font-size: 13px; color: var(--text); }
  .ac-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .ac-timeline { display: flex; flex-direction: column; gap: 0; }
  .ac-event {
    display: grid; grid-template-columns: 150px 120px minmax(0, 1fr); gap: 10px;
    padding: 6px 4px; border-bottom: 1px solid var(--line-soft); font-size: 12px;
  }
  .ac-event:last-child { border-bottom: 0; }
  .ac-event time { color: var(--muted); font-variant-numeric: tabular-nums; }
  .ac-event code { color: var(--accent); font-size: 11.5px; }
  .ac-event span { color: var(--text); overflow-wrap: anywhere; }
  .ac-confidence { font-size: 11.5px; color: var(--muted); }
  .ac-dialog {
    border: 1px solid var(--line); border-radius: 12px; padding: 14px;
    background: var(--panel); color: var(--text); width: min(420px, calc(100vw - 32px));
  }
  .ac-dialog::backdrop { background: #05070bb3; }
  .ac-dialog h3 { margin: 0 0 10px; font-size: 13px; }
  .ac-dialog label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--muted); }
  .ac-dialog select {
    padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .ac-dialog .ac-actions { justify-content: flex-end; margin-top: 12px; }

  html[data-viewport="tablet"] .ac-grid { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); }
  html[data-viewport="tablet"] .ac-event { grid-template-columns: 130px 110px minmax(0, 1fr); }

  html[data-viewport="mobile"] .ac-grid { grid-template-columns: minmax(0, 1fr); }
  html[data-viewport="mobile"] .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
  html[data-viewport="mobile"] .ac-actions { width: 100%; }
  html[data-viewport="mobile"] .ac-actions button { flex: 1 1 auto; }
  html[data-viewport="mobile"] .ac-detail { padding: 10px; }

  @container (max-width: 640px) {
    .ac-grid { grid-template-columns: minmax(0, 1fr); }
    .ac-event { grid-template-columns: minmax(0, 1fr); gap: 2px; }
    .ac-actions button { flex: 1 1 auto; }
  }
`;

// src/modal.ts
var extra = `
  .acm { display: flex; flex-direction: column; gap: 12px; }
  .acm label { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: 12px; }
  .acm input[type="text"], .acm input[type="email"], .acm select {
    width: 100%; padding: 6px 8px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--bg-soft); color: var(--text); font: inherit; font-size: 13px;
  }
  .acm-row { display: flex; gap: 14px; flex-wrap: wrap; }
  .acm-row > label { flex: 1 1 180px; }
  .acm-check { flex-direction: row; align-items: center; gap: 6px; }
  .acm-note { font-size: 11.5px; color: var(--muted); }
  .acm-actions { display: flex; gap: 8px; justify-content: flex-end; }
  .acm button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--panel-2); color: var(--text); font: inherit; font-size: 12px; cursor: pointer;
  }
  .acm button.primary { border-color: var(--accent); background: var(--panel-3); }
  .acm-status { margin: 0; font-size: 12px; color: var(--muted); }
  .acm-status.bad { color: var(--danger); }
  @media (max-width: 720px) {
    .acm-actions { flex-direction: column-reverse; }
    .acm-actions button { width: 100%; }
  }
`;
function escape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function status(box) {
  return box.querySelector("[data-status]");
}
function setStatus(box, message, bad = false) {
  const node = status(box);
  node.textContent = message;
  node.className = bad ? "acm-status bad" : "acm-status";
}
function text(box, name) {
  return box.querySelector(`[name="${name}"]`)?.value.trim() ?? "";
}
function checked(box, name) {
  return box.querySelector(`[name="${name}"]`)?.checked ?? false;
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles + extra;
  root.append(style);
  const box = document.createElement("div");
  box.className = "acm";
  box.dataset.testid = "acme-issue-modal";
  box.innerHTML = `
    <div class="acm-row">
      <label>CA directory<select name="directory" data-testid="acme-issue-directory"></select></label>
      <label>Contact (when a new account is created)<input type="text" name="contact" data-testid="acme-issue-contact" placeholder="ops@example.com" /></label>
    </div>
    <label>Names<input type="text" name="names" data-testid="acme-issue-names" placeholder="example.com, www.example.com, *.example.com" /></label>
    <div class="acm-row">
      <label>Challenge<select name="challenge" data-testid="acme-issue-challenge">
        <option value="http-01">HTTP-01</option>
        <option value="dns-01">DNS-01</option>
      </select></label>
      <label>Solver<select name="solver" data-testid="acme-issue-solver"></select></label>
      <label>Key type<select name="key-type" data-testid="acme-issue-key-type">
        <option value="ecdsa-p256">ECDSA P-256</option>
        <option value="rsa-2048">RSA 2048</option>
      </select></label>
    </div>
    <label class="acm-check"><input type="checkbox" name="auto-renew" data-testid="acme-issue-auto-renew" checked /> renew automatically from the certificate store</label>
    <label class="acm-check"><input type="checkbox" name="force" data-testid="acme-issue-force" /> allow a duplicate order (reissue without revoking)</label>
    <p class="acm-note" data-note></p>
    <div class="acm-actions">
      <button type="button" data-cancel data-testid="acme-modal-cancel">Cancel</button>
      <button type="button" class="primary" data-submit data-testid="acme-modal-submit">Issue certificate</button>
    </div>
    <p class="acm-status" role="status" data-status data-testid="acme-modal-status"></p>
  `;
  root.append(box);
  let accounts = [];
  let capabilities = [];
  let directories = [];
  let stagingDirectories = [];
  let solvers = [];
  let chosen = false;
  const directorySelect = box.querySelector('[name="directory"]');
  const challengeSelect = box.querySelector('[name="challenge"]');
  const solverSelect = box.querySelector('[name="solver"]');
  const note = box.querySelector("[data-note]");
  function staging(url) {
    return isStaging(capabilities, url);
  }
  function updateNote() {
    const url = directorySelect.value;
    const existing = accounts.find((account) => account.directory === url);
    const parts = [staging(url) ? "staging CA: never promoted" : "production CA"];
    parts.push(existing ? `using account ${existing.id}` : "a new account is created for this directory");
    if (challengeSelect.value === "dns-01") {
      parts.push("DNS-01 needs a solver; manual waits for your confirmation");
    }
    note.textContent = parts.join(" · ");
  }
  function updateSolvers() {
    const dns = challengeSelect.value === "dns-01";
    solverSelect.disabled = !dns;
    solverSelect.innerHTML = dns ? solvers.map((id) => `<option value="${escape(id)}">${escape(id)}</option>`).join("") : `<option value="">(not used by HTTP-01)</option>`;
    updateNote();
  }
  function fillOptions() {
    directorySelect.innerHTML = directories.map((url) => `<option value="${escape(url)}">${escape(directoryLabel(url))}${staging(url) ? " (staging)" : ""}</option>`).join("");
    updateSolvers();
  }
  async function submit() {
    const button = box.querySelector("[data-submit]");
    button.disabled = true;
    try {
      const url = directorySelect.value;
      const names = splitNames(text(box, "names"));
      if (names.length === 0) {
        setStatus(box, "Add at least one DNS name.", true);
        return;
      }
      if (!url) {
        setStatus(box, "Choose a CA directory.", true);
        return;
      }
      const challenge = challengeSelect.value;
      if (challenge === "http-01" && names.some((name) => name.startsWith("*."))) {
        setStatus(box, "A wildcard needs DNS-01.", true);
        return;
      }
      const keyType = box.querySelector('[name="key-type"]').value;
      let account = accounts.find((entry) => entry.directory === url);
      if (!account) {
        setStatus(box, "Creating the account…");
        account = await call(context, "/accounts", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            directory: url,
            contact: [text(box, "contact")].filter(Boolean),
            "key-type": keyType
          })
        });
      }
      setStatus(box, "Starting the order…");
      const order = await call(context, "/orders", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          account: account.id,
          names,
          challenge,
          solver: challenge === "dns-01" ? solverSelect.value : null,
          "key-type": keyType,
          "auto-renew": checked(box, "auto-renew"),
          staging: staging(url),
          force: checked(box, "force")
        })
      });
      const opened = await context.openView?.({
        tag: "anth-acme",
        params: { order: order.id }
      });
      if (!opened) {
        setStatus(box, "The panel did not open the order in the view.", true);
        return;
      }
      context.close();
    } catch (error) {
      setStatus(box, String(error), true);
    } finally {
      button.disabled = false;
    }
  }
  box.querySelector("[data-cancel]")?.addEventListener("click", () => {
    context.close();
  });
  box.querySelector("[data-submit]")?.addEventListener("click", () => {
    submit();
  });
  directorySelect.addEventListener("change", () => {
    chosen = true;
    updateNote();
  });
  challengeSelect.addEventListener("change", updateSolvers);
  (async () => {
    try {
      const [nextAccounts, nextCapabilities] = await Promise.all([
        call(context, "/accounts"),
        call(context, "/capabilities")
      ]);
      accounts = nextAccounts ?? [];
      capabilities = nextCapabilities ?? [];
      directories = capabilities[0]?.directories ?? [];
      stagingDirectories = capabilities[0]?.["staging-directories"] ?? [];
      solvers = capabilities.find((entry) => entry.challenge === "dns-01")?.solvers ?? [];
      if (directories.length === 0) {
        setStatus(box, "The service has no CA directory configured.", true);
        return;
      }
      const firstProduction = directories.find((url) => !staging(url));
      fillOptions();
      if (!chosen && firstProduction)
        directorySelect.value = firstProduction;
      updateSolvers();
    } catch (error) {
      setStatus(box, String(error), true);
    }
  })();
}
export {
  mount
};
