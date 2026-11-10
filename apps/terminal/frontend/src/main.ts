import { call } from "./api";
import { bindTerminalGestures } from "./gestures";
import {
  BAR_SYMBOLS,
  KEY_SEQUENCES,
  applyModifiers,
  encodeText,
  type KeyName,
} from "./keys";
import { styles } from "./styles";
import type { FrameContext } from "./types";

// The demo Terminal view: a scripted session from the terminal service.
//
// The service is polled through `read(id, cursor, wait-ms)`, which keeps the
// fixture honest: Phase 8 replaces the poll with `stream<u8>` without changing
// this route. Everything runs inside the sandboxed frame; the module only sees
// its own app prefix.

const decoder = new TextDecoder();

export function mount(root: HTMLElement, context: FrameContext) {
  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  const box = document.createElement("div");
  box.className = "term";
  box.innerHTML = `
    <div class="term-bar">
      <span class="who">operator@anthracite</span>
      <span class="fixture">· scripted fixture</span>
      <div class="term-controls">
        <button type="button" data-signal="INT" data-session-control>INT</button>
        <button type="button" data-signal="TERM" data-session-control>TERM</button>
        <button type="button" data-clear data-session-control>Clear</button>
        <button type="button" data-reconnect>Reconnect</button>
      </div>
      <span class="state">connecting…</span>
    </div>
    <pre class="term-screen" aria-label="Terminal output"></pre>
    <p class="term-status" role="status"></p>
    <div class="term-keys" data-keys aria-label="Terminal special keys">
      <div class="term-modifiers">
        <button type="button" data-mod="ctrl" aria-pressed="false" data-session-key>Ctrl</button>
        <button type="button" data-mod="alt" aria-pressed="false" data-session-key>Alt</button>
      </div>
      <div class="term-key-scroll" data-key-scroll></div>
    </div>
    <input class="term-input" aria-label="Terminal input" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false" enterkeyhint="send" disabled />
  `;
  root.append(box);

  const screen = box.querySelector(".term-screen") as any;
  const input = box.querySelector(".term-input") as any;
  const status = box.querySelector(".term-status") as any;
  const state = box.querySelector(".state") as any;
  const sessionControls: any[] = [...box.querySelectorAll("[data-session-control]")];
  const keyBar = box.querySelector(".term-keys") as any;
  const keyScroll = box.querySelector("[data-key-scroll]") as any;
  const ctrlButton = box.querySelector('[data-mod="ctrl"]') as any;
  const altButton = box.querySelector('[data-mod="alt"]') as any;
  const history = [];
  let historyIndex = 0;
  let session = null;
  let cursor = 0;
  let timer = null;
  let disposed = false;
  let ctrlArmed = false;
  let altArmed = false;
  let output = "";
  let pendingEcho = "";

  const updateModifiers = (): void => {
    for (const [button, armed] of [
      [ctrlButton, ctrlArmed],
      [altButton, altArmed],
    ] as const) {
      button.classList.toggle("armed", armed);
      button.setAttribute("aria-pressed", String(armed));
    }
  };

  const clearModifiers = (): void => {
    ctrlArmed = false;
    altArmed = false;
    updateModifiers();
  };

  const focusInput = (): void => {
    if (!input.disabled) input.focus({ preventScroll: true });
  };

  const getFontSize = (): number => {
    const value = Number.parseFloat(getComputedStyle(screen).fontSize);
    return Number.isFinite(value) ? value : 12.5;
  };

  const setFontSize = (size: number): void => {
    const next = Math.max(10, Math.min(20, Math.round(size * 10) / 10));
    screen.style.fontSize = `${next}px`;
  };

  const keyButton = (label: string, title: string): HTMLButtonElement => {
    const node = document.createElement("button");
    node.type = "button";
    node.innerHTML = label;
    node.title = title;
    node.setAttribute("aria-label", title);
    node.dataset.sessionKey = "";
    return node;
  };

  const arrow = (path: string): string =>
    `<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${path}</svg>`;

  const keyButtons: Array<[KeyName, string, string]> = [
    ["esc", "Esc", "Escape"],
    ["tab", "Tab", "Tab"],
    ["left", arrow('<path d="M12.5 8H3.5M6.5 5l-3 3 3 3"/>'), "Left arrow"],
    ["up", arrow('<path d="M8 12.5v-9M5 6.5l3-3 3 3"/>'), "Up arrow"],
    ["down", arrow('<path d="M8 3.5v9M5 9.5l3 3 3-3"/>'), "Down arrow"],
    ["right", arrow('<path d="M3.5 8h9M9.5 5l3 3-3 3"/>'), "Right arrow"],
    ["home", "Home", "Home"],
    ["end", "End", "End"],
    ["pgup", "PgUp", "Page up"],
    ["pgdn", "PgDn", "Page down"],
    ["ctrlc", "^C", "Interrupt"],
    ["ctrld", "^D", "End of input"],
    ["ctrll", "^L", "Clear screen"],
    ["ctrlz", "^Z", "Suspend"],
  ];
  for (const [key, label, title] of keyButtons) {
    const node = keyButton(label, title);
    node.dataset.key = key;
    keyScroll.append(node);
  }
  for (const symbol of BAR_SYMBOLS) {
    const node = keyButton(symbol, `Send ${symbol}`);
    node.dataset.symbol = symbol;
    keyScroll.append(node);
  }
  const enterKey = keyButton("Enter", "Send the current line");
  enterKey.dataset.enter = "true";
  keyScroll.append(enterKey);
  for (const [label, delta] of [
    ["A-", -1],
    ["A+", 1],
  ] as const) {
    const node = keyButton(label, delta < 0 ? "Smaller text" : "Larger text");
    delete node.dataset.sessionKey;
    node.dataset.font = String(delta);
    keyScroll.append(node);
  }

  const sessionKeys: any[] = [...box.querySelectorAll("[data-session-key]")];

  const setSessionControls = (enabled) => {
    for (const control of sessionControls) control.disabled = !enabled;
    for (const control of sessionKeys) control.disabled = !enabled;
    if (!enabled) clearModifiers();
  };
  setSessionControls(false);

  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "term-status bad" : "term-status";
  };

  const render = (): void => {
    screen.textContent = output + input.value;
    screen.scrollTop = screen.scrollHeight;
  };

  const append = (bytes) => {
    // The fixture echoes the bytes it receives, including `\r`; normalise line
    // endings so the `<pre>` shows real lines instead of overstrikes.
    const text = decoder.decode(Uint8Array.from(bytes)).replace(/\r\n?/g, "\n");
    if (pendingEcho && text === pendingEcho) {
      pendingEcho = "";
      return;
    }
    output += text;
    render();
  };

  async function submitLine(): Promise<void> {
    if (!session) return;
    const line = input.value;
    input.value = "";
    if (line.trim()) history.push(line);
    historyIndex = history.length;
    // Show the line locally; the service echoes the same bytes back, so the
    // next matching chunk is dropped instead of drawing it twice.
    output += `${line}\n`;
    render();
    pendingEcho = `${line}\n`;
    try {
      await writeText(`${line}\r`);
    } catch (error) {
      pendingEcho = "";
      setStatus(String(error), true);
    }
    focusInput();
  }

  async function writeText(value: string): Promise<void> {
    if (!session) return;
    await call(context, "/write", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, data: encodeText(value) }),
    });
  }

  const historyStep = (direction: "up" | "down"): void => {
    if (history.length === 0) return;
    historyIndex =
      direction === "up"
        ? Math.max(0, historyIndex - 1)
        : Math.min(history.length, historyIndex + 1);
    input.value = history[historyIndex] ?? "";
    render();
  };

  async function sendKey(key: KeyName): Promise<void> {
    if (key === "ctrlc") {
      input.value = "";
      render();
      clearModifiers();
      await sendSignal("INT");
      focusInput();
      return;
    }
    if (key === "ctrll") {
      output = "";
      render();
      clearModifiers();
      focusInput();
      return;
    }
    if (key === "up" || key === "down") {
      historyStep(key);
      clearModifiers();
      focusInput();
      return;
    }
    const sequence = KEY_SEQUENCES[key];
    if (!sequence) return;
    try {
      await writeText(sequence);
      clearModifiers();
    } catch (error) {
      setStatus(String(error), true);
    } finally {
      focusInput();
    }
  }

  async function sendSymbol(symbol: string): Promise<void> {
    if (!ctrlArmed && !altArmed) {
      input.value += symbol;
      render();
      focusInput();
      return;
    }
    try {
      await writeText(applyModifiers(symbol, ctrlArmed, altArmed));
      clearModifiers();
    } catch (error) {
      setStatus(String(error), true);
    } finally {
      focusInput();
    }
  }

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
      output = "";
      pendingEcho = "";
      input.value = "";
      render();
      cursor = 0;    }
    input.disabled = true;
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
      state.textContent = `session ${session.id}`;
      setStatus("Connected to the scripted fixture.");
      resize();
      void poll();
      input.focus();
    } catch (error) {
      session = null;
      setSessionControls(false);
      input.disabled = true;
      state.textContent = "unavailable";
      setStatus(String(error), true);
    }
  }

  async function sendSignal(name) {
    if (!session) return;
    if (name === "INT") {
      input.value = "";
      render();
    }
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

  input.addEventListener("input", () => {
    render();
  });

  input.addEventListener("focus", () => {
    screen.classList.add("focused");
  });
  input.addEventListener("blur", () => {
    screen.classList.remove("focused");
  });

  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void submitLine();
      return;
    }
    if (
      (ctrlArmed || altArmed) &&
      event.key.length === 1 &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.isComposing
    ) {
      event.preventDefault();
      const data = applyModifiers(event.key, ctrlArmed, altArmed);
      clearModifiers();
      if (data === "\x03") {
        input.value = "";
        render();
        void sendSignal("INT");
        return;
      }
      void writeText(data).catch((error) => setStatus(String(error), true));
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    if (history.length === 0) return;
    event.preventDefault();
    historyStep(event.key === "ArrowUp" ? "up" : "down");
  });

  for (const control of box.querySelectorAll("[data-signal]")) {
    control.addEventListener("click", () => {
      void sendSignal((control as any).dataset.signal);
    });
  }
  box.querySelector("[data-clear]").addEventListener("click", () => {
    output = "";
    pendingEcho = "";
    input.value = "";
    render();
    setStatus("Screen cleared");
  });

  keyBar.addEventListener("pointerdown", (event) => {
    if ((event.target as HTMLElement).closest("button")) event.preventDefault();
  });
  screen.addEventListener("pointerdown", (event) => {
    if (event.pointerType === "mouse") focusInput();
  });
  keyBar.addEventListener("click", (event) => {
    const target = (event.target as HTMLElement).closest("button") as
      | HTMLButtonElement
      | null;
    if (!target || target.disabled) return;
    if (target.dataset.mod === "ctrl") {
      ctrlArmed = !ctrlArmed;
      updateModifiers();
      focusInput();
      return;
    }
    if (target.dataset.mod === "alt") {
      altArmed = !altArmed;
      updateModifiers();
      focusInput();
      return;
    }
    if (target.dataset.enter === "true") {
      void submitLine();
      return;
    }
    const key = target.dataset.key as KeyName | undefined;
    if (key) {
      void sendKey(key);
      return;
    }
    const symbol = target.dataset.symbol;
    if (symbol) {
      void sendSymbol(symbol);
      return;
    }
    const delta = Number(target.dataset.font);
    if (delta) {
      setFontSize(getFontSize() + delta);
      focusInput();
    }
  });

  const unbindGestures = bindTerminalGestures(screen, {
    sendKey: (key) => void sendKey(key),
    focusInput,
    getFontSize,
    setFontSize,
  });
  const onViewport = (): void => {
    if (document.activeElement === input) {
      input.scrollIntoView({ block: "nearest" });
    }
  };
  window.visualViewport?.addEventListener("resize", onViewport);

  const onResize = () => resize();
  window.addEventListener("resize", onResize);
  box.querySelector("[data-reconnect]").addEventListener("click", () => {
    void open(true);
  });
  updateModifiers();
  void open();

  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
    unbindGestures();
    window.visualViewport?.removeEventListener("resize", onViewport);
    window.removeEventListener("resize", onResize);
    if (session) {
      void call(context, `/session?id=${encodeURIComponent(session.id)}`, {
        method: "DELETE",
      }).catch(() => {});
    }
    root.replaceChildren();
  };
}
