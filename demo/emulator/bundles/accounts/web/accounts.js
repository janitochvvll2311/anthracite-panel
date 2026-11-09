// The demo Accounts view: users and groups from the accounts service.

const styles = `
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
    display: grid; grid-template-columns: minmax(110px, 1fr) 70px 60px minmax(120px, 1fr) minmax(110px, 1fr) 70px;
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
  .acc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .acc-status.bad { color: #ffb3ba; }
`;

async function call(context, route) {
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    { headers: { accept: "application/json" } },
  );
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message =
      value && typeof value === "object" && value.error
        ? `${value.error.code}: ${value.error.message}`
        : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}

function table(headers, entries, rowClass) {
  const node = document.createElement("div");
  node.className = "acc-table";
  const head = document.createElement("div");
  head.className = `acc-row head${rowClass ? ` ${rowClass}` : ""}`;
  for (const label of headers) head.append(cell("", label));
  node.append(head);
  for (const entry of entries) node.append(entry);
  return node;
}

export function mount(root, context) {
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
    <p class="acc-status" role="status"></p>
  `;
  root.append(box);

  const usersRoot = box.querySelector("[data-users]");
  const groupsRoot = box.querySelector("[data-groups]");
  const status = box.querySelector(".acc-status");
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
        call(context, "/groups"),
      ]);
      if (disposed) return;

      const userRows = users.map((user) => {
        const row = document.createElement("div");
        row.className = "acc-row";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", user.name));
        if (user.system) name.append(cell("acc-badge system", "system"));
        if (user.locked) name.append(cell("acc-badge locked", "locked"));
        row.append(
          name,
          cell("acc-mono", String(user.uid)),
          cell("acc-mono", String(user.gid)),
          cell("acc-mono", user.home),
          cell("acc-mono", user.shell),
          cell("acc-mono", user.system ? "system" : "user"),
        );
        return row;
      });
      usersRoot.replaceChildren(
        table(["USER", "UID", "GID", "HOME", "SHELL", "KIND"], userRows),
      );

      const groupRows = groups.map((group) => {
        const row = document.createElement("div");
        row.className = "acc-row group";
        row.append(
          cell("acc-name", group.name),
          cell("acc-mono", String(group.gid)),
          cell("acc-members", group.members.length ? group.members.join(", ") : "—"),
        );
        return row;
      });
      groupsRoot.replaceChildren(
        table(["GROUP", "GID", "MEMBERS"], groupRows, "group"),
      );
      setStatus(`${users.length} users · ${groups.length} groups`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
