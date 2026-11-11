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

// src/format.ts
function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}
function table(headers, entries, rowClass = "") {
  const node = document.createElement("div");
  node.className = "acc-table";
  const head = document.createElement("div");
  head.className = `acc-row head${rowClass ? ` ${rowClass}` : ""}`;
  for (const label of headers)
    head.append(cell("", label));
  node.append(head);
  for (const entry of entries)
    node.append(entry);
  return node;
}

// src/styles.ts
var styles = `
  .acc { display: flex; flex-direction: column; gap: 15px; min-height: 360px; }
  .acc-bar { display: flex; align-items: center; gap: 10px; }
  .acc-bar .grow { flex: 1; }
  .acc-bar button {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .acc-bar button:hover { border-color: #3a4560; color: #fff; }
  .acc-section { display: grid; gap: 7px; }
  .acc-title { color: #667087; font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
  .acc-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .acc-row {
    display: grid; grid-template-columns: minmax(150px, 1.3fr) 55px 55px minmax(80px, .8fr) minmax(80px, .8fr) minmax(120px, 1fr) 90px minmax(190px, auto);
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .acc-row:last-child { border-bottom: 0; }
  .acc-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .acc-row.group { grid-template-columns: minmax(150px, 1fr) 70px minmax(200px, 3fr) minmax(80px, auto); }
  .acc-name { color: #e8ebf2; display: flex; align-items: center; gap: 7px; }
  .acc-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .acc-badge {
    padding: 1px 6px; border-radius: 999px; border: 1px solid #2a3142; font-size: 9.5px;
    text-transform: uppercase; letter-spacing: .07em; color: #8f98ab;
  }
  .acc-badge.system { border-color: #7aa2ff44; color: #9ab8ff; }
  .acc-badge.locked { border-color: #ff7d8a44; color: #ffb3ba; }
  .acc-badge.nss { border-color: #d8b45a44; color: #e5ca83; }
  .acc-badge.unknown { border-color: #8f98ab44; color: #b7bfce; }
  .acc-badge.policy.on { border-color: #6fcf9744; color: #9fe0b8; }
  .acc-badge.policy.off { border-color: #ff7d8a44; color: #ffb3ba; }
  .acc-policy { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .acc-members { color: #8f98ab; font-size: 11.5px; }
  .acc-actions { display: flex; gap: 4px; flex-wrap: wrap; }
  .acc-btn {
    padding: 3px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #8f98ab; font: inherit; font-size: 10.5px; cursor: pointer;
  }
  .acc-btn:hover { border-color: #3a4560; color: #fff; }
  .acc-btn.danger:hover { border-color: #ff7d8a66; color: #ffb3ba; }
  .acc-add { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .acc-add input, .acc-add button {
    padding: 6px 9px; border-radius: 7px; border: 1px solid #2a3142; background: #0f1219;
    color: #e8ebf2; font: 12px ui-monospace, monospace;
  }
  .acc-add input[data-name] { width: 120px; }
  .acc-add input[data-uid], .acc-add input[data-gid] { width: 76px; }
  .acc-add input[data-home] { flex: 1; min-width: 140px; }
  .acc-add input[data-gecos] { width: 140px; }
  .acc-add input[data-group-name] { width: 140px; }
  .acc-add input[data-shell] { width: 110px; }
  .acc-add button { background: #151a24; cursor: pointer; }
  .acc-add button:hover { border-color: #3a4560; color: #fff; }
  .acc-add .check { display: flex; align-items: center; gap: 5px; color: #8f98ab; font-size: 11px; }
  .acc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .acc-status.bad { color: #ffb3ba; }
`;

// src/main.ts
function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);
  const box = document.createElement("div");
  box.className = "acc";
  box.innerHTML = `
    <div class="acc-bar">
      <span class="acc-mono">users and groups</span>
      <span class="grow"></span>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <div class="acc-policy" data-policy></div>
    <section class="acc-section">
      <div class="acc-title">Users</div>
      <div data-users></div>
      <div data-password></div>
      <form class="acc-add" data-user-form>
        <input data-name placeholder="name" aria-label="User name" autocomplete="off" />
        <input data-uid type="number" min="1" placeholder="uid" aria-label="UID" />
        <input data-gid type="number" min="1" placeholder="gid" aria-label="GID" />
        <input data-gecos placeholder="description" aria-label="Description" autocomplete="off" />
        <input data-home placeholder="/home/name" aria-label="Home" autocomplete="off" />
        <input data-shell value="/bin/sh" aria-label="Shell" autocomplete="off" />
        <label class="check"><input type="checkbox" data-system /> system</label>
        <button type="submit">Create user</button>
      </form>
    </section>
    <section class="acc-section">
      <div class="acc-title">Groups</div>
      <div data-groups></div>
      <form class="acc-add" data-group-form>
        <input data-group-name placeholder="group name" aria-label="Group name" autocomplete="off" />
        <input data-group-gid type="number" min="1" placeholder="gid" aria-label="Group GID" />
        <label class="check"><input type="checkbox" data-group-system /> system</label>
        <button type="submit">Create group</button>
      </form>
    </section>
    <p class="acc-status" role="status"></p>
  `;
  root.append(box);
  const policyRoot = box.querySelector("[data-policy]");
  const usersRoot = box.querySelector("[data-users]");
  const groupsRoot = box.querySelector("[data-groups]");
  const passwordPanel = box.querySelector("[data-password]");
  const status = box.querySelector(".acc-status");
  const userForm = box.querySelector("[data-user-form]");
  const formName = box.querySelector("[data-name]");
  const formUid = box.querySelector("[data-uid]");
  const formGid = box.querySelector("[data-gid]");
  const formGecos = box.querySelector("[data-gecos]");
  const formHome = box.querySelector("[data-home]");
  const formShell = box.querySelector("[data-shell]");
  const formSystem = box.querySelector("[data-system]");
  const groupForm = box.querySelector("[data-group-form]");
  const groupName = box.querySelector("[data-group-name]");
  const groupGid = box.querySelector("[data-group-gid]");
  const groupSystem = box.querySelector("[data-group-system]");
  let disposed = false;
  let policy = {};
  let shellInitialized = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "acc-status bad" : "acc-status";
  };
  const policyBadge = (label, enabled) => {
    const badge = cell(`acc-badge policy ${enabled ? "on" : "off"}`, label);
    badge.title = enabled ? `${label} enabled` : `${label} disabled`;
    return badge;
  };
  function renderPolicy() {
    policyRoot.replaceChildren(policyBadge("control", policy.control), policyBadge("passwords", policy.passwords), policyBadge("system ids", policy["allow-system"]), policyBadge("privileged groups", policy["allow-privileged-groups"]), cell("acc-mono", `home ${policy["home-root"] ?? "/home"}`));
    const canControl = policy.control === true;
    formHome.placeholder = `${policy["home-root"] ?? "/home"}/name`;
    if (!shellInitialized) {
      formShell.value = policy["default-shell"] ?? "/bin/sh";
      shellInitialized = true;
    }
    userForm.querySelector("button").disabled = !canControl;
    groupForm.querySelector("button").disabled = !canControl;
    formSystem.disabled = policy["allow-system"] !== true;
    groupSystem.disabled = policy["allow-system"] !== true;
  }
  async function load() {
    setStatus("Reading accounts…");
    try {
      const [users, groups, settings] = await Promise.all([
        call(context, "/users"),
        call(context, "/groups"),
        call(context, "/policy")
      ]);
      if (disposed)
        return;
      policy = settings;
      renderPolicy();
      const userRows = users.map((user) => {
        const row = document.createElement("div");
        row.className = "acc-row";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", user.name));
        if (user.system)
          name.append(cell("acc-badge system", "system"));
        if (user.source === "nss")
          name.append(cell("acc-badge nss", "directory"));
        if (user.locked === true)
          name.append(cell("acc-badge locked", "locked"));
        else if (user.locked === null)
          name.append(cell("acc-badge unknown", "lock ?"));
        name.title = `${user.gecos || user.name} · ${user.groups.length ? user.groups.join(", ") : "no supplementary groups"}`;
        const actions = document.createElement("span");
        actions.className = "acc-actions";
        const local = user.source === "local";
        const protectedAccount = user.name === "root" || user.name === "operator" || !local;
        const lock = document.createElement("button");
        lock.type = "button";
        lock.className = "acc-btn";
        lock.textContent = user.locked === false ? "Lock" : "Unlock";
        lock.disabled = !local || user.name === "root" || !policy.control || user.locked === null;
        lock.title = lock.disabled ? "this account cannot be locked from the panel" : `${lock.textContent} ${user.name}`;
        lock.addEventListener("click", () => void toggleLock(user));
        const password = document.createElement("button");
        password.type = "button";
        password.className = "acc-btn";
        password.textContent = "Password";
        password.disabled = protectedAccount || !policy.passwords;
        password.title = password.disabled ? "password changes are disabled for this account" : `Set a password for ${user.name}`;
        password.addEventListener("click", () => openPassword(user));
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "acc-btn danger";
        remove.textContent = "Delete";
        remove.disabled = protectedAccount || !policy.control;
        remove.title = remove.disabled ? "this account is protected" : `Delete ${user.name} and keep the home`;
        remove.addEventListener("click", () => void deleteUser(user, false));
        const removeHome = document.createElement("button");
        removeHome.type = "button";
        removeHome.className = "acc-btn danger";
        removeHome.textContent = "Delete home";
        removeHome.disabled = protectedAccount || !policy.control;
        removeHome.title = removeHome.disabled ? "this account is protected" : `Delete ${user.name} and remove ${user.home}`;
        removeHome.addEventListener("click", () => void deleteUser(user, true));
        actions.append(lock, password, remove, removeHome);
        row.append(name, cell("acc-mono", String(user.uid)), cell("acc-mono", String(user.gid)), cell("acc-mono", user.group), cell("acc-members", user.groups.length ? user.groups.join(", ") : "—"), cell("acc-mono", user.home), cell("acc-mono", user.shell), actions);
        row.dataset.source = user.source;
        return row;
      });
      usersRoot.replaceChildren(table(["USER", "UID", "GID", "GROUP", "GROUPS", "HOME", "SHELL", "ACTIONS"], userRows));
      const groupRows = groups.map((group) => {
        const row = document.createElement("div");
        row.className = "acc-row group";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", group.name));
        if (group.system)
          name.append(cell("acc-badge system", "system"));
        if (group.source === "nss")
          name.append(cell("acc-badge nss", "directory"));
        const actions = document.createElement("span");
        actions.className = "acc-actions";
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "acc-btn danger";
        remove.textContent = "Delete";
        remove.disabled = group.source !== "local" || group.gid === 0 || !policy.control;
        remove.title = remove.disabled ? "this group cannot be deleted from the panel" : `Delete ${group.name}`;
        remove.addEventListener("click", () => void deleteGroup(group.name));
        actions.append(remove);
        row.append(name, cell("acc-mono", String(group.gid)), cell("acc-members", group.members.length ? group.members.join(", ") : "—"), actions);
        return row;
      });
      groupsRoot.replaceChildren(table(["GROUP", "GID", "MEMBERS", "ACTIONS"], groupRows, "group"));
      setStatus(`${users.length} users · ${groups.length} groups`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function toggleLock(user) {
    const locked = user.locked !== true;
    setStatus(`${locked ? "Locking" : "Unlocking"} ${user.name}…`);
    try {
      await call(context, "/lock", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name: user.name, locked })
      });
      await load();
      setStatus(`${user.name} ${locked ? "locked" : "unlocked"}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function deleteUser(user, removeHome) {
    if (!window.confirm(`Delete ${user.name}${removeHome ? ` and remove ${user.home}` : ""}?`)) {
      return;
    }
    setStatus(`Deleting ${user.name}…`);
    try {
      await call(context, `/user?name=${encodeURIComponent(user.name)}&remove-home=${removeHome}`, { method: "DELETE" });
      passwordPanel.replaceChildren();
      await load();
      setStatus(`Deleted ${user.name}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function deleteGroup(name) {
    if (!window.confirm(`Delete group ${name}?`))
      return;
    setStatus(`Deleting group ${name}…`);
    try {
      await call(context, `/group?name=${encodeURIComponent(name)}`, {
        method: "DELETE"
      });
      await load();
      setStatus(`Deleted group ${name}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  function openPassword(user) {
    const form = document.createElement("form");
    form.className = "acc-add";
    const input = document.createElement("input");
    input.type = "password";
    input.placeholder = `new password for ${user.name}`;
    input.autocomplete = "new-password";
    input.setAttribute("aria-label", "New password");
    const submit = document.createElement("button");
    submit.type = "submit";
    submit.textContent = "Set password";
    form.append(input, submit);
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const password = input.value;
      if (!password)
        return;
      setStatus(`Setting the password for ${user.name}…`);
      try {
        await call(context, "/password", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: user.name, password })
        });
        form.remove();
        await load();
        setStatus(`Password set for ${user.name}`);
      } catch (error) {
        if (!disposed)
          setStatus(String(error), true);
      }
    });
    passwordPanel.replaceChildren(form);
    input.focus();
  }
  userForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = formName.value.trim();
    const home = formHome.value.trim() || `${policy["home-root"] ?? "/home"}/${name}`;
    if (!name) {
      setStatus("Write a user name first.", true);
      return;
    }
    setStatus(`Creating ${name}…`);
    try {
      await call(context, "/user", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name,
          uid: formUid.value.trim() ? Number(formUid.value) : null,
          gid: formGid.value.trim() ? Number(formGid.value) : null,
          gecos: formGecos.value.trim(),
          home,
          shell: formShell.value.trim() || policy["default-shell"] || "/bin/sh",
          system: formSystem.checked
        })
      });
      formName.value = "";
      formUid.value = "";
      formGid.value = "";
      formGecos.value = "";
      formHome.value = "";
      formSystem.checked = false;
      await load();
      setStatus(`Created ${name}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  });
  groupForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = groupName.value.trim();
    if (!name) {
      setStatus("Write a group name first.", true);
      return;
    }
    setStatus(`Creating group ${name}…`);
    try {
      await call(context, "/group", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name,
          gid: groupGid.value.trim() ? Number(groupGid.value) : null,
          system: groupSystem.checked
        })
      });
      groupName.value = "";
      groupGid.value = "";
      groupSystem.checked = false;
      await load();
      setStatus(`Created group ${name}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  });
  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  let visible = true;
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible)
      return;
    visible = next;
    if (!next || disposed)
      return;
    load();
  });
  load();
  return () => {
    disposed = true;
    offVisibility?.();
    root.replaceChildren();
  };
}
export {
  mount
};
