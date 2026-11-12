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

  const policyRoot = box.querySelector("[data-policy]") as any;
  const usersRoot = box.querySelector("[data-users]") as any;
  const groupsRoot = box.querySelector("[data-groups]") as any;
  const passwordPanel = box.querySelector("[data-password]") as any;
  const status = box.querySelector(".acc-status") as any;
  const userForm = box.querySelector("[data-user-form]") as any;
  const formName = box.querySelector("[data-name]") as any;
  const formUid = box.querySelector("[data-uid]") as any;
  const formGid = box.querySelector("[data-gid]") as any;
  const formGecos = box.querySelector("[data-gecos]") as any;
  const formHome = box.querySelector("[data-home]") as any;
  const formShell = box.querySelector("[data-shell]") as any;
  const formSystem = box.querySelector("[data-system]") as any;
  const groupForm = box.querySelector("[data-group-form]") as any;
  const groupName = box.querySelector("[data-group-name]") as any;
  const groupGid = box.querySelector("[data-group-gid]") as any;
  const groupSystem = box.querySelector("[data-group-system]") as any;
  const usePanelForms =
    context.panel?.slots?.includes("panel.fab") === true &&
    context.panel?.slots?.includes("panel.modal") === true;
  if (usePanelForms) {
    userForm.remove();
    groupForm.remove();
  }
  let disposed = false;
  let policy: any = {};
  let shellInitialized = false;

  const labeled = (className: string, value: string, label: string) => {
    const node = cell(className, value);
    node.dataset.label = label;
    return node;
  };

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
    policyRoot.replaceChildren(
      policyBadge("control", policy.control),
      policyBadge("passwords", policy.passwords),
      policyBadge("system ids", policy["allow-system"]),
      policyBadge("privileged groups", policy["allow-privileged-groups"]),
      cell("acc-mono", `home ${policy["home-root"] ?? "/home"}`),
    );
    if (!usePanelForms) {
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
  }

  async function load() {
    setStatus("Reading accounts…");
    try {
      const [users, groups, settings] = await Promise.all([
        call(context, "/users"),
        call(context, "/groups"),
        call(context, "/policy"),
      ]);
      if (disposed) return;
      policy = settings;
      renderPolicy();

      const userRows = users.map((user) => {
        const row = document.createElement("div");
        row.className = "acc-row";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", user.name));
        if (user.system) name.append(cell("acc-badge system", "system"));
        if (user.source === "nss") name.append(cell("acc-badge nss", "directory"));
        if (user.locked === true) name.append(cell("acc-badge locked", "locked"));
        else if (user.locked === null) name.append(cell("acc-badge unknown", "lock ?"));
        name.title = `${user.gecos || user.name} · ${user.groups.length ? user.groups.join(", ") : "no supplementary groups"}`;

        const actions = document.createElement("span");
        actions.className = "acc-actions";
        const local = user.source === "local";
        const protectedAccount =
          user.name === "root" || user.name === "operator" || !local;
        const lock = document.createElement("button");
        lock.type = "button";
        lock.className = "acc-btn";
        lock.textContent = user.locked === false ? "Lock" : "Unlock";
        lock.disabled = !local || user.name === "root" || !policy.control || user.locked === null;
        lock.title = lock.disabled
          ? "this account cannot be locked from the panel"
          : `${lock.textContent} ${user.name}`;
        lock.addEventListener("click", () => void toggleLock(user));
        const password = document.createElement("button");
        password.type = "button";
        password.className = "acc-btn";
        password.textContent = "Password";
        password.disabled = protectedAccount || !policy.passwords;
        password.title = password.disabled
          ? "password changes are disabled for this account"
          : `Set a password for ${user.name}`;
        password.addEventListener("click", () => openPassword(user));
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "acc-btn danger";
        remove.textContent = "Delete";
        remove.disabled = protectedAccount || !policy.control;
        remove.title = remove.disabled
          ? "this account is protected"
          : `Delete ${user.name} and keep the home`;
        remove.addEventListener("click", () => void deleteUser(user, false));
        const removeHome = document.createElement("button");
        removeHome.type = "button";
        removeHome.className = "acc-btn danger";
        removeHome.textContent = "Delete home";
        removeHome.disabled = protectedAccount || !policy.control;
        removeHome.title = removeHome.disabled
          ? "this account is protected"
          : `Delete ${user.name} and remove ${user.home}`;
        removeHome.addEventListener("click", () => void deleteUser(user, true));
        actions.append(lock, password, remove, removeHome);

        row.append(
          name,
          labeled("acc-mono", String(user.uid), "UID"),
          labeled("acc-mono", String(user.gid), "GID"),
          labeled("acc-mono", user.group, "Group"),
          labeled(
            "acc-members",
            user.groups.length ? user.groups.join(", ") : "—",
            "Groups",
          ),
          labeled("acc-mono", user.home, "Home"),
          labeled("acc-mono", user.shell, "Shell"),
          actions,
        );
        row.dataset.source = user.source;
        return row;
      });
      usersRoot.replaceChildren(
        table(
          ["USER", "UID", "GID", "GROUP", "GROUPS", "HOME", "SHELL", "ACTIONS"],
          userRows,
        ),
      );

      const groupRows = groups.map((group) => {
        const row = document.createElement("div");
        row.className = "acc-row group";
        const name = document.createElement("span");
        name.className = "acc-name";
        name.append(cell("", group.name));
        if (group.system) name.append(cell("acc-badge system", "system"));
        if (group.source === "nss") name.append(cell("acc-badge nss", "directory"));
        const actions = document.createElement("span");
        actions.className = "acc-actions";
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "acc-btn danger";
        remove.textContent = "Delete";
        remove.disabled =
          group.source !== "local" || group.gid === 0 || !policy.control;
        remove.title = remove.disabled
          ? "this group cannot be deleted from the panel"
          : `Delete ${group.name}`;
        remove.addEventListener("click", () => void deleteGroup(group.name));
        actions.append(remove);
        row.append(
          name,
          labeled("acc-mono", String(group.gid), "GID"),
          labeled(
            "acc-members",
            group.members.length ? group.members.join(", ") : "—",
            "Members",
          ),
          actions,
        );
        return row;
      });
      groupsRoot.replaceChildren(
        table(["GROUP", "GID", "MEMBERS", "ACTIONS"], groupRows, "group"),
      );
      setStatus(`${users.length} users · ${groups.length} groups`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function toggleLock(user) {
    const locked = user.locked !== true;
    setStatus(`${locked ? "Locking" : "Unlocking"} ${user.name}…`);
    try {
      await call(context, "/lock", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name: user.name, locked }),
      });
      await load();
      setStatus(`${user.name} ${locked ? "locked" : "unlocked"}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function deleteUser(user, removeHome) {
    if (
      !window.confirm(
        `Delete ${user.name}${removeHome ? ` and remove ${user.home}` : ""}?`,
      )
    ) {
      return;
    }
    setStatus(`Deleting ${user.name}…`);
    try {
      await call(
        context,
        `/user?name=${encodeURIComponent(user.name)}&remove-home=${removeHome}`,
        { method: "DELETE" },
      );
      passwordPanel.replaceChildren();
      await load();
      setStatus(`Deleted ${user.name}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  async function deleteGroup(name) {
    if (!window.confirm(`Delete group ${name}?`)) return;
    setStatus(`Deleting group ${name}…`);
    try {
      await call(context, `/group?name=${encodeURIComponent(name)}`, {
        method: "DELETE",
      });
      await load();
      setStatus(`Deleted group ${name}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  }

  function openPassword(user) {
    if (usePanelForms) {
      void context.openModal?.({
        tag: "accounts-password-modal",
        params: { name: user.name },
      });
      return;
    }
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
      if (!password) return;
      setStatus(`Setting the password for ${user.name}…`);
      try {
        await call(context, "/password", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: user.name, password }),
        });
        form.remove();
        await load();
        setStatus(`Password set for ${user.name}`);
      } catch (error) {
        if (!disposed) setStatus(String(error), true);
      }
    });
    passwordPanel.replaceChildren(form);
    input.focus();
  }

  userForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = formName.value.trim();
    const home =
      formHome.value.trim() || `${policy["home-root"] ?? "/home"}/${name}`;
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
          system: formSystem.checked,
        }),
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
      if (!disposed) setStatus(String(error), true);
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
          system: groupSystem.checked,
        }),
      });
      groupName.value = "";
      groupGid.value = "";
      groupSystem.checked = false;
      await load();
      setStatus(`Created group ${name}`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
    }
  });

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") {
      void load();
      return;
    }
    if (tag === "new-user") {
      if (usePanelForms) void context.openModal?.({ tag: "accounts-user-modal" });
      else formName.focus();
      return;
    }
    if (tag === "new-group") {
      if (usePanelForms) void context.openModal?.({ tag: "accounts-group-modal" });
      else groupName.focus();
    }
  });
  let visible = true;
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible) return;
    visible = next;
    if (!next || disposed) return;
    // A CLI or another tab may have changed an account while this view was
    // hidden.
    void load();
  });
  void load();

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    root.replaceChildren();
  };
}
