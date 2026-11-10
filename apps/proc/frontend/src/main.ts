import { call } from "./api";
import { cell, memory, reason } from "./format";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Processes view: a deterministic /proc snapshot with a tree.
//
// Every call goes through the frame's scoped fetch, so the module only ever
// reaches its own app prefix.

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "proc";
  box.innerHTML = `
    <div class="proc-bar">
      <span class="proc-count">…</span>
      <label class="proc-check"><input type="checkbox" data-tree /> Tree</label>
      <label class="proc-check"><input type="checkbox" data-live /> Live</label>
      <span class="grow"></span>
      <select class="proc-pick" aria-label="Signal">
        <option value="TERM">TERM</option>
        <option value="INT">INT</option>
        <option value="HUP">HUP</option>
        <option value="STOP">STOP</option>
        <option value="CONT">CONT</option>
        <option value="KILL">KILL</option>
      </select>
      <button type="button" data-refresh>Refresh</button>
    </div>
    <form class="proc-spawn">
      <input name="command" aria-label="Detached command" placeholder="sleep 30" autocomplete="off" />
      <button type="submit">Spawn</button>
    </form>
    <div class="proc-table" role="table" aria-label="Processes"></div>
    <div class="proc-more"><button type="button" data-more hidden>Load more</button></div>
    <p class="proc-status" role="status"></p>
  `;
  root.append(box);

  const table = box.querySelector(".proc-table") as any;
  const count = box.querySelector(".proc-count") as any;
  const status = box.querySelector(".proc-status") as any;
  const signal = box.querySelector(".proc-pick") as any;
  const more = box.querySelector("[data-more]") as any;
  const treeToggle = box.querySelector("[data-tree]") as any;
  const liveToggle = box.querySelector("[data-live]") as any;
  const spawnForm = box.querySelector(".proc-spawn") as any;
  const spawnInput = spawnForm.querySelector("input") as any;
  let processes = [];
  let nextCursor = null;
  let tree = false;
  let live = null;
  let disposed = false;

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-status bad" : "proc-status";
  };

  function ordered() {
    if (!tree) return processes.map((process) => [process, 0]);
    const byPid = new Map(processes.map((process) => [process.pid, process]));
    const children = new Map();
    const roots = [];
    for (const process of processes) {
      const parent = byPid.get(process.ppid);
      if (parent && process.ppid !== process.pid) {
        children.set(process.ppid, [...(children.get(process.ppid) ?? []), process]);
      } else {
        roots.push(process);
      }
    }
    const out = [];
    const walk = (process, depth) => {
      out.push([process, depth]);
      const nested = (children.get(process.pid) ?? []).sort((a, b) => a.pid - b.pid);
      for (const child of nested) walk(child, depth + 1);
    };
    for (const process of roots.sort((a, b) => a.pid - b.pid)) walk(process, 0);
    return out;
  }

  function renderRows() {
    table.replaceChildren();
    const head = document.createElement("div");
    head.className = "proc-row head";
    for (const label of ["PID", "PPID", "NAME", "USER", "STATE", "CPU", "MEMORY", ""]) {
      head.append(cell("", label));
    }
    table.append(head);
    for (const [process, depth] of ordered()) {
      const row = document.createElement("div");
      row.className = "proc-row";
      row.append(
        cell("proc-mono", String(process.pid)),
        cell("proc-mono", String(process.ppid)),
      );
      const name = cell(
        "proc-name",
        `${"> ".repeat(depth)}${process.name}`,
      );
      name.title = process.name;
      row.append(
        name,
        cell("proc-user", process.user),
        cell(`proc-state ${process.state}`, process.state),
      );

      const cpu = document.createElement("span");
      cpu.className = "proc-cpu";
      const track = document.createElement("span");
      track.className = "proc-bar-track";
      const fill = document.createElement("span");
      fill.className = "proc-bar-fill";
      fill.style.width = `${Math.min(100, Math.max(0, process["cpu-percent"]))}%`;
      track.append(fill);
      cpu.append(track, cell("percent", `${process["cpu-percent"].toFixed(1)}%`));
      cpu.title = `${process["cpu-percent"].toFixed(1)}% CPU`;
      row.append(cpu);

      row.append(cell("proc-mono", memory(process["memory-bytes"])));

      const send = document.createElement("button");
      send.type = "button";
      send.className = "proc-signal";
      send.textContent = "Send";
      send.title = `Send ${signal.value} to ${process.name} (${process.pid})`;
      send.addEventListener("click", () => {
        void signalProcess(process.pid, process["start-ticks"]);
      });
      const action = document.createElement("span");
      action.append(send);
      row.append(action);
      table.append(row);
    }
    count.textContent = `${processes.length} process${processes.length === 1 ? "" : "es"}${nextCursor ? " · more available" : ""}`;
  }

  async function load(cursor = null, append = false) {
    setStatus(append ? "Loading more processes…" : "Reading the process table…");
    try {
      const limit = tree ? 500 : 200;
      const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
      const page = await call(context, `/processes?limit=${limit}${suffix}`);
      if (disposed) return;
      if (!append) processes = [];
      processes.push(...page.processes);
      nextCursor = tree ? null : page.cursor;
      more.hidden = tree || Boolean(page.done) || !page.cursor;
      renderRows();
      setStatus(`Snapshot updated · ${processes.length} processes`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  async function signalProcess(pid, startTicks) {
    const chosen = signal.value;
    setStatus(`Sending ${chosen} to ${pid}…`);
    try {
      await call(context, "/signal", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ pid, "start-ticks": startTicks, signal: chosen }),
      });
      setStatus(`${chosen} sent to ${pid}`);
      await load();
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  }

  spawnForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const command = spawnInput.value.trim();
    if (!command) {
      setStatus("Write a command first, for example `sleep 30`.", true);
      return;
    }
    const argv = command.split(/\s+/);
    setStatus("Spawning…");
    try {
      const pid = await call(context, "/spawn", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ argv }),
      });
      spawnInput.value = "";
      await load();
      setStatus(`Spawned ${argv[0]} as pid ${pid}`);
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    }
  });

  treeToggle.addEventListener("change", () => {
    tree = treeToggle.checked;
    void load();
  });
  liveToggle.addEventListener("change", () => {
    if (live) {
      clearInterval(live);
      live = null;
    }
    if (liveToggle.checked) {
      live = setInterval(() => {
        if (!document.hidden) void load();
      }, 2000);
    }
  });
  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  more.addEventListener("click", () => {
    if (nextCursor && !tree) void load(nextCursor, true);
  });
  void load();

  return () => {
    disposed = true;
    if (live) clearInterval(live);
    root.replaceChildren();
  };
}
