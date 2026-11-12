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

// src/modal.ts
var STYLES = `
  .acc-modal { display: flex; flex-direction: column; gap: 14px; }
  .acc-modal h3 { margin: 0; font-size: 18px; font-weight: 600; }
  .acc-modal p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .acc-modal form { display: grid; gap: 10px; }
  .acc-modal .row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .acc-modal label { display: grid; gap: 5px; color: #8f98ab; font-size: 11.5px; }
  .acc-modal .check { display: flex; align-items: center; gap: 7px; font-size: 12px; }
  .acc-modal input:not([type=checkbox]), .acc-modal select {
    padding: 8px 10px; border-radius: 7px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: inherit; font-size: 12.5px;
  }
  .acc-modal input:focus, .acc-modal select:focus { outline: 2px solid #7aa2ff; outline-offset: 1px; }
  .acc-modal-actions { display: flex; justify-content: flex-end; gap: 8px; }
  .acc-modal button {
    padding: 7px 12px; border-radius: 7px; border: 1px solid #2a3142;
    background: #151a24; color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .acc-modal button[type="submit"] { border-color: #3c4e73; background: #1c2840; color: #dfe7ff; }
  .acc-modal button:hover:not(:disabled) { border-color: #4a5c82; color: #fff; }
  .acc-modal button:disabled { opacity: .5; cursor: default; }
  .acc-modal-status { min-height: 16px; color: #667087; font-size: 11.5px; }
  .acc-modal-status.bad { color: #ffb3ba; }
  @media (max-width: 520px) { .acc-modal .row { grid-template-columns: 1fr; } }
`;
function shell(title, copy, body) {
  const box = document.createElement("div");
  box.className = "acc-modal";
  box.innerHTML = `<div><h3>${title}</h3><p>${copy}</p></div>${body}<p class="acc-modal-status" role="status"></p>`;
  return box;
}
function statusOf(box) {
  return box.querySelector(".acc-modal-status");
}
function setStatus(node, message, bad = false) {
  node.textContent = message ?? "";
  node.className = bad ? "acc-modal-status bad" : "acc-modal-status";
}
async function policy(context) {
  return call(context, "/policy");
}
function userForm(box, context) {
  box.innerHTML = `<div><h3>Create user</h3><p>The account is written to the host's local user database.</p></div>
    <form>
      <div class="row">
        <label>Name <input data-name placeholder="alice" aria-label="User name" autocomplete="off" /></label>
        <label>Description <input data-gecos placeholder="Alice" aria-label="Description" autocomplete="off" /></label>
      </div>
      <div class="row">
        <label>UID <input data-uid type="number" min="1" placeholder="automatic" aria-label="UID" /></label>
        <label>GID <input data-gid type="number" min="1" placeholder="automatic" aria-label="GID" /></label>
      </div>
      <label>Home <input data-home placeholder="/home/name" aria-label="Home" autocomplete="off" /></label>
      <label>Shell <input data-shell value="/bin/sh" aria-label="Shell" autocomplete="off" /></label>
      <label class="check"><input type="checkbox" data-system /> system account</label>
      <div class="acc-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Create user</button>
      </div>
    </form>
    <p class="acc-modal-status" role="status"></p>`;
  const form = box.querySelector("form");
  const name = box.querySelector("[data-name]");
  const gecos = box.querySelector("[data-gecos]");
  const uid = box.querySelector("[data-uid]");
  const gid = box.querySelector("[data-gid]");
  const home = box.querySelector("[data-home]");
  const shell = box.querySelector("[data-shell]");
  const system = box.querySelector("[data-system]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = statusOf(box);
  let settings = null;
  submit.disabled = true;
  policy(context).then((value) => {
    settings = value;
    home.placeholder = `${value["home-root"] ?? "/home"}/name`;
    shell.value = value["default-shell"] ?? "/bin/sh";
    system.disabled = value["allow-system"] !== true;
    submit.disabled = value.control !== true;
    if (value.control !== true)
      setStatus(status, "Account control is disabled.", true);
    name.focus();
  }).catch((error) => {
    submit.disabled = false;
    setStatus(status, String(error), true);
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = name.value.trim();
    if (!value) {
      setStatus(status, "Write a user name first.", true);
      return;
    }
    submit.disabled = true;
    setStatus(status, `Creating ${value}…`);
    try {
      await call(context, "/user", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: value,
          uid: uid.value.trim() ? Number(uid.value) : null,
          gid: gid.value.trim() ? Number(gid.value) : null,
          gecos: gecos.value.trim(),
          home: home.value.trim() || `${settings?.["home-root"] ?? "/home"}/${value}`,
          shell: shell.value.trim() || settings?.["default-shell"] || "/bin/sh",
          system: system.checked
        })
      });
      context.close();
    } catch (error) {
      submit.disabled = false;
      setStatus(status, String(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  return () => box.replaceChildren();
}
function groupForm(box, context) {
  box.innerHTML = `<div><h3>Create group</h3><p>The group is written to the host's local group database.</p></div>
    <form>
      <div class="row">
        <label>Name <input data-name placeholder="developers" aria-label="Group name" autocomplete="off" /></label>
        <label>GID <input data-gid type="number" min="1" placeholder="automatic" aria-label="GID" /></label>
      </div>
      <label class="check"><input type="checkbox" data-system /> system group</label>
      <div class="acc-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Create group</button>
      </div>
    </form>
    <p class="acc-modal-status" role="status"></p>`;
  const form = box.querySelector("form");
  const name = box.querySelector("[data-name]");
  const gid = box.querySelector("[data-gid]");
  const system = box.querySelector("[data-system]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = statusOf(box);
  submit.disabled = true;
  policy(context).then((value) => {
    system.disabled = value["allow-system"] !== true;
    submit.disabled = value.control !== true;
    if (value.control !== true)
      setStatus(status, "Account control is disabled.", true);
    name.focus();
  }).catch((error) => {
    submit.disabled = false;
    setStatus(status, String(error), true);
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = name.value.trim();
    if (!value) {
      setStatus(status, "Write a group name first.", true);
      return;
    }
    submit.disabled = true;
    setStatus(status, `Creating group ${value}…`);
    try {
      await call(context, "/group", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: value,
          gid: gid.value.trim() ? Number(gid.value) : null,
          system: system.checked
        })
      });
      context.close();
    } catch (error) {
      submit.disabled = false;
      setStatus(status, String(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  return () => box.replaceChildren();
}
function passwordForm(box, context) {
  const params = context.params ?? {};
  const name = params.name ?? "";
  box.innerHTML = `<div><h3>Set password</h3><p>Change the password for <strong></strong>.</p></div>
    <form>
      <label>New password
        <input type="password" data-password aria-label="New password" autocomplete="new-password" />
      </label>
      <div class="acc-modal-actions">
        <button type="button" data-cancel>Cancel</button>
        <button type="submit" data-submit>Set password</button>
      </div>
    </form>
    <p class="acc-modal-status" role="status"></p>`;
  box.querySelector("p strong").textContent = name || "this account";
  const form = box.querySelector("form");
  const input = box.querySelector("[data-password]");
  const submit = box.querySelector("[data-submit]");
  const cancel = box.querySelector("[data-cancel]");
  const status = statusOf(box);
  submit.disabled = !name;
  input.focus();
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!name) {
      setStatus(status, "The account is missing.", true);
      return;
    }
    if (!input.value) {
      setStatus(status, "Write a password first.", true);
      return;
    }
    submit.disabled = true;
    setStatus(status, `Setting the password for ${name}…`);
    try {
      await call(context, "/password", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name, password: input.value })
      });
      context.close();
    } catch (error) {
      submit.disabled = false;
      setStatus(status, String(error), true);
    }
  });
  cancel.addEventListener("click", () => context.close());
  return () => box.replaceChildren();
}
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = STYLES;
  root.append(style);
  const box = shell("", "", "");
  root.append(box);
  let disposeInner;
  switch (context.contribution.tag) {
    case "accounts-user-modal":
      disposeInner = userForm(box, context);
      break;
    case "accounts-group-modal":
      disposeInner = groupForm(box, context);
      break;
    case "accounts-password-modal":
      disposeInner = passwordForm(box, context);
      break;
    default:
      disposeInner = () => box.replaceChildren();
  }
  return () => {
    disposeInner();
    root.replaceChildren();
  };
}
export {
  mount
};
