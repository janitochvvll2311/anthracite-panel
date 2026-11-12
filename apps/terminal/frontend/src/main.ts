import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import xtermCss from "@xterm/xterm/css/xterm.css" with { type: "text" };

import { call, openStream } from "./api";
import { bindTerminalGestures } from "./gestures";
import {
  BAR_SYMBOLS,
  KEY_SEQUENCES,
  applyModifiers,
  encodeText,
  type KeyName,
} from "./keys";
import { styles } from "./styles";
import type { FrameContext, FrameSocket } from "./types";

// The Terminal view renders the service's raw PTY stream with xterm.js. The
// panel owns the frame; this module only knows its own routes.
export function mount(root: HTMLElement, context: FrameContext) {
  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }

  const style = document.createElement("style");
  style.textContent = styles + xtermCss;
  root.append(style);

  const box = document.createElement("div");
  box.className = "term";
  box.innerHTML = `
    <div class="term-bar">
      <span class="who">terminal</span>
      <div class="term-controls">
        <button type="button" data-signal="INT" data-session-control>INT</button>
        <button type="button" data-signal="TERM" data-session-control>TERM</button>
        <button type="button" data-clear data-session-control>Clear</button>
        <button type="button" data-reconnect>Reconnect</button>
      </div>
      <span class="state">connecting…</span>
    </div>
    <div class="term-screen" data-terminal aria-label="Terminal output"></div>
    <p class="term-status" role="status"></p>
    <div class="term-keys" data-keys aria-label="Terminal special keys">
      <div class="term-modifiers">
        <button type="button" data-keyboard aria-label="Show keyboard" aria-pressed="false" data-session-key>
          <svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><rect x="1.5" y="4" width="13" height="8" rx="1.5"/><path d="M4 6.5h.01M6.5 6.5h.01M9 6.5h.01M11.5 6.5h.01M4 9h.01M11.5 9h.01M6 9h4"/></svg>
        </button>
        <button type="button" data-mod="ctrl" aria-pressed="false" data-session-key>Ctrl</button>
        <button type="button" data-mod="alt" aria-pressed="false" data-session-key>Alt</button>
      </div>
      <div class="term-key-scroll" data-key-scroll></div>
    </div>
    <div class="term-input" role="textbox" aria-label="Terminal input" aria-multiline="false" aria-disabled="true" contenteditable="false" autocapitalize="off" autocomplete="off" autocorrect="off" spellcheck="false" enterkeyhint="send" inputmode="text" data-form-type="other" data-lpignore="true" data-1p-ignore="true" data-bwignore></div>
  `;
  root.append(box);

  const screen = box.querySelector(".term-screen") as HTMLElement;
  const input = box.querySelector(".term-input") as HTMLElement;
  const status = box.querySelector(".term-status") as HTMLElement;
  const state = box.querySelector(".state") as HTMLElement;
  const who = box.querySelector(".who") as HTMLElement;
  const sessionControls = [
    ...box.querySelectorAll("[data-session-control]"),
  ] as HTMLButtonElement[];
  const keyBar = box.querySelector(".term-keys") as HTMLElement;
  const keyScroll = box.querySelector("[data-key-scroll]") as HTMLElement;
  const ctrlButton = box.querySelector('[data-mod="ctrl"]') as HTMLButtonElement;
  const altButton = box.querySelector('[data-mod="alt"]') as HTMLButtonElement;
  const keyboardButton = box.querySelector("[data-keyboard]") as HTMLButtonElement;

  let session: any = null;
  let streamAbort: AbortController | undefined;
  let socket: FrameSocket | undefined;
  let writeChain: Promise<void> = Promise.resolve();
  let decoder = new TextDecoder();
  let disposed = false;
  let ctrlArmed = false;
  let altArmed = false;
  let composing = false;
  let lastSize = "";

  const term = new Terminal({
    cols: 80,
    rows: 24,
    disableStdin: true,
    cursorBlink: true,
    cursorStyle: "block",
    scrollback: window.matchMedia("(max-width: 560px)").matches ? 2500 : 5000,
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: 13,
    theme: {
      background: "#080a0f",
      foreground: "#c8d0df",
      cursor: "#7aa2ff",
      cursorAccent: "#080a0f",
      selectionBackground: "#264f78",
      black: "#0b0e14",
      red: "#ff7d8a",
      green: "#6ee7b7",
      yellow: "#f7c948",
      blue: "#7aa2ff",
      magenta: "#c792ea",
      cyan: "#89ddff",
      white: "#e8ebf2",
      brightBlack: "#667087",
      brightRed: "#ff9aa5",
      brightGreen: "#9af3cf",
      brightYellow: "#ffe08a",
      brightBlue: "#a8c2ff",
      brightMagenta: "#ddb8ff",
      brightCyan: "#b6ecff",
      brightWhite: "#ffffff",
    },
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(screen);
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    term.loadAddon(webgl);
  } catch {
    // DOM renderer fallback for browsers without WebGL.
  }
  term.textarea?.setAttribute("tabindex", "-1");
  term.textarea?.setAttribute("readonly", "readonly");

  const fitTerminal = (): void => {
    if (screen.clientWidth === 0 || screen.clientHeight === 0) return;
    try {
      fitAddon.fit();
    } catch {
      // xterm may not have measured its cell size yet; the next resize retries.
    }
  };

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

  const inputEnabled = (): boolean =>
    input.getAttribute("contenteditable") !== "false";

  const setInputEnabled = (enabled: boolean): void => {
    input.setAttribute("contenteditable", enabled ? "true" : "false");
    input.setAttribute("aria-disabled", String(!enabled));
    if (!enabled) {
      input.blur();
      input.textContent = "";
    }
  };

  const updateKeyboard = (): void => {
    const visible = document.activeElement === input;
    keyboardButton.classList.toggle("armed", visible);
    keyboardButton.setAttribute("aria-pressed", String(visible));
    const label = visible ? "Hide keyboard" : "Show keyboard";
    keyboardButton.title = label;
    keyboardButton.setAttribute("aria-label", label);
  };

  const focusInput = (): void => {
    if (inputEnabled()) input.focus({ preventScroll: true });
  };

  const toggleKeyboard = (): void => {
    if (document.activeElement === input) input.blur();
    else focusInput();
    updateKeyboard();
  };

  const getFontSize = (): number => term.options.fontSize ?? 13;

  const setFontSize = (size: number): void => {
    term.options.fontSize = Math.max(10, Math.min(20, Math.round(size * 10) / 10));
    fitTerminal();
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

  const sessionKeys = [
    ...box.querySelectorAll("[data-session-key]"),
  ] as HTMLButtonElement[];

  const setSessionControls = (enabled: boolean): void => {
    for (const control of sessionControls) control.disabled = !enabled;
    for (const control of sessionKeys) control.disabled = !enabled;
    setInputEnabled(enabled);
    if (!enabled) clearModifiers();
    updateKeyboard();
  };
  setSessionControls(false);

  const setStatus = (message: string, bad = false): void => {
    status.textContent = message ?? "";
    status.className = bad ? "term-status bad" : "term-status";
  };

  async function writeText(value: string): Promise<void> {
    if (!session) return;
    await call(context, "/write", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, data: encodeText(value) }),
    });
  }

  const sendRaw = (value: string): void => {
    if (!value) return;
    if (socket) {
      // The socket carries input as a frame, so a keystroke is not an HTTP call.
      socket.send(new Uint8Array(encodeText(value)).buffer);
      return;
    }
    void writeText(value).catch((error) => setStatus(String(error), true));
  };

  const sendTyped = (value: string): void => {
    if (!value) return;
    let data = value;
    if (ctrlArmed || altArmed) {
      const characters = [...value];
      data =
        applyModifiers(characters[0], ctrlArmed, altArmed) +
        characters.slice(1).join("");
      clearModifiers();
    }
    sendRaw(data);
  };

  async function sendSignal(name: string): Promise<void> {
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

  async function sendKey(key: KeyName): Promise<void> {
    if (key === "ctrlc") {
      clearModifiers();
      await sendSignal("INT");
      focusInput();
      return;
    }
    const sequence = KEY_SEQUENCES[key];
    if (!sequence) return;
    sendRaw(sequence);
    clearModifiers();
    focusInput();
  }

  async function sendSymbol(symbol: string): Promise<void> {
    const data = applyModifiers(symbol, ctrlArmed, altArmed);
    sendRaw(data);
    clearModifiers();
    focusInput();
  }

  const sendResize = (cols: number, rows: number): void => {
    if (!session) return;
    const key = `${cols}x${rows}`;
    if (key === lastSize) return;
    lastSize = key;
    void call(context, "/resize", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, cols, rows }),
    }).catch((error) => setStatus(String(error), true));
  };

  term.onResize(({ cols, rows }) => {
    sendResize(cols, rows);
  });
  term.onData((data) => {
    if (data) sendRaw(data);
  });
  term.textarea?.addEventListener("focus", () => {
    term.textarea?.blur();
    focusInput();
  });
  screen.addEventListener("focusin", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.classList.contains("xterm-helper-textarea")) {
      target.blur();
      focusInput();
    }
  });

  const append = (bytes: Uint8Array): Promise<void> =>
    new Promise((resolve) => {
      const text = decoder.decode(bytes, { stream: true });
      if (!text) {
        resolve();
        return;
      }
      term.write(text, resolve);
    });

  const endSession = (): void => {
    const ended = session?.id;
    session = null;
    if (ended) {
      void call(context, `/session?id=${encodeURIComponent(ended)}`, {
        method: "DELETE",
      }).catch(() => {});
    }
    setSessionControls(false);
    state.textContent = "ended";
    setStatus("Session ended. Reconnect to start a new one.", true);
    term.write("\r\n[session ended]\r\n");
  };

  // One live follower per session. Waiting for xterm to parse each chunk before
  // reading the next keeps the frame's stream queue bounded.
  const follow = async (): Promise<void> => {
    if (disposed || !session) return;
    const controller = new AbortController();
    streamAbort = controller;
    try {
      const reader = await openStream(
        context,
        `/stream?id=${encodeURIComponent(session.id)}&seq=0`,
        controller.signal,
      );
      for (;;) {
        const { done, value } = await reader.read();
        if (disposed) return;
        if (done) break;
        if (value) await append(value);
      }
      if (disposed || controller.signal.aborted) return;
      endSession();
    } catch (error) {
      if (disposed || controller.signal.aborted) return;
      state.textContent = "degraded";
      setStatus(error instanceof Error ? error.message : String(error), true);
    } finally {
      if (streamAbort === controller) streamAbort = undefined;
    }
  };

  // The socket carries both directions: output frames from the app stream and
  // input frames that the engine turns into `write` calls. Panels without the
  // relay fall back to the chunked route above.
  const connectSocket = (): void => {
    if (disposed || !session || !context.socket) return;
    const openSocket = context.socket;
    socket = openSocket(
      `/api/apps/${encodeURIComponent(context.app.id)}/socket?id=${encodeURIComponent(session.id)}&seq=0`,
      {
        onOpen: () => {
          if (!disposed) setStatus("Connected.");
        },
        onMessage: (data) => {
          writeChain = writeChain
            .then(() => append(new Uint8Array(data)))
            .catch(() => {});
          return writeChain;
        },
        onText: (text) => {
          writeChain = writeChain
            .then(() => append(new TextEncoder().encode(text)))
            .catch(() => {});
          return writeChain;
        },
        onClose: (error, code) => {
          socket = undefined;
          if (disposed) return;
          if (error || (code !== undefined && code !== 1000 && code !== 1005)) {
            state.textContent = "degraded";
            setStatus(error ?? `socket closed (${code})`, true);
            return;
          }
          endSession();
        },
      },
    );
  };

  async function open(reset = false) {
    if (reset) {
      streamAbort?.abort();
      streamAbort = undefined;
      socket?.close();
      socket = undefined;
      const previous = session?.id;
      session = null;
      if (previous) {
        await call(context, `/session?id=${encodeURIComponent(previous)}`, {
          method: "DELETE",
        }).catch(() => {});
      }
      decoder = new TextDecoder();
      writeChain = Promise.resolve();
      term.reset();
    }
    setInputEnabled(false);
    setSessionControls(false);
    fitTerminal();
    const cols = Math.max(20, term.cols || 80);
    const rows = Math.max(5, term.rows || 24);
    lastSize = "";
    try {
      session = await call(context, "/open", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ shell: "/bin/sh", cols, rows }),
      });
      if (disposed) return;
      lastSize = `${cols}x${rows}`;
      setInputEnabled(true);
      setSessionControls(true);
      state.textContent = `session ${session.id}`;
      who.textContent = `${session.owner} · ${session.shell}`;
      setStatus("Connected.");
      sendResize(term.cols, term.rows);
      if (context.socket) connectSocket();
      else void follow();
      focusInput();
    } catch (error) {
      session = null;
      setSessionControls(false);
      state.textContent = "unavailable";
      who.textContent = "terminal";
      lastSize = "";
      setStatus(String(error), true);
    }
  }

  const consumeInput = (): void => {
    const text = input.textContent ?? "";
    input.textContent = "";
    if (text) sendTyped(text);
  };

  input.addEventListener("beforeinput", (event) => {
    if (composing) return;
    const type = (event as InputEvent).inputType;
    if (
      type === "insertText" ||
      type === "insertFromPaste" ||
      type === "insertReplacementText"
    ) {
      const data = (event as InputEvent).data;
      if (data) {
        event.preventDefault();
        input.textContent = "";
        sendTyped(data);
      }
      return;
    }
    if (type === "deleteContentBackward") {
      event.preventDefault();
      sendRaw("\x7f");
      return;
    }
    if (type === "deleteContentForward") {
      event.preventDefault();
      sendRaw("\x1b[3~");
      return;
    }
    if (type === "insertLineBreak" || type === "insertParagraph") {
      event.preventDefault();
      sendRaw("\r");
    }
  });
  input.addEventListener("input", () => {
    if (!composing) consumeInput();
  });
  input.addEventListener("compositionstart", () => {
    composing = true;
  });
  input.addEventListener("compositionend", () => {
    composing = false;
    consumeInput();
  });
  input.addEventListener("focus", () => {
    input.textContent = "";
    updateKeyboard();
  });
  input.addEventListener("blur", () => {
    input.textContent = "";
    updateKeyboard();
  });

  input.addEventListener("keydown", (event) => {
    const sequences: Record<string, string> = {
      Enter: "\r",
      Backspace: "\x7f",
      Delete: "\x1b[3~",
      Tab: "\t",
      Escape: "\x1b",
      ArrowUp: KEY_SEQUENCES.up,
      ArrowDown: KEY_SEQUENCES.down,
      ArrowLeft: KEY_SEQUENCES.left,
      ArrowRight: KEY_SEQUENCES.right,
      Home: KEY_SEQUENCES.home,
      End: KEY_SEQUENCES.end,
      PageUp: KEY_SEQUENCES.pgup,
      PageDown: KEY_SEQUENCES.pgdn,
    };
    const sequence = sequences[event.key];
    if (sequence) {
      event.preventDefault();
      sendRaw(sequence);
      return;
    }
    if (
      (event.ctrlKey || event.altKey || ctrlArmed || altArmed) &&
      event.key.length === 1 &&
      !event.metaKey &&
      !event.isComposing
    ) {
      event.preventDefault();
      const ctrl = event.ctrlKey || ctrlArmed;
      const alt = event.altKey || altArmed;
      clearModifiers();
      sendRaw(applyModifiers(event.key, ctrl, alt));
    }
  });

  box.querySelector("[data-clear]")!.addEventListener("click", () => {
    term.write("\x1b[2J\x1b[3J\x1b[H");
    setStatus("Screen cleared");
  });
  for (const control of box.querySelectorAll("[data-signal]")) {
    control.addEventListener("click", () => {
      void sendSignal((control as HTMLElement).dataset.signal ?? "");
    });
  }

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
    if (target.hasAttribute("data-keyboard")) {
      toggleKeyboard();
      return;
    }
    if (target.dataset.enter === "true") {
      sendRaw("\r");
      focusInput();
      return;
    }
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
  const onResize = (): void => {
    fitTerminal();
    updateKeyboard();
  };
  window.addEventListener("resize", onResize);
  box.querySelector("[data-reconnect]")!.addEventListener("click", () => {
    void open(true);
  });
  updateModifiers();
  requestAnimationFrame(fitTerminal);
  void open();

  return () => {
    disposed = true;
    streamAbort?.abort();
    streamAbort = undefined;
    socket?.close();
    socket = undefined;
    unbindGestures();
    window.visualViewport?.removeEventListener("resize", onViewport);
    window.removeEventListener("resize", onResize);
    if (session) {
      void call(context, `/session?id=${encodeURIComponent(session.id)}`, {
        method: "DELETE",
      }).catch(() => {});
    }
    term.dispose();
    root.replaceChildren();
  };
}
