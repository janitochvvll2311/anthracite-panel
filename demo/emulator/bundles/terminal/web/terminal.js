// The demo Terminal view: a scripted session from the terminal service.
//
// The service is polled through `read(id, cursor, wait-ms)`, which keeps the
// fixture honest: Phase 8 replaces the poll with `stream<u8>` without changing
// this route. Everything runs inside the sandboxed frame; the module only sees
// its own app prefix.

const styles = `
  .term { display: flex; flex-direction: column; gap: 10px; min-height: 380px; }
  .term-bar { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #667087; }
  .term-bar .who { color: #8f98ab; }
  .term-bar .state { margin-left: auto; }
  .term-screen {
    flex: 1; min-height: 320px; margin: 0; padding: 12px 14px; overflow: auto;
    border: 1px solid #202632; border-radius: 9px; background: #080a0f;
    font: 12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; color: #c8d0df;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .term-form { display: flex; gap: 8px; }
  .term-form input {
    flex: 1; padding: 8px 11px; border-radius: 8px; border: 1px solid #2a3142;
    background: #0f1219; color: #e8ebf2; font: 12.5px ui-monospace, monospace;
  }
  .term-form input:focus { outline: 2px solid #7aa2ff66; outline-offset: 1px; border-color: #3a4560; }
  .term-form button {
    padding: 8px 13px; border-radius: 8px; border: 1px solid #3a4560; cursor: pointer;
    background: linear-gradient(180deg, #2a3550, #222b3f); color: #e8ebf2; font: inherit;
  }
  .term-status { margin: 0; min-height: 16px; font-size: 12px; color: #667087; }
  .term-status.bad { color: #ffb3ba; }
`;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

async function call(context, route, init) {
  const { headers, ...rest } = init ?? {};
  const response = await context.fetch(
    `/api/apps/${encodeURIComponent(context.app.id)}${route}`,
    {
      ...rest,
      headers: { accept: "application/json", ...(headers ?? {}) },
    },
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

export function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "term";
  box.innerHTML = `
    <div class="term-bar">
      <span class="who">operator@anthracite</span>
      <span>· scripted fixture</span>
      <span class="state">connecting…</span>
    </div>
    <pre class="term-screen" aria-label="Terminal output"></pre>
    <form class="term-form">
      <input name="line" aria-label="Terminal input" autocomplete="off" spellcheck="false" placeholder="Type a command and press Enter" />
      <button type="submit">Send</button>
    </form>
    <p class="term-status" role="status"></p>
  `;
  root.append(box);

  const screen = box.querySelector(".term-screen");
  const form = box.querySelector(".term-form");
  const input = form.querySelector("input");
  const status = box.querySelector(".term-status");
  const state = box.querySelector(".state");
  let session = null;
  let cursor = 0;
  let timer = null;
  let disposed = false;

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
      state.textContent = "degraded";
      setStatus(String(error), true);
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

  async function open() {
    try {
      session = await call(context, "/open", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ shell: "/bin/sh", cols: 80, rows: 24 }),
      });
      if (disposed) return;
      cursor = 0;
      state.textContent = `session ${session.id}`;
      setStatus("Connected to the scripted fixture.");
      resize();
      void poll();
      input.focus();
    } catch (error) {
      state.textContent = "unavailable";
      setStatus(String(error), true);
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!session) return;
    const line = input.value;
    input.value = "";
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

  const onResize = () => resize();
  window.addEventListener("resize", onResize);
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
