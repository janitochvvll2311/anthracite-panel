import { call } from "./api";
import { confirmAction } from "./confirm";
import { cell, memory, reason } from "./format";
import { styles } from "./styles";
import {
  filterProcesses,
  orderProcesses,
  type Filters,
  type Process,
  type TreeRow,
} from "./tree";
import type { FrameContext } from "./types";

type SortKey = "pid" | "ppid" | "name" | "user" | "state" | "cpu" | "memory";

// The Processes view reads the real /proc snapshot. Desktop has a flat list and
// a tree with continuous guides; the layout is chosen by the component width.
// Phones keep the list only, with the filters in their own bar.
export function mount(root: HTMLElement, context: FrameContext) {
  const filled = typeof context.fill === "function";
  if (filled) {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = filled ? "proc fill" : "proc";
  box.innerHTML = `
    <div class="proc-bar">
      <span class="proc-count">…</span>
      <div class="proc-tabs" role="group" aria-label="Layout">
        <button type="button" data-mode="flat" aria-pressed="true">List</button>
        <button type="button" data-mode="tree" aria-pressed="false">Tree</button>
      </div>
      <label class="proc-interval"><span class="proc-interval-label">Interval</span>
        <select data-interval aria-label="Refresh interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button type="button" class="proc-fold" data-fold hidden>Collapse all</button>
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
    <div class="proc-mobile-tools">
      <input type="search" data-mobile-filter placeholder="Filter processes" aria-label="Filter processes" />
      <select data-mobile-sort aria-label="Sort by">
        <option value="pid">PID</option>
        <option value="ppid">PPID</option>
        <option value="name">Name</option>
        <option value="user">User</option>
        <option value="state">State</option>
        <option value="cpu">CPU</option>
        <option value="memory">Memory</option>
      </select>
      <button type="button" class="proc-direction" data-mobile-direction aria-label="Sort direction">Asc</button>
    </div>
    <div class="proc-table" role="table" aria-label="Processes">
      <div class="proc-head" data-head></div>
      <div class="proc-body" data-body role="rowgroup"></div>
      <div class="proc-sentinel" data-sentinel aria-hidden="true"></div>
    </div>
    <p class="proc-status" role="status"></p>
  `;
  root.append(box);

  const table = box.querySelector(".proc-table") as HTMLElement;
  const headRoot = box.querySelector("[data-head]") as HTMLElement;
  const bodyRoot = box.querySelector("[data-body]") as HTMLElement;
  const count = box.querySelector(".proc-count") as HTMLElement;
  const status = box.querySelector(".proc-status") as HTMLElement;
  const signal = box.querySelector(".proc-pick") as HTMLSelectElement | null;
  const mobileFilter = box.querySelector("[data-mobile-filter]") as HTMLInputElement;
  const mobileSort = box.querySelector("[data-mobile-sort]") as HTMLSelectElement;
  const mobileDirection = box.querySelector("[data-mobile-direction]") as HTMLButtonElement;
  const sentinel = box.querySelector("[data-sentinel]") as HTMLElement;  const fold = box.querySelector("[data-fold]") as HTMLButtonElement;
  const modeButtons = [...box.querySelectorAll("[data-mode]")] as HTMLButtonElement[];
  const interval = box.querySelector("[data-interval]") as HTMLSelectElement;

  const MOBILE_PAGE = 200;
  // The engine caps one service answer at 4 MiB. Ten thousand rows stay well
  // below it (~150 B per row); desktop and tree walk the cursor until `done`,
  // so the table itself is never truncated.
  const DESKTOP_PAGE = 10_000;
  const ROW_HEIGHT = 40;
  const OVERSCAN = 8;
  let currentRows: TreeRow[] = [];
  let windowStart = -1;
  let windowEnd = -1;
  let scrollFrame = 0;
  const COMPACT_WIDTH = 820;
  let processes: Process[] = [];
  let nextCursor: string | null = null;
  let sentinelVisible = false;
  let tree = false;
  let compact = false;
  let live: number | null = null;
  let intervalMs = 2000;
  let visible = true;
  let collapsed = new Set<number>();
  let allCollapsed = false;
  let sortKey: SortKey = "pid";
  let sortAsc = true;
  const filters: Filters = {};
  let loading = false;
  let pending = false;
  let disposed = false;

  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    scheduleLive();
    if (next && intervalMs > 0) void load();
  });

  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void load();
  });

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "proc-status bad" : "proc-status";
  };

  const stampStatus = (): void => {
    setStatus(
      `${intervalMs > 0 ? "Live" : "Updated"} · ${new Date().toLocaleTimeString()}`,
    );
  };

  const updateDirection = (): void => {
    mobileDirection.textContent = sortAsc ? "Asc" : "Desc";
    mobileDirection.setAttribute(
      "aria-label",
      sortAsc ? "Sort ascending" : "Sort descending",
    );
    mobileSort.value = sortKey;
  };

  const compareBy = (left: Process, right: Process): number => {
    let value = 0;
    switch (sortKey) {
      case "cpu":
        value = left["cpu-percent"] - right["cpu-percent"];
        break;
      case "memory":
        value = left["memory-bytes"] - right["memory-bytes"];
        break;
      case "name":
        value = left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
        break;
      case "user":
        value = left.user < right.user ? -1 : left.user > right.user ? 1 : 0;
        break;
      case "state":
        value = left.state < right.state ? -1 : left.state > right.state ? 1 : 0;
        break;
      case "ppid":
        value = left.ppid - right.ppid;
        break;
      default:
        value = left.pid - right.pid;
    }
    if (value === 0) value = left.pid - right.pid;
    return sortAsc ? value : -value;
  };

  const sortable = (key: SortKey | ""): key is SortKey => {
    if (!key) return false;
    if (!tree) return true;
    return key === "user" || key === "state" || key === "cpu" || key === "memory";
  };

  const syncModeButtons = (): void => {
    for (const button of modeButtons) {
      button.setAttribute(
        "aria-pressed",
        String(button.dataset.mode === (tree ? "tree" : "flat")),
      );
    }
  };

  const filterCell = (key: keyof Filters, placeholder: string): HTMLSpanElement => {
    const wrapper = cell("proc-filter-cell", "");
    const input = document.createElement("input");
    input.type = "search";
    input.placeholder = placeholder;
    input.value = filters[key] ?? "";
    input.setAttribute("aria-label", `Filter by ${placeholder.toLowerCase()}`);
    input.addEventListener("input", () => {
      filters[key] = input.value;
      renderRows(true);
    });
    wrapper.append(input);
    return wrapper;
  };

  function renderHead(): void {
    headRoot.replaceChildren();
    const head = document.createElement("div");
    head.className = `proc-row head ${tree ? "tree" : "flat"}`;
    const labels: Array<[string, SortKey | ""]> = tree
      ? [
          ["PROCESS", ""],
          ["USER", "user"],
          ["STATE", "state"],
          ["CPU", "cpu"],
          ["MEMORY", "memory"],
          ["", ""],
        ]
      : [
          ["PID", "pid"],
          ["PPID", "ppid"],
          ["NAME", "name"],
          ["USER", "user"],
          ["STATE", "state"],
          ["CPU", "cpu"],
          ["MEMORY", "memory"],
          ["", ""],
        ];
    for (const [label, key] of labels) {
      const node = cell("", label);
      if (sortable(key)) {
        node.classList.add("sortable");
        if (sortKey === key) {
          node.classList.add("sorted");
          node.textContent = `${label} ${sortAsc ? "▲" : "▼"}`;
        }
        node.title = `Sort by ${label.toLowerCase()}`;
        node.tabIndex = 0;
        node.dataset.sort = key;
        node.setAttribute("role", "button");
        node.setAttribute(
          "aria-label",
          `Sort by ${label.toLowerCase()}${
            sortKey === key ? (sortAsc ? ", ascending" : ", descending") : ""
          }`,
        );
        node.addEventListener("click", () => setSort(key));
        node.addEventListener("keydown", (event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setSort(key);
            headRoot
              .querySelector<HTMLElement>(`[data-sort="${key}"]`)
              ?.focus();
          }
        });
      }
      head.append(node);
    }
    headRoot.append(head);

    const row = document.createElement("div");
    row.className = `proc-row filters ${tree ? "tree" : "flat"}`;
    if (tree) {
      row.append(
        filterCell("query", "PID or name"),
        filterCell("user", "User"),
        filterCell("state", "State"),
        cell("proc-filter-cell", ""),
        cell("proc-filter-cell", ""),
        cell("proc-filter-cell", ""),
      );
    } else {
      row.append(
        filterCell("pid", "PID"),
        filterCell("ppid", "PPID"),
        filterCell("name", "Name"),
        filterCell("user", "User"),
        filterCell("state", "State"),
        cell("proc-filter-cell", ""),
        cell("proc-filter-cell", ""),
        cell("proc-filter-cell", ""),
      );
    }
    headRoot.append(row);
  }

  function setSort(key: SortKey): void {
    if (key === sortKey) sortAsc = !sortAsc;
    else {
      sortKey = key;
      sortAsc =
        key === "pid" ||
        key === "ppid" ||
        key === "name" ||
        key === "user" ||
        key === "state";
    }
    updateDirection();
    renderHead();
    scheduleLive();
    renderRows(true);
  }

  const metaCell = (className: string, label: string, text: string): HTMLSpanElement => {
    const node = cell(`proc-cell ${className}`, text);
    node.dataset.label = label;
    return node;
  };

  const cpuCell = (process: Process): HTMLSpanElement => {
    const cpu = document.createElement("span");
    cpu.className = "proc-cell cpu";
    cpu.dataset.label = "CPU";
    const track = document.createElement("span");
    track.className = "proc-bar-track";
    const fill = document.createElement("span");
    fill.className = "proc-bar-fill";
    fill.style.width = `${Math.min(100, Math.max(0, process["cpu-percent"]))}%`;
    track.append(fill);
    cpu.append(track, cell("percent", `${process["cpu-percent"].toFixed(1)}%`));
    cpu.title = `${process["cpu-percent"].toFixed(1)}% CPU`;
    return cpu;
  };

  const identityCell = (process: Process, row: TreeRow | null): HTMLSpanElement => {
    const identity = document.createElement("span");
    identity.className = "proc-cell identity";
    if (row) {
      const gutter = document.createElement("span");
      gutter.className = "proc-gutter";
      for (const line of row.guides) {
        const guide = document.createElement("span");
        guide.className = line ? "proc-guide" : "proc-guide blank";
        guide.setAttribute("aria-hidden", "true");
        gutter.append(guide);
      }
      if (row.depth > 0) {
        const connector = document.createElement("span");
        connector.className = row.isLast
          ? "proc-connector last"
          : "proc-connector";
        connector.setAttribute("aria-hidden", "true");
        gutter.append(connector);
      }
      identity.append(gutter);

      const twisty = document.createElement("button");
      twisty.type = "button";
      twisty.className = row.hasChildren ? "proc-twisty" : "proc-twisty empty";
      twisty.disabled = !row.hasChildren;
      twisty.tabIndex = row.hasChildren ? 0 : -1;
      twisty.textContent = collapsed.has(process.pid) ? "▸" : "▾";
      twisty.setAttribute(
        "aria-label",
        collapsed.has(process.pid) ? "Expand children" : "Collapse children",
      );
      twisty.setAttribute("aria-expanded", String(!collapsed.has(process.pid)));
      if (row.hasChildren) {
        twisty.addEventListener("click", () => {
          if (collapsed.has(process.pid)) collapsed.delete(process.pid);
          else collapsed.add(process.pid);
          renderRows();
        });
      }
      identity.append(twisty);
    }

    const pid = cell("proc-pid", String(process.pid));
    pid.dataset.label = "PID";
    identity.append(pid);
    if (!row) {
      const ppid = cell("proc-ppid", String(process.ppid));
      ppid.dataset.label = "PPID";
      identity.append(ppid);
    }
    const name = cell("proc-name", process.name);
    name.title = process.name;
    identity.append(name);
    return identity;
  };

  function renderRow(rowData: TreeRow): HTMLElement {
    const process = rowData.process;
    const row = document.createElement("div");
    row.className = `proc-row ${tree ? "tree" : "flat"}`;
    row.append(identityCell(process, tree ? rowData : null));

    const meta = document.createElement("span");
    meta.className = "proc-meta";
    const state = cell(`proc-cell state ${process.state}`, process.state);
    state.dataset.label = "State";
    meta.append(
      metaCell("user", "User", process.user),
      state,
      cpuCell(process),
      metaCell("memory", "Memory", memory(process["memory-bytes"])),
    );
    row.append(meta);

    const action = document.createElement("span");
    action.className = "proc-cell action";
    const send = document.createElement("button");
    send.type = "button";
    send.className = "proc-signal";
    send.textContent = "Send";
    send.setAttribute(
      "aria-label",
      `Send signal to ${process.name} (${process.pid})`,
    );
    send.addEventListener("click", () => {
      void signalProcess(process.pid, process["start-ticks"], process.name);
    });
    action.append(send);
    row.append(action);
    return row;
  }

  function emptyCell(): HTMLElement {
    return cell(
      "proc-empty",
      Object.values(filters).some((value) => value && value.trim())
        ? "No process matches the filters."
        : "No processes.",
    );
  }

  function renderWindow(force = false): void {
    if (compact) {
      const fragment = document.createDocumentFragment();
      for (const row of currentRows) fragment.append(renderRow(row));
      if (currentRows.length === 0) fragment.append(emptyCell());
      bodyRoot.replaceChildren(fragment);
      windowStart = -1;
      windowEnd = -1;
      return;
    }

    const headHeight = headRoot.offsetHeight;
    const viewport = Math.max(200, table.clientHeight - headHeight);
    const top = Math.max(0, table.scrollTop - headHeight);
    let start = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
    start = Math.min(start, currentRows.length);
    const end = Math.min(
      currentRows.length,
      Math.ceil((top + viewport) / ROW_HEIGHT) + OVERSCAN,
    );
    if (!force && start === windowStart && end === windowEnd) return;

    windowStart = start;
    windowEnd = end;
    const fragment = document.createDocumentFragment();
    if (start > 0) {
      const spacer = document.createElement("div");
      spacer.className = "proc-spacer";
      spacer.style.height = `${start * ROW_HEIGHT}px`;
      fragment.append(spacer);
    }
    for (let index = start; index < end; index += 1) {
      fragment.append(renderRow(currentRows[index]));
    }
    if (end < currentRows.length) {
      const spacer = document.createElement("div");
      spacer.className = "proc-spacer";
      spacer.style.height = `${(currentRows.length - end) * ROW_HEIGHT}px`;
      fragment.append(spacer);
    }
    if (currentRows.length === 0) fragment.append(emptyCell());
    bodyRoot.replaceChildren(fragment);
  }

  function renderRows(resetScroll = false): void {
    const visible = filterProcesses(processes, filters, tree);
    currentRows = orderProcesses(visible, tree, collapsed, compareBy);
    if (resetScroll) table.scrollTop = 0;
    renderWindow(true);

    const active = Object.values(filters).some((value) => value && value.trim());
    const countText = active
      ? `${currentRows.length} / ${processes.length}`
      : `${processes.length}`;
    count.textContent = `${countText} process${processes.length === 1 ? "" : "es"}${
      tree ? " · tree" : ""
    }`;
  }

  async function loadAll(): Promise<Process[]> {
    const all: Process[] = [];
    const seen = new Set<string>();
    let cursor: string | null = null;
    while (true) {
      const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
      const page = await call(context, `/processes?limit=${DESKTOP_PAGE}${suffix}`);
      all.push(...page.processes);
      if (disposed || page.done || !page.cursor || seen.has(page.cursor)) break;
      seen.add(page.cursor);
      cursor = page.cursor;
    }
    return all;
  }

  async function loadTree(): Promise<void> {
    const all = await loadAll();
    if (disposed) return;
    processes = all;
    nextCursor = null;
    fold.hidden = false;
    renderRows(true);
    stampStatus();
  }

  async function load(cursor: string | null = null, append = false): Promise<void> {
    if (loading) {
      pending = true;
      return;
    }
    loading = true;
    setStatus(append ? "Loading more processes…" : "Reading the process table…");
    try {
      if (tree) {
        await loadTree();
        return;
      }
      if (compact) {
        // Live refreshes every loaded page: asking for the first page again
        // would discard the rows loaded by scrolling to the end.
        const limit = append
          ? MOBILE_PAGE
          : Math.max(MOBILE_PAGE, processes.length);
        const suffix = cursor ? `&cursor=${encodeURIComponent(cursor)}` : "";
        const page = await call(
          context,
          `/processes?limit=${limit}${suffix}`,
        );
        if (disposed) return;
        if (append) processes.push(...page.processes);
        else processes = page.processes;
        nextCursor = page.cursor ?? null;
      } else {
        processes = await loadAll();
        if (disposed) return;
        nextCursor = null;
      }
      fold.hidden = true;
      renderRows(false);
      stampStatus();
    } catch (error) {
      if (!disposed) setStatus(reason(error), true);
    } finally {
      loading = false;
      if (pending) {
        pending = false;
        void load();
      }
    }
  }

  /// Phones load the next page when the end of the list scrolls into view; the
  /// desktop snapshot already has every row.
  function moreIfNeeded(): void {
    if (!compact || tree || loading || !nextCursor) return;
    void load(nextCursor, true);
  }

  async function signalProcess(
    pid: number,
    startTicks: number,
    name: string,
  ): Promise<void> {
    if (!signal) return;
    const chosen = signal.value;
    const dangerous = chosen === "TERM" || chosen === "KILL" || chosen === "STOP";
    const ok = await confirmAction({
      title: `Send ${chosen} to ${name} (${pid})?`,
      message: dangerous
        ? "The process may stop and unsaved work can be lost."
        : "The process will receive the signal.",
      confirmLabel: `Send ${chosen}`,
      danger: dangerous,
    });
    if (!ok) return;
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

  function scheduleLive(): void {
    if (live !== null) window.clearInterval(live);
    live = null;
    if (!visible || intervalMs === 0) return;
    live = window.setInterval(
      () => {
        if (!document.hidden) void load();
      },
      intervalMs,
    );
  }

  function setMode(next: boolean): void {
    if (next && compact) return;
    if (next) {
      filters.pid = "";
      filters.ppid = "";
      filters.name = "";
    } else {
      filters.query = "";
    }
    tree = next;
    if (!sortable(sortKey)) {
      sortKey = "pid";
      sortAsc = true;
    }
    syncModeButtons();
    fold.hidden = !tree;
    allCollapsed = false;
    fold.textContent = "Collapse all";
    renderHead();
    updateDirection();
    table.scrollTop = 0;
    scheduleLive();
    void load();
  }

  function applyCompact(): void {
    const next = box.clientWidth < COMPACT_WIDTH;
    if (next === compact) return;
    compact = next;
    box.classList.toggle("compact", compact);
    if (compact) {
      filters.pid = "";
      filters.ppid = "";
      filters.name = "";
      filters.user = "";
      filters.state = "";
      mobileFilter.value = filters.query ?? "";
      processes = [];
      nextCursor = null;
      if (tree) {
        tree = false;
        syncModeButtons();
        fold.hidden = true;
      }
    } else {
      filters.query = "";
      mobileFilter.value = "";
    }
    renderHead();
    table.scrollTop = 0;
    scheduleLive();
    void load();
  }

  for (const button of modeButtons) {
    button.addEventListener("click", () => setMode(button.dataset.mode === "tree"));
  }
  fold.addEventListener("click", () => {
    allCollapsed = !allCollapsed;
    collapsed = new Set();
    if (allCollapsed) {
      const parents = new Set(processes.map((process) => process.ppid));
      for (const process of processes) {
        if (parents.has(process.pid)) collapsed.add(process.pid);
      }
    }
    fold.textContent = allCollapsed ? "Expand all" : "Collapse all";
    renderRows(true);
  });
  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    scheduleLive();
    void load();
  });
  mobileFilter.addEventListener("input", () => {
    filters.query = mobileFilter.value;
    renderRows(true);
  });
  mobileSort.addEventListener("change", () => {
    setSort(mobileSort.value as SortKey);
  });
  mobileDirection.addEventListener("click", () => {
    sortAsc = !sortAsc;
    updateDirection();
    renderHead();
    renderRows(true);
  });
  box.querySelector("[data-refresh]")!.addEventListener("click", () => void load());
  const resize = new ResizeObserver(applyCompact);
  resize.observe(box);
  const tableResize = new ResizeObserver(() => renderWindow(false));
  tableResize.observe(table);
  const sentinelObserver = new IntersectionObserver(
    ([entry]) => {
      sentinelVisible = entry.isIntersecting;
      if (sentinelVisible) moreIfNeeded();
    },
    { rootMargin: "160px" },
  );
  sentinelObserver.observe(sentinel);
  applyCompact();
  renderHead();
  updateDirection();
  scheduleLive();
  table.addEventListener("scroll", () => {
    if (compact || scrollFrame) return;
    scrollFrame = window.requestAnimationFrame(() => {
      scrollFrame = 0;
      renderWindow(false);
    });
  });
  void load();

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    if (live !== null) window.clearInterval(live);
    if (scrollFrame) window.cancelAnimationFrame(scrollFrame);
    resize.disconnect();
    tableResize.disconnect();
    sentinelObserver.disconnect();
    root.replaceChildren();
  };
}
