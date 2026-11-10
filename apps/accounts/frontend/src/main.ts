import { call } from "./api";
import { cell, table } from "./format";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Accounts view: users and groups from the accounts service.

export function mount(root: HTMLElement, context: FrameContext) {
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

  const usersRoot = box.querySelector("[data-users]") as any;
  const groupsRoot = box.querySelector("[data-groups]") as any;
  const status = box.querySelector(".acc-status") as any;
  const form = box.querySelector(".acc-add") as any;
  const formName = box.querySelector("[data-name]") as any;
  const formUid = box.querySelector("[data-uid]") as any;
  const formGid = box.querySelector("[data-gid]") as any;
  const formHome = box.querySelector("[data-home]") as any;
  const formShell = box.querySelector("[data-shell]") as any;
  const formSystem = box.querySelector("[data-system]") as any;
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
        const protectedAccount =
          user.name === "root" || user.name === "operator" || user.system;
        remove.disabled = protectedAccount;
        remove.title = protectedAccount
          ? "System, root and operator accounts are protected"
          : `Delete ${user.name}`;
        remove.addEventListener("click", () => void deleteUser(user.name));
        actions.append(lock, remove);
        row.append(
          name,
          cell("acc-mono", String(user.uid)),
          cell("acc-mono", String(user.gid)),
          cell("acc-mono", user.home),
          cell("acc-mono", user.shell),
          actions,
        );
        return row;
      });
      usersRoot.replaceChildren(
        table(["USER", "UID", "GID", "HOME", "SHELL", "ACTIONS"], userRows),
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

  async function toggleLock(name, locked) {
    setStatus(`${locked ? "Locking" : "Unlocking"} ${name}…`);
    try {
      await call(context, "/lock", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name, locked }),
      });
      await load();
      setStatus(`${name} ${locked ? "locked" : "unlocked"}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function deleteUser(name) {
    setStatus(`Deleting ${name}…`);
    try {
      await call(context, `/user?name=${encodeURIComponent(name)}`, {
        method: "DELETE",
      });
      await load();
      setStatus(`Deleted ${name}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
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
          system: formSystem.checked,
        }),
      });
      formName.value = "";
      formUid.value = "";
      formGid.value = "";
      formHome.value = "";
      formSystem.checked = false;
      await load();
      setStatus(`Created ${name}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  });

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
