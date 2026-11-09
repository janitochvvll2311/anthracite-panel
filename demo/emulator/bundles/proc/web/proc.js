// The demo Processes view: a deterministic /proc snapshot.
//
// Every call goes through the frame's scoped fetch, so the module only ever
// reaches its own app prefix.

const styles = `
  .proc { display: flex; flex-direction: column; gap: 10px; min-height: 360px; }
  .proc-bar { display: flex; align-items: center; gap: 8px; }
  .proc-count { color: #667087; font-size: 12px; }
  .proc-bar .grow { flex: 1; }
  .proc-bar button, .proc-bar select {
    padding: 6px 10px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-bar button:hover { border-color: #3a4560; color: #fff; }
  .proc-table { border: 1px solid #202632; border-radius: 9px; overflow: auto; background: #0d1016; }
  .proc-row {
    display: grid; grid-template-columns: 64px 64px minmax(120px, 1.4fr) 90px 90px 74px 90px 74px;
    gap: 8px; align-items: center; padding: 7px 10px; border-bottom: 1px solid #171c26;
    font-size: 12px; min-width: 760px;
  }
  .proc-row:last-child { border-bottom: 0; }
  .proc-row.head {
    position: sticky; top: 0; z-index: 1; background: #11151e; color: #667087;
    text-transform: uppercase; letter-spacing: .06em; font-size: 10px;
  }
  .proc-row:hover:not(.head) { background: #151a24; }
  .proc-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #e8ebf2; }
  .proc-user, .proc-mono { color: #8f98ab; font-family: ui-monospace, monospace; font-size: 11px; }
  .proc-state { text-transform: capitalize; }
  .proc-state.running { color: #52d1a4; }
  .proc-state.sleeping { color: #8f98ab; }
  .proc-state.stopped { color: #f3c969; }
  .proc-state.zombie { color: #ff7d8a; }
  .proc-bar-track { height: 5px; border-radius: 999px; background: #202634; overflow: hidden; flex: 1; }
  .proc-bar-fill { height: 100%; background: linear-gradient(90deg, #7aa2ff, #9d7bff); }
  .proc-cpu { display: flex; align-items: center; gap: 6px; }
  .proc-cpu .percent { width: 38px; text-align: right; color: #8f98ab; font: 11px ui-monospace, monospace; }
  .proc-more { display: flex; justify-content: center; }
  .proc-more button {
    padding: 6px 12px; border-radius: 7px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 12px; cursor: pointer;
  }
  .proc-more button:hover { border-color: #3a4560; color: #fff; }
  .proc-signal {
    padding: 4px 7px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .proc-signal:hover { border-color: #ff7d8a88; color: #ffb3ba; }
  .proc-status { margin: 0; min-height: 16px; color: #667087; font-size: 12px; }
  .proc-status.bad { color: #ffb3ba; }
`;

async function call(context, route, init) {
  const { headers, ...rest } = init ?? {};
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    { ...rest, headers: { accept: "application/json", ...(headers ?? {}) } },
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

function memory(bytes) {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))} MiB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KiB`;
  return `${bytes} B`;
}

function cell(className, text) {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}

export function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "proc";
  box.innerHTML = `
    <div class="proc-bar">
      <span class="proc-count">…</span>
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
    <div class="proc-table" role="table" aria-label="Processes"></div>
    <div class="proc-more"><button type="button" data-more hidden>Load more</button></div>
    <p class="proc-status" role="status"></p>
  `;
  root.append(box);

  const table = box.querySelector(".proc-table");
  const count = box.querySelector(".proc-count");
  const status = box.querySelector(".proc-status");
  const signal = box.querySelector(".proc-pick");
  const more = box.querySelector("[data-more]");
  let nextCursor = null;
  let disposed = false;

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-status bad" : "proc-status";
  };

  async function load(cursor = null, append = false) {
    setStatus(append ? "Loading more processes…" : "Reading the process table…");
    try {
      const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
      const page = await call(context, `/processes?limit=200${suffix}`);
      if (disposed) return;
      if (!append) {
        table.replaceChildren();
        const head = document.createElement("div");
        head.className = "proc-row head";
        for (const label of ["PID", "PPID", "NAME", "USER", "STATE", "CPU", "MEMORY", ""]) {
          head.append(cell("", label));
        }
        table.append(head);
      }

      for (const process of page.processes) {
        const row = document.createElement("div");
        row.className = "proc-row";
        row.append(
          cell("proc-mono", String(process.pid)),
          cell("proc-mono", String(process.ppid)),
          cell("proc-name", process.name),
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
        cpu.append(
          track,
          cell("percent", `${process["cpu-percent"].toFixed(1)}%`),
        );
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

      nextCursor = page.cursor;
      more.hidden = Boolean(page.done) || !page.cursor;
      const shown = table.querySelectorAll(".proc-row:not(.head)").length;
      count.textContent = `${shown} process${shown === 1 ? "" : "es"}${page.done ? "" : " · more available"}`;
      setStatus(`Snapshot updated · ${shown} processes`);
    } catch (error) {
      if (!disposed) setStatus(String(error), true);
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
      if (!disposed) setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]").addEventListener("click", () => void load());
  more.addEventListener("click", () => {
    if (nextCursor) void load(nextCursor, true);
  });
  void load();

  return () => {
    disposed = true;
    root.replaceChildren();
  };
}
