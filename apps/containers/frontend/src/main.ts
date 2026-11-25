import { call, watchExec, watchLogs, watchStats } from "./api";
import { formatBytes, matches, percent, shortId, stateKind } from "./state";
import { styles } from "./styles";
import type {
  Container,
  FrameContext,
  Image,
  Network,
  Sample,
  Volume,
} from "./types";

type Tab = "containers" | "images" | "volumes" | "networks";

function date(seconds: number): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

function fillStrip(strip: HTMLElement, sample: Sample): void {
  const cpu = strip.querySelector("[data-cpu]");
  const memory = strip.querySelector("[data-memory]");
  const network = strip.querySelector("[data-network]");
  const pids = strip.querySelector("[data-pids]");
  if (cpu) cpu.textContent = percent(sample["cpu-percent"] ?? 0);
  if (memory)
    memory.textContent = `${formatBytes(sample["memory-bytes"] ?? 0)} / ${formatBytes(sample["memory-limit"] ?? 0)}`;
  if (network)
    network.textContent = `↓ ${formatBytes(sample["net-rx"] ?? 0)} ↑ ${formatBytes(sample["net-tx"] ?? 0)}`;
  if (pids) pids.textContent = String(sample.pids ?? 0);
}

function escape(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "cn";
  box.dataset.testid = "containers-view";
  box.innerHTML = `
    <div class="cn-bar">
      <span class="cn-title">Containers</span>
      <span class="cn-badge" data-counts></span>
      <span class="grow"></span>
      <button type="button" data-create data-testid="containers-create">Create</button>
      <button type="button" data-refresh data-testid="containers-refresh">Refresh</button>
    </div>
    <p class="cn-status" role="status" data-status data-testid="containers-status"></p>
    <div class="cn-tabs" role="tablist">
      <button type="button" role="tab" class="cn-tab active" data-tab="containers" data-testid="containers-tab-containers">Containers</button>
      <button type="button" role="tab" class="cn-tab" data-tab="images" data-testid="containers-tab-images">Images</button>
      <button type="button" role="tab" class="cn-tab" data-tab="volumes" data-testid="containers-tab-volumes">Volumes</button>
      <button type="button" role="tab" class="cn-tab" data-tab="networks" data-testid="containers-tab-networks">Networks</button>
    </div>
    <div class="cn-grid" data-containers data-testid="containers-list"></div>
    <section class="cn-detail" data-detail data-testid="containers-detail" hidden></section>
    <section data-panel-images hidden>
      <div class="cn-bar">
        <input type="text" data-pull-reference placeholder="alpine:3.21" data-testid="containers-pull-reference" />
        <button type="button" data-pull data-testid="containers-pull">Pull image</button>
      </div>
      <div class="cn-grid" data-images></div>
    </section>
    <section data-panel-volumes hidden>
      <div class="cn-bar">
        <input type="text" data-volume-name placeholder="data" data-testid="containers-volume-name" />
        <button type="button" data-volume-create data-testid="containers-volume-create">Create volume</button>
      </div>
      <div class="cn-grid" data-volumes></div>
    </section>
    <section data-panel-networks hidden>
      <div class="cn-grid" data-networks></div>
    </section>
    <dialog class="cn-dialog" data-confirm>
      <h3 data-confirm-title>Confirm</h3>
      <p class="cn-note" data-confirm-message></p>
      <label data-force hidden><span><input type="checkbox" data-confirm-force /> force a running container</span></label>
      <div class="cn-actions">
        <button type="button" data-confirm-cancel data-testid="containers-confirm-cancel">Cancel</button>
        <button type="button" class="danger" data-confirm-run data-testid="containers-confirm-run">Confirm</button>
      </div>
    </dialog>
    <dialog class="cn-dialog" data-exec-dialog>
      <h3 data-exec-title>Exec</h3>
      <pre class="cn-exec-out" data-exec-out data-testid="containers-exec-out"></pre>
      <div class="cn-exec-line">
        <input type="text" data-exec-input data-testid="containers-exec-input" placeholder="command and Enter" />
        <button type="button" data-exec-send data-testid="containers-exec-send">Send</button>
      </div>
      <div class="cn-actions">
        <button type="button" data-exec-close data-testid="containers-exec-close">Close</button>
      </div>
    </dialog>
  `;
  root.append(box);

  const status = box.querySelector("[data-status]") as HTMLElement;
  const counts = box.querySelector("[data-counts]") as HTMLElement;
  const list = box.querySelector("[data-containers]") as HTMLElement;
  const detail = box.querySelector("[data-detail]") as HTMLElement;
  const imagesNode = box.querySelector("[data-images]") as HTMLElement;
  const volumesNode = box.querySelector("[data-volumes]") as HTMLElement;
  const networksNode = box.querySelector("[data-networks]") as HTMLElement;
  const confirmDialog = box.querySelector("[data-confirm]") as HTMLDialogElement;
  const execDialog = box.querySelector("[data-exec-dialog]") as HTMLDialogElement;
  const execOut = box.querySelector("[data-exec-out]") as HTMLElement;
  const execInput = box.querySelector("[data-exec-input]") as HTMLInputElement;
  const pullReference = box.querySelector(
    "[data-pull-reference]",
  ) as HTMLInputElement;
  const volumeName = box.querySelector("[data-volume-name]") as HTMLInputElement;
  const createButton = box.querySelector("[data-create]") as HTMLButtonElement;
  const hasFab = context.panel?.slots?.includes("panel.fab") === true;
  const hasModal = context.panel?.slots?.includes("panel.modal") === true;
  if (hasFab || !hasModal) createButton.hidden = true;

  let containers: Container[] = [];
  let images: Image[] = [];
  let volumes: Volume[] = [];
  let networks: Network[] = [];
  let selected: string | null = null;
  let tab: Tab = "containers";
  let logs: AbortController | null = null;
  let stats: AbortController | null = null;
  let exec: AbortController | null = null;
  let execSession: string | null = null;
  let visible = true;
  let disposed = false;
  let pending: (() => Promise<void>) | null = null;
  let logBuffer = "";
  let statSample: Sample | null = null;

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "cn-status bad" : "cn-status";
  };

  const badge = (kind: string, text: string): string =>
    `<span class="cn-badge ${kind}">${escape(text)}</span>`;

  function renderCounts(): void {
    const running = containers.filter((entry) => entry.state === "running").length;
    counts.textContent = `${containers.length} containers · ${running} running`;
  }

  function renderContainers(): void {
    list.replaceChildren();
    if (containers.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent =
        "No container in this runtime. Use the FAB or the CLI to create one.";
      list.append(empty);
      return;
    }
    for (const container of containers) {
      const card = document.createElement("div");
      card.className = `cn-card${container.id === selected ? " selected" : ""}`;
      card.dataset.testid = "container";
      card.dataset.containerId = container.id;
      card.dataset.state = container.state;
      card.innerHTML = `
        <div class="cn-card-head">
          <strong title="${escape(container.name)}">${escape(container.name)}</strong>
          <span class="grow"></span>
          ${container.health ? badge(container.health === "healthy" ? "ok" : "warn", container.health) : ""}
          <span class="cn-badge ${stateKind(container.state)}" data-testid="container-state">${escape(container.state)}</span>
        </div>
        <div class="cn-field"><span class="cn-field-name">Image</span><span class="cn-mono" title="${escape(container.image)}">${escape(container.image)}</span></div>
        <div class="cn-field"><span class="cn-field-name">Status</span><span>${escape(container.status || "—")}</span></div>
        <div class="cn-field"><span class="cn-field-name">Ports</span><span class="cn-mono">${escape(container.ports.join(", ") || "—")}</span></div>
      `;
      card.addEventListener("click", () => select(container.id));
      list.append(card);
    }
  }

  function renderDetail(): void {
    const container = containers.find((entry) => entry.id === selected);
    if (!container) {
      detail.hidden = true;
      detail.replaceChildren();
      return;
    }
    detail.hidden = false;
    detail.replaceChildren();
    const head = document.createElement("div");
    head.className = "cn-detail-head";
    head.innerHTML = `
      <strong>${escape(container.name)}</strong>
      ${badge(stateKind(container.state), container.state)}
      <span class="cn-mono">${escape(shortId(container.id))}</span>
    `;
    const facts = document.createElement("div");
    facts.className = "cn-facts";
    const fact = (name: string, value: string): string =>
      `<div class="cn-field"><span class="cn-field-name">${escape(name)}</span><span>${escape(value)}</span></div>`;
    facts.innerHTML = [
      fact("Image", container.image),
      fact("Created", date(container["created-at"])),
      fact("Ports", container.ports.join(", ") || "—"),
      fact("Labels", container.labels.map((label) => `${label.name}=${label.value}`).join(", ") || "—"),
    ].join("");
    const actions = document.createElement("div");
    actions.className = "cn-actions";
    const action = (label: string, testid: string, run: () => void, kind = "") => {
      const button = document.createElement("button");
      button.type = "button";
      if (kind) button.className = kind;
      button.textContent = label;
      button.dataset.testid = testid;
      button.addEventListener("click", run);
      actions.append(button);
    };
    if (container.state === "running") {
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
      action("Restart", "container-restart", () => void lifecycle(container, "restart"));
      action("Pause", "container-pause", () => void lifecycle(container, "pause"));
    } else if (container.state === "paused") {
      action("Unpause", "container-unpause", () => void lifecycle(container, "unpause"));
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
    } else if (container.state === "restarting") {
      action("Stop", "container-stop", () => void lifecycle(container, "stop"));
    } else {
      action("Start", "container-start", () => void lifecycle(container, "start"));
    }
    action("Logs", "container-logs", () => void toggleLogs(container));
    action("Exec", "container-exec", () => void openExec(container));
    action("Remove", "container-remove", () => openRemove(container), "danger");
    action("Close", "container-close", () => {
      selected = null;
      stopDetailStreams();
      render();
    });
    const strip = document.createElement("div");
    strip.className = "cn-strip";
    strip.dataset.testid = "containers-stats";
    strip.innerHTML = `<span>CPU <strong data-cpu>—</strong></span><span>Memory <strong data-memory>—</strong></span><span>Network <strong data-network>—</strong></span><span>PIDs <strong data-pids>—</strong></span>`;
    if (statSample) fillStrip(strip, statSample);
    const log = document.createElement("pre");
    log.className = "cn-log";
    log.dataset.testid = "containers-log";
    log.textContent = logBuffer || "Select Logs to follow this container.\n";
    detail.append(head, facts, actions, strip, log);
  }

  function renderImages(): void {
    imagesNode.replaceChildren();
    if (images.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No image in this runtime.";
      imagesNode.append(empty);
      return;
    }
    for (const image of images) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "image";
      card.dataset.imageId = image.id;
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(image.tags[0] ?? shortId(image.id))}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Id</span><span class="cn-mono">${escape(shortId(image.id))}</span></div>
        <div class="cn-field"><span class="cn-field-name">Size</span><span>${escape(formatBytes(image.size))}</span></div>
        <div class="cn-field"><span class="cn-field-name">Digest</span><span class="cn-mono">${escape(image.digest ?? "—")}</span></div>
      `;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Remove";
      remove.dataset.testid = "image-remove";
      remove.addEventListener("click", () =>
        openConfirm(`Remove image ${image.tags[0] ?? shortId(image.id)}?`, "Remove", false, async () => {
          await call(context, "/images/remove", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ id: image.id, force: false }),
          });
          await loadImages();
        }),
      );
      card.append(remove);
      imagesNode.append(card);
    }
  }

  function renderVolumes(): void {
    volumesNode.replaceChildren();
    if (volumes.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No named volume.";
      volumesNode.append(empty);
      return;
    }
    for (const volume of volumes) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "volume";
      card.dataset.volumeName = volume.name;
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(volume.name)}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Driver</span><span>${escape(volume.driver)}</span></div>
      `;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger";
      remove.textContent = "Remove";
      remove.dataset.testid = "volume-remove";
      remove.addEventListener("click", () =>
        openConfirm(`Remove volume ${volume.name}?`, "Remove", false, async () => {
          await call(context, "/volumes/remove", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ name: volume.name, force: false }),
          });
          await loadVolumes();
        }),
      );
      card.append(remove);
      volumesNode.append(card);
    }
  }

  function renderNetworks(): void {
    networksNode.replaceChildren();
    if (networks.length === 0) {
      const empty = document.createElement("div");
      empty.className = "cn-empty";
      empty.textContent = "No network in this runtime.";
      networksNode.append(empty);
      return;
    }
    for (const network of networks) {
      const card = document.createElement("div");
      card.className = "cn-card";
      card.dataset.testid = "network";
      card.innerHTML = `
        <div class="cn-card-head"><strong>${escape(network.name)}</strong></div>
        <div class="cn-field"><span class="cn-field-name">Driver</span><span>${escape(network.driver)}</span></div>
        <div class="cn-field"><span class="cn-field-name">Subnet</span><span class="cn-mono">${escape(network.subnet ?? "—")}</span></div>
        <div class="cn-field"><span class="cn-field-name">Containers</span><span>${network.containers}</span></div>
      `;
      networksNode.append(card);
    }
  }

  function render(): void {
    renderCounts();
    renderContainers();
    renderDetail();
    renderImages();
    renderVolumes();
    renderNetworks();
    for (const button of box.querySelectorAll<HTMLButtonElement>(".cn-tab")) {
      const current = button.dataset.tab === tab;
      button.classList.toggle("active", current);
      button.setAttribute("aria-selected", String(current));
    }
  }

  async function loadContainers(): Promise<void> {
    try {
      containers = (await call(context, "/containers")) ?? [];
      if (selected && !containers.some((entry) => entry.id === selected)) {
        selected = null;
        stopDetailStreams();
      }
      setStatus("");
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function loadImages(): Promise<void> {
    try {
      images = (await call(context, "/images")) ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function loadVolumes(): Promise<void> {
    try {
      volumes = (await call(context, "/volumes")) ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function loadNetworks(): Promise<void> {
    try {
      networks = (await call(context, "/networks")) ?? [];
      render();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  function switchTab(next: Tab): void {
    tab = next;
    (box.querySelector("[data-containers]") as HTMLElement).hidden =
      next !== "containers";
    (detail as HTMLElement).hidden = next !== "containers" || !selected;
    for (const name of ["images", "volumes", "networks"] as const) {
      (box.querySelector(`[data-panel-${name}]`) as HTMLElement).hidden =
        next !== name;
    }
    if (next === "images") void loadImages();
    else if (next === "volumes") void loadVolumes();
    else if (next === "networks") void loadNetworks();
    render();
  }

  function select(id: string): void {
    if (selected === id) return;
    selected = id;
    stopDetailStreams();
    logBuffer = "";
    statSample = null;
    render();
    startStats();
  }

  function stopDetailStreams(): void {
    logs?.abort();
    logs = null;
    stats?.abort();
    stats = null;
    closeExec();
  }

  function startStats(): void {
    if (!selected) return;
    stats?.abort();
    const container = containers.find((entry) => entry.id === selected);
    if (!container) return;
    const strip = detail.querySelector(".cn-strip");
    if (!strip) return;
    const controller = new AbortController();
    stats = controller;
    void watchStats(context, container.id, controller.signal, (sample: Sample) => {
      statSample = sample;
      const current = detail.querySelector(".cn-strip");
      if (current) fillStrip(current as HTMLElement, sample);
    }).catch(() => {
      if (!controller.signal.aborted) setStatus("Stats stopped; the runtime ended the stream.", true);
    });
  }

  async function toggleLogs(container: Container): Promise<void> {
    const log = detail.querySelector(".cn-log") as HTMLElement | null;
    if (!log) return;
    if (logs) {
      logs.abort();
      logs = null;
      setStatus("Logs stopped.");
      return;
    }
    log.textContent = "";
    const controller = new AbortController();
    logs = controller;
    try {
      await watchLogs(context, container.id, controller.signal, (text) => {
        logBuffer += text;
        if (logBuffer.length > 200_000) logBuffer = logBuffer.slice(-150_000);
        const current = detail.querySelector(".cn-log") as HTMLElement | null;
        if (current) {
          current.textContent = logBuffer;
          current.scrollTop = current.scrollHeight;
        }
      });
    } catch (error) {
      if (!controller.signal.aborted) setStatus(String(error), true);
    } finally {
      if (logs === controller) logs = null;
    }
  }

  async function lifecycle(container: Container, action: string): Promise<void> {
    try {
      setStatus(`${action} ${container.name}…`);
      await call(context, `/container/${action}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: container.id, "timeout-s": 10 }),
      });
      setStatus(`${container.name}: ${action} requested.`);
      await loadContainers();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  function openRemove(container: Container): void {
    openConfirm(
      `Remove ${container.name}? Named volumes are kept.`,
      "Remove",
      true,
      async () => {
        const force = (
          confirmDialog.querySelector("[data-confirm-force]") as HTMLInputElement
        ).checked;
        await call(context, "/container/remove", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id: container.id, force }),
        });
        selected = null;
        stopDetailStreams();
        await loadContainers();
      },
    );
  }

  function openConfirm(
    message: string,
    label: string,
    force: boolean,
    run: () => Promise<void>,
  ): void {
    pending = run;
    (confirmDialog.querySelector("[data-confirm-title]") as HTMLElement).textContent =
      label;
    (confirmDialog.querySelector("[data-confirm-message]") as HTMLElement).textContent =
      message;
    (
      confirmDialog.querySelector("[data-confirm-run]") as HTMLButtonElement
    ).textContent = label;
    const forceLabel = confirmDialog.querySelector("[data-force]") as HTMLElement;
    forceLabel.hidden = !force;
    const checkbox = confirmDialog.querySelector(
      "[data-confirm-force]",
    ) as HTMLInputElement;
    checkbox.checked = false;
    confirmDialog.showModal();
  }

  async function openExec(container: Container): Promise<void> {
    try {
      const session = await call(context, "/container/exec", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          id: container.id,
          command: ["/bin/sh"],
          tty: true,
          cols: 100,
          rows: 30,
        }),
      });
      execSession = String(session);
      execOut.textContent = "";
      execInput.value = "";
      execDialog.showModal();
      execInput.focus();
      const controller = new AbortController();
      exec = controller;
      void watchExec(context, execSession, controller.signal, (text) => {
        execOut.textContent = (execOut.textContent ?? "") + text;
        execOut.scrollTop = execOut.scrollHeight;
      }).catch(() => {
        if (!controller.signal.aborted) setStatus("Exec stream ended.", true);
      });
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function sendExec(): Promise<void> {
    if (!execSession) return;
    const text = `${execInput.value}\n`;
    execInput.value = "";
    try {
      await call(context, "/container/exec-write", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          session: execSession,
          data: Array.from(new TextEncoder().encode(text)),
        }),
      });
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  function closeExec(): void {
    exec?.abort();
    exec = null;
    if (execSession) {
      const session = execSession;
      execSession = null;
      void call(context, "/container/exec-close", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session }),
      }).catch(() => {});
    }
    if (execDialog.open) execDialog.close();
  }

  async function pull(): Promise<void> {
    const reference = pullReference.value.trim();
    if (!reference) {
      setStatus("An image reference is required.", true);
      return;
    }
    try {
      setStatus(`Pulling ${reference}…`);
      const response = await context.fetch(
        `/api/apps/${encodeURIComponent(context.app.id)}/images/pull?reference=${encodeURIComponent(reference)}`,
        { headers: { accept: "application/octet-stream" } },
      );
      if (!response.ok) throw new Error(await response.text());
      if (response.body) {
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          const text = decoder.decode(value, { stream: true });
          if (text.trim()) setStatus(text.trim().slice(0, 240));
        }
      }
      setStatus(`${reference} pulled.`);
      await loadImages();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  async function createVolume(): Promise<void> {
    const name = volumeName.value.trim();
    if (!name) {
      setStatus("A volume name is required.", true);
      return;
    }
    try {
      await call(context, "/volumes/create", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name }),
      });
      volumeName.value = "";
      await loadVolumes();
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  box.querySelector("[data-refresh]")?.addEventListener("click", () => {
    void loadContainers();
  });
  createButton.addEventListener("click", () => {
    void context.openModal?.({ tag: "containers-create-modal" });
  });
  for (const button of box.querySelectorAll<HTMLButtonElement>(".cn-tab")) {
    button.addEventListener("click", () => switchTab(button.dataset.tab as Tab));
  }
  box.querySelector("[data-pull]")?.addEventListener("click", () => void pull());
  box.querySelector("[data-volume-create]")?.addEventListener("click", () => void createVolume());
  box.querySelector("[data-confirm-cancel]")?.addEventListener("click", () => {
    pending = null;
    confirmDialog.close();
  });
  box.querySelector("[data-confirm-run]")?.addEventListener("click", () => {
    const run = pending;
    pending = null;
    if (!run) return;
    confirmDialog.close();
    void run().catch((error) => setStatus(String(error), true));
  });
  box.querySelector("[data-exec-send]")?.addEventListener("click", () => void sendExec());
  execInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void sendExec();
    }
  });
  box.querySelector("[data-exec-close]")?.addEventListener("click", () => closeExec());
  execDialog.addEventListener("close", () => {
    if (execSession) {
      const session = execSession;
      execSession = null;
      void call(context, "/container/exec-close", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session }),
      }).catch(() => {});
    }
    exec?.abort();
    exec = null;
  });

  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") void loadContainers();
  });
  const offVisibility = context.onVisibility?.((next) => {
    visible = next;
    if (next) void loadContainers();
  });
  const initial = context.params as { refresh?: boolean } | undefined;
  void loadContainers().then(() => {
    if (initial?.refresh) void loadContainers();
  });

  const poll = window.setInterval(() => {
    if (visible && !disposed && tab === "containers") void loadContainers();
  }, 5_000);

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    stopDetailStreams();
    window.clearInterval(poll);
    root.replaceChildren();
  };
}
