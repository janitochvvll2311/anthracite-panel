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
function importCertificate(context, request) {
  return call(context, "/certs", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      "cert-pem": request.cert,
      "chain-pem": request.chain,
      "key-pem": request.key,
      names: request.names,
      "auto-renew": false
    })
  });
}

// src/modal.ts
var STYLES = `
  .cert-modal { display: flex; flex-direction: column; gap: 14px; }
  .cert-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .cert-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .cert-modal form { display: grid; gap: 10px; }
  .cert-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .cert-modal input[type="file"] { color: #c8d0df; font-size: 12px; }
  .cert-modal input[type="text"] {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .cert-modal .actions { display: flex; justify-content: flex-end; gap: 8px; }
  .cert-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .cert-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .cert-modal button:disabled { opacity: .5; cursor: default; }
  .cert-modal .warning { padding: 8px 10px; border-radius: 7px; border: 1px solid #5a4a2a; background: #221c10; color: #f3c969; font-size: 12px; }
  .cert-modal .status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .cert-modal .status.bad { color: #ffb3ba; }
`;
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = document.createElement("div");
  box.className = "cert-modal";
  box.innerHTML = `
    <div>
      <h3>Import a certificate</h3>
      <p>Paste or load the PEM chain and its private key. The pair is validated before anything is written.</p>
    </div>
    <div class="warning">
      The private key transits this browser and the panel process. Use ACME or the
      service import directory in production; the manager never renders the key back.
    </div>
    <form>
      <label>Certificate (leaf PEM)
        <input type="file" accept=".pem,.crt,text/plain" data-cert />
      </label>
      <label>Chain (optional, PEM)
        <input type="file" accept=".pem,.crt,text/plain" data-chain />
      </label>
      <label>Private key (PEM)
        <input type="file" accept=".pem,.key,text/plain" data-key />
      </label>
      <label>Names (optional, comma separated)
        <input type="text" data-names placeholder="example.com, www.example.com" autocomplete="off" />
      </label>
      <div class="actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Import and publish</button>
      </div>
    </form>
    <p class="status" role="status"></p>
  `;
  root.append(box);
  const form = box.querySelector("form");
  const certInput = box.querySelector("[data-cert]");
  const chainInput = box.querySelector("[data-chain]");
  const keyInput = box.querySelector("[data-key]");
  const namesInput = box.querySelector("[data-names]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = box.querySelector(".status");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "status bad" : "status";
  };
  const text = async (input) => {
    const file = input.files?.[0];
    return file ? file.text() : "";
  };
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    setStatus("Validating the pair…");
    try {
      const cert = await text(certInput);
      const chain = await text(chainInput);
      const key = await text(keyInput);
      if (!cert.trim() || !key.trim()) {
        throw new Error("Select a certificate and its private key.");
      }
      const names = namesInput.value.split(",").map((name) => name.trim()).filter(Boolean);
      await importCertificate(context, { cert, chain, key, names });
      if (!disposed)
        context.close();
    } catch (error) {
      submit.disabled = false;
      if (!disposed)
        setStatus(String(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  certInput.focus();
  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
export {
  mount
};
