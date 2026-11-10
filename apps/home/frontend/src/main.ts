import { fetchHostInfo } from "./api";
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
  if (!subtitle || !status || !refresh) {
    return () => root.replaceChildren();
  }

  let disposed = false;
  let loading = false;

  function setStatus(message: string, bad = false): void {
    status!.textContent = message;
    status!.className = bad ? "home-status bad" : "home-status";
  }

  async function load(): Promise<void> {
    if (loading || disposed) return;
    loading = true;
    refresh!.disabled = true;
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
      if (!disposed) refresh!.disabled = false;
    }
  }

  refresh.addEventListener("click", () => void load());
  void load();
  const timer = window.setInterval(() => {
    if (!document.hidden) void load();
  }, 5000);

  return () => {
    disposed = true;
    window.clearInterval(timer);
    root.replaceChildren();
  };
}
