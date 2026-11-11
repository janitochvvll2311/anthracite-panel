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
    display: grid; grid-template-columns: minmax(160px, 1.2fr) 70px 60px minmax(140px, 1fr) minmax(110px, 1fr) minmax(110px, auto);
    gap: 10px; padding: 7px 10px; border-bottom: 1px solid #171c26; font-size: 12px; align-items: center;
  }
  .acc-row:last-child { border-bottom: 0; }
  .acc-row.head { background: #11151e; color: #667087; text-transform: uppercase; letter-spacing: .06em; font-size: 10px; }
  .acc-row.group { grid-template-columns: minmax(140px, 1fr) 70px minmax(200px, 3fr); }
  .acc-name { color: #e8ebf2; display: flex; align-items: center; gap: 7px; }
  .acc-mono { font-family: ui-monospace, monospace; color: #8f98ab; font-size: 11px; }
  .acc-badge {
    padding: 1px 6px; border-radius: 999px; border: 1px solid #2a3142; font-size: 9.5px;
    text-transform: uppercase; letter-spacing: .07em; color: #8f98ab;
  }
  .acc-badge.system { border-color: #7aa2ff44; color: #9ab8ff; }
  .acc-badge.locked { border-color: #ff7d8a44; color: #ffb3ba; }
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
    <section class="acc-section">
      <div class="acc-title">Users</div>
      <div data-users></div>
    </section>
    <section class="acc-section">
      <div class="acc-title">Groups</div>
      <div data-groups></div>
    </section>
    <form class="acc-add">
      <input data-name placeholder="name" aria-label="User name" autocomplete="off" />
      <input data-uid type="number" min="1" placeholder="uid" aria-label="UID" />
      <input data-gid type="number" min="1" placeholder="gid" aria-label="GID" />
      <input data-home placeholder="/home/name" aria-label="Home" autocomplete="off" />
      <input data-shell value="/bin/sh" aria-label="Shell" autocomplete="off" />
      <label class="check"><input type="checkbox" data-system /> system</label>
      <button type="submit">Create user</button>
    </form>
    <p class="acc-status" role="status"></p>
  `;
  root.append(box);
  const usersRoot = box.querySelector("[data-users]");
  const groupsRoot = box.querySelector("[data-groups]");
  const status = box.querySelector(".acc-status");
  const form = box.querySelector(".acc-add");
  const formName = box.querySelector("[data-name]");
  const formUid = box.querySelector("[data-uid]");
  const formGid = box.querySelector("[data-gid]");
  const formHome = box.querySelector("[data-home]");
  const formShell = box.querySelector("[data-shell]");
  const formSystem = box.querySelector("[data-system]");
  let disposed = false;
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "acc-status bad" : "acc-status";
  };
  async function load() {
    setStatus("Reading accounts…");
    try {
      const [users, groups] = await Promise.all([
        call(context, "/users"),
        call(context, "/groups")
      ]);
      if (disposed)
        return;
      const userRows = users.map((user) => {
        const row = document.createElement("div");
        row.className = "acc-row";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", user.name));
        if (user.system)
          name.append(cell("acc-badge system", "system"));
        if (user.locked)
          name.append(cell("acc-badge locked", "locked"));
        const actions = document.createElement("span");
        actions.className = "acc-actions";
        const lock = document.createElement("button");
        lock.type = "button";
        lock.className = "acc-btn";
        lock.textContent = user.locked ? "Unlock" : "Lock";
        lock.disabled = user.name === "root";
        lock.title = user.name === "root" ? "root cannot be locked" : `${user.locked ? "Unlock" : "Lock"} ${user.name}`;
        lock.addEventListener("click", () => void toggleLock(user.name, !user.locked));
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "acc-btn danger";
        remove.textContent = "Delete";
        const protectedAccount = user.name === "root" || user.name === "operator" || user.system;
        remove.disabled = protectedAccount;
        remove.title = protectedAccount ? "System, root and operator accounts are protected" : `Delete ${user.name}`;
        remove.addEventListener("click", () => void deleteUser(user.name));
        actions.append(lock, remove);
        row.append(name, cell("acc-mono", String(user.uid)), cell("acc-mono", String(user.gid)), cell("acc-mono", user.home), cell("acc-mono", user.shell), actions);
        return row;
      });
      usersRoot.replaceChildren(table(["USER", "UID", "GID", "HOME", "SHELL", "ACTIONS"], userRows));
      const groupRows = groups.map((group) => {
        const row = document.createElement("div");
        row.className = "acc-row group";
        row.append(cell("acc-name", group.name), cell("acc-mono", String(group.gid)), cell("acc-members", group.members.length ? group.members.join(", ") : "—"));
        return row;
      });
      groupsRoot.replaceChildren(table(["GROUP", "GID", "MEMBERS"], groupRows, "group"));
      setStatus(`${users.length} users · ${groups.length} groups`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function toggleLock(name, locked) {
    setStatus(`${locked ? "Locking" : "Unlocking"} ${name}…`);
    try {
      await call(context, "/lock", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name, locked })
      });
      await load();
      setStatus(`${name} ${locked ? "locked" : "unlocked"}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  async function deleteUser(name) {
    setStatus(`Deleting ${name}…`);
    try {
      await call(context, `/user?name=${encodeURIComponent(name)}`, {
        method: "DELETE"
      });
      await load();
      setStatus(`Deleted ${name}`);
    } catch (error) {
      if (!disposed)
        setStatus(String(error), true);
    }
  }
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = formName.value.trim();
    const home = formHome.value.trim() || `/home/${name}`;
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
          uid: Number(formUid.value),
          gid: Number(formGid.value),
          home,
          shell: formShell.value.trim() || "/bin/sh",
          system: formSystem.checked
        })
      });
      formName.value = "";
      formUid.value = "";
      formGid.value = "";
      formHome.value = "";
      formSystem.checked = false;
      await load();
      setStatus(`Created ${name}`);
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
