import { fetchHostInfo, watchHostInfo } from "./api";
import { query } from "./dom";
import { renderHost } from "./render";
import { styles } from "./styles";
import type { FrameContext } from "./types";

const TEMPLATE = `
  <header class="home-head">
    <div>
      <h3>Home</h3>
      <p class="home-sub" data-subtitle>Reading host information…</p>
    </div>
    <div class="home-actions">
      <label class="home-interval">Interval
        <select data-interval aria-label="Watch interval">
          <option value="0">Manual (0s)</option>
          <option value="1000">1s</option>
          <option value="2000" selected>2s</option>
          <option value="5000">5s</option>
          <option value="10000">10s</option>
        </select>
      </label>
      <button class="home-refresh" type="button" data-refresh>Refresh</button>
    </div>
  </header>
  <div class="home-grid">
    <section class="home-card"><h4>System</h4><dl data-system></dl></section>
    <section class="home-card"><h4>CPU</h4><div data-cpu></div></section>
    <section class="home-card"><h4>Memory</h4><div data-memory></div></section>
    <section class="home-card wide"><h4>Disks</h4><div data-disks></div></section>
    <section class="home-card wide"><h4>Network</h4><div data-network></div></section>
  </div>
  <p class="home-status" data-status role="status"></p>
`;

export function mount(root: HTMLElement, context: FrameContext): () => void {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "home";
  box.innerHTML = TEMPLATE;
  root.append(box);

  const subtitle = query<HTMLElement>(box, "[data-subtitle]");
  const status = query<HTMLElement>(box, "[data-status]");
  const refresh = query<HTMLButtonElement>(box, "[data-refresh]");
  const interval = query<HTMLSelectElement>(box, "[data-interval]");
  if (!subtitle || !status || !refresh || !interval) {
    return () => root.replaceChildren();
  }
  const refreshButton = refresh;

  // The FAB is the refresh control in panels that implement it. A panel that
  // does not implement `panel.fab` keeps the toolbar button, so manual mode
  // never becomes a dead end.
  if (context.panel?.slots?.includes("panel.fab")) refreshButton.remove();

  let disposed = false;
  let loading = false;
  let intervalMs = 2000;
  let visible = true;
  let controller: AbortController | undefined;

  function setStatus(message: string, bad = false): void {
    status!.textContent = message;
    status!.className = bad ? "home-status bad" : "home-status";
  }

  async function load(): Promise<void> {
    if (loading || disposed) return;
    loading = true;
    refreshButton.disabled = true;
    setStatus("Reading host information…");
    try {
      const info = await fetchHostInfo(context);
      if (disposed) return;
      subtitle!.textContent = `${info.hostname} · ${info.os}`;
      renderHost(box, info);
      setStatus(`Updated ${new Date().toLocaleTimeString()}`);
    } catch (error) {
      if (!disposed) {
        setStatus(
          `Could not read host information: ${
            error instanceof Error ? error.message : String(error)
          }`,
          true,
        );
      }
    } finally {
      loading = false;
      if (!disposed) refreshButton.disabled = false;
    }
  }

  // The engine frames each host snapshot as an SSE event, so the view follows a
  // single response instead of polling `/info` every few seconds.
  async function watch(): Promise<void> {
    if (disposed || intervalMs === 0 || !visible) return;
    const abort = new AbortController();
    controller = abort;
    setStatus("Watching host information…");
    try {
      await watchHostInfo(context, abort.signal, intervalMs, (info) => {
        if (disposed) return;
        subtitle!.textContent = `${info.hostname} · ${info.os}`;
        renderHost(box, info);
        setStatus(`Live · ${new Date().toLocaleTimeString()}`);
      });
      if (!disposed && !abort.signal.aborted) {
        setStatus("The host watch ended.", true);
      }
    } catch (error) {
      if (disposed || abort.signal.aborted) return;
      setStatus(
        `Could not watch host information: ${
          error instanceof Error ? error.message : String(error)
        }`,
        true,
      );
      // A missed SSE stream must not leave the view empty.
      void load();
    } finally {
      if (controller === abort) controller = undefined;
    }
  }

  function restart(): void {
    controller?.abort();
    controller = undefined;
    if (intervalMs > 0) void watch();
    else void load();
  }

  interval.addEventListener("change", () => {
    intervalMs = Number(interval.value) || 0;
    restart();
  });
  refreshButton.addEventListener("click", restart);
  const offAction = context.onAction?.((tag) => {
    if (tag === "refresh") restart();
  });
  const offVisibility = context.onVisibility?.((next) => {
    if (next === visible) return;
    visible = next;
    if (!visible) {
      controller?.abort();
      controller = undefined;
    } else if (intervalMs > 0) {
      void watch();
    }
  });
  void (intervalMs > 0 ? watch() : load());

  return () => {
    disposed = true;
    offAction?.();
    offVisibility?.();
    controller?.abort();
    controller = undefined;
    root.replaceChildren();
  };
}
