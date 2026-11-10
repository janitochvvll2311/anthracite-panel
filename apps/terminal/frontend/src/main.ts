import { call } from "./api";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Terminal view: a scripted session from the terminal service.
//
// The service is polled through `read(id, cursor, wait-ms)`, which keeps the
// fixture honest: Phase 8 replaces the poll with `stream<u8>` without changing
// this route. Everything runs inside the sandboxed frame; the module only sees
// its own app prefix.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function mount(root: HTMLElement, context: FrameContext) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "term";
  box.innerHTML = `
    <div class="term-bar">
      <span class="who">operator@anthracite</span>
      <span>· scripted fixture</span>
      <button type="button" data-signal="INT" data-session-control>INT</button>
      <button type="button" data-signal="TERM" data-session-control>TERM</button>
      <button type="button" data-clear data-session-control>Clear</button>
      <button type="button" data-reconnect>Reconnect</button>
      <span class="state">connecting…</span>
    </div>
    <pre class="term-screen" aria-label="Terminal output"></pre>
    <form class="term-form">
      <input name="line" aria-label="Terminal input" autocomplete="off" spellcheck="false" placeholder="Type a command and press Enter" disabled />
      <button type="submit" disabled>Send</button>
    </form>
    <p class="term-status" role="status"></p>
  `;
  root.append(box);

  const screen = box.querySelector(".term-screen") as any;
  const form = box.querySelector(".term-form") as any;
  const input = form.querySelector("input") as any;
  const send = form.querySelector("button") as any;
  const status = box.querySelector(".term-status") as any;
  const state = box.querySelector(".state") as any;
  const sessionControls: any[] = [...box.querySelectorAll("[data-session-control]")];
  const history = [];
  let historyIndex = 0;
  let session = null;
  let cursor = 0;
  let timer = null;
  let disposed = false;

  const setSessionControls = (enabled) => {
    for (const control of sessionControls) control.disabled = !enabled;
  };
  setSessionControls(false);

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "term-status bad" : "term-status";
  };

  const append = (bytes) => {
    // The fixture echoes the bytes it receives, including `\r`; normalise line
    // endings so the `<pre>` shows real lines instead of overstrikes.
    const text = decoder.decode(Uint8Array.from(bytes)).replace(/\r\n?/g, "\n");
    screen.textContent += text;
    screen.scrollTop = screen.scrollHeight;
  };

  const poll = async () => {
    if (disposed || !session) return;
    try {
      const output = await call(
        context,
        `/read?id=${encodeURIComponent(session.id)}&seq=${cursor}&wait-ms=500`,
      );
      if (disposed) return;
      if (output) {
        append(output.data);
        cursor = output.seq + 1;
        timer = setTimeout(poll, 20);
      } else {
        timer = setTimeout(poll, 200);
      }
    } catch (error) {
      if (disposed) return;
      const text = error instanceof Error ? error.message : String(error);
      if (text.startsWith("not-found:")) {
        session = null;
        input.disabled = true;
        send.disabled = true;
        setSessionControls(false);
        state.textContent = "ended";
        setStatus("Session ended. Reconnect to start a new one.", true);
        return;
      }
      state.textContent = "degraded";
      setStatus(text, true);
      timer = setTimeout(poll, 500);
    }
  };

  const resize = () => {
    if (!session) return;
    const cols = Math.max(40, Math.floor(screen.clientWidth / 7.6));
    const rows = Math.max(12, Math.floor(screen.clientHeight / 18));
    void call(context, "/resize", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, cols, rows }),
    }).catch((error) => setStatus(String(error), true));
  };

  async function open(reset = false) {
    if (reset) {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      if (session) {
        const previous = session.id;
        session = null;
        await call(context, `/session?id=${encodeURIComponent(previous)}`, {
          method: "DELETE",
        }).catch(() => {});
      }
      screen.textContent = "";
      cursor = 0;
    }
    input.disabled = true;
    send.disabled = true;
    setSessionControls(false);
    try {
      session = await call(context, "/open", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ shell: "/bin/sh", cols: 80, rows: 24 }),
      });
      if (disposed) return;
      cursor = 0;
      input.disabled = false;
      setSessionControls(true);
      send.disabled = false;
      state.textContent = `session ${session.id}`;
      setStatus("Connected to the scripted fixture.");
      resize();
      void poll();
      input.focus();
    } catch (error) {
      session = null;
      setSessionControls(false);
      input.disabled = true;
      send.disabled = true;
      state.textContent = "unavailable";
      setStatus(String(error), true);
    }
  }

  async function sendSignal(name) {
    if (!session) return;
    try {
      await call(context, "/signal", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: session.id, signal: name }),
      });
      setStatus(`${name} sent to ${session.id}`);
    } catch (error) {
      setStatus(String(error), true);
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!session) return;
    const line = input.value;
    input.value = "";
    if (line.trim()) history.push(line);
    historyIndex = history.length;
    try {
      await call(context, "/write", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          id: session.id,
          data: Array.from(encoder.encode(`${line}\r`)),
        }),
      });
    } catch (error) {
      setStatus(String(error), true);
    }
  });

  input.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    if (history.length === 0) return;
    event.preventDefault();
    historyIndex =
      event.key === "ArrowUp"
        ? Math.max(0, historyIndex - 1)
        : Math.min(history.length, historyIndex + 1);
    input.value = history[historyIndex] ?? "";
  });

  for (const control of box.querySelectorAll("[data-signal]")) {
    control.addEventListener("click", () => {
      void sendSignal((control as any).dataset.signal);
    });
  }
  box.querySelector("[data-clear]").addEventListener("click", () => {
    screen.textContent = "";
    setStatus("Screen cleared");
  });

  const onResize = () => resize();
  window.addEventListener("resize", onResize);
  box.querySelector("[data-reconnect]").addEventListener("click", () => {
    void open(true);
  });
  void open();

  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
    window.removeEventListener("resize", onResize);
    if (session) {
      void call(context, `/session?id=${encodeURIComponent(session.id)}`, {
        method: "DELETE",
      }).catch(() => {});
    }
    root.replaceChildren();
  };
}
