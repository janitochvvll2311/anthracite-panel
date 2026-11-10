// src/api.ts
async function call(context, route, init = {}) {
  const { headers, ...rest } = init;
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}${route}`, {
    ...rest,
    headers: { accept: "application/json", ...headers ?? {} }
  });
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message = value && typeof value === "object" && value.error ? `${value.error.code}: ${value.error.message}` : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}

// src/gestures.ts
function bindTerminalGestures(screen, handlers) {
  let lastTap = 0;
  let tapReset;
  let hold;
  let startX = 0;
  let startY = 0;
  let lastX = 0;
  let lastY = 0;
  let arrows = false;
  let moved = false;
  let pinching = false;
  let suppressTap = false;
  let pinchStart = 0;
  let pinchFont = 0;
  const clearHold = () => {
    if (hold !== undefined) {
      window.clearTimeout(hold);
      hold = undefined;
    }
  };
  const distance = (touches) => {
    const first = touches[0];
    const second = touches[1];
    if (!first || !second)
      return 0;
    return Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY);
  };
  const onStart = (event) => {
    if (event.touches.length === 2) {
      clearHold();
      arrows = false;
      moved = false;
      pinching = true;
      suppressTap = true;
      pinchStart = distance(event.touches);
      pinchFont = handlers.getFontSize();
      event.preventDefault();
      return;
    }
    if (event.touches.length !== 1)
      return;
    const touch = event.touches[0];
    if (!touch)
      return;
    suppressTap = false;
    startX = lastX = touch.clientX;
    startY = lastY = touch.clientY;
    moved = false;
    arrows = false;
    pinching = false;
    clearHold();
    hold = window.setTimeout(() => {
      hold = undefined;
      arrows = true;
    }, 450);
  };
  const onMove = (event) => {
    if (pinching) {
      if (event.touches.length < 2)
        return;
      event.preventDefault();
      const current = distance(event.touches);
      if (pinchStart > 0) {
        handlers.setFontSize(pinchFont * (current / pinchStart));
      }
      return;
    }
    const touch = event.touches[0];
    if (!touch)
      return;
    if (arrows) {
      event.preventDefault();
      window.getSelection()?.removeAllRanges();
      const deltaX = touch.clientX - lastX;
      const deltaY = touch.clientY - lastY;
      const threshold = 24;
      if (Math.abs(deltaX) >= threshold || Math.abs(deltaY) >= threshold) {
        if (Math.abs(deltaX) >= Math.abs(deltaY)) {
          handlers.sendKey(deltaX > 0 ? "right" : "left");
        } else {
          handlers.sendKey(deltaY > 0 ? "down" : "up");
        }
        lastX = touch.clientX;
        lastY = touch.clientY;
      }
      return;
    }
    if (Math.abs(touch.clientX - startX) > 8 || Math.abs(touch.clientY - startY) > 8) {
      moved = true;
      clearHold();
    }
  };
  const onEnd = (event) => {
    if (pinching) {
      if (event.touches.length >= 2)
        return;
      pinching = false;
      suppressTap = true;
      return;
    }
    if (arrows) {
      arrows = false;
      moved = true;
      return;
    }
    clearHold();
    if (moved) {
      moved = false;
      return;
    }
    if (suppressTap) {
      suppressTap = false;
      return;
    }
    const now = Date.now();
    if (now - lastTap < 320) {
      event.preventDefault();
      lastTap = 0;
      if (tapReset !== undefined) {
        window.clearTimeout(tapReset);
        tapReset = undefined;
      }
      handlers.sendKey("tab");
      handlers.focusInput();
      return;
    }
    lastTap = now;
    event.preventDefault();
    handlers.focusInput();
    if (tapReset !== undefined)
      window.clearTimeout(tapReset);
    tapReset = window.setTimeout(() => {
      lastTap = 0;
      tapReset = undefined;
    }, 320);
  };
  const onCancel = () => {
    clearHold();
    arrows = false;
    moved = false;
    pinching = false;
    suppressTap = false;
  };
  screen.addEventListener("touchstart", onStart, { passive: false });
  screen.addEventListener("touchmove", onMove, { passive: false });
  screen.addEventListener("touchend", onEnd, { passive: false });
  screen.addEventListener("touchcancel", onCancel, { passive: false });
  return () => {
    clearHold();
    if (tapReset !== undefined)
      window.clearTimeout(tapReset);
    screen.removeEventListener("touchstart", onStart);
    screen.removeEventListener("touchmove", onMove);
    screen.removeEventListener("touchend", onEnd);
    screen.removeEventListener("touchcancel", onCancel);
  };
}

// src/keys.ts
var KEY_SEQUENCES = {
  esc: "\x1B",
  tab: "\t",
  up: "\x1B[A",
  down: "\x1B[B",
  right: "\x1B[C",
  left: "\x1B[D",
  home: "\x1B[H",
  end: "\x1B[F",
  pgup: "\x1B[5~",
  pgdn: "\x1B[6~",
  ctrlc: "\x03",
  ctrld: "\x04",
  ctrll: "\f",
  ctrlz: "\x1A"
};
var BAR_SYMBOLS = ["|", "~", "/", "-", "_", "="];
var encoder = new TextEncoder;
function encodeText(value) {
  return Array.from(encoder.encode(value));
}
function applyModifiers(value, ctrl, alt) {
  if (!ctrl && !alt || [...value].length !== 1)
    return value;
  const code = value.codePointAt(0) ?? 0;
  if (code < 32 || code === 127)
    return value;
  if (ctrl) {
    const upper = value.toUpperCase().codePointAt(0) ?? 0;
    if (upper >= 64 && upper <= 95) {
      return String.fromCharCode(upper & 31);
    }
    if (value === "?")
      return "";
    if (value === " ")
      return "\x00";
  }
  return alt ? `\x1B${value}` : value;
}

// src/styles.ts
var styles = `
  .term { position: relative; display: flex; flex-direction: column; gap: 10px; height: 100%; min-height: 0; }
  .term-bar { display: flex; align-items: center; gap: 8px; flex: none; font-size: 12px; color: #667087; }
  .term-bar .who { color: #8f98ab; }
  .term-bar .state { margin-left: auto; }
  .term-controls { display: flex; align-items: center; gap: 4px; }
  .term-bar button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .term-bar button:hover { border-color: #3a4560; color: #fff; }
  .term-bar button:disabled { opacity: .4; cursor: default; }
  .term-screen {
    flex: 1; min-height: 0; margin: 0; padding: 12px 14px; overflow: auto;
    border: 1px solid #202632; border-radius: 9px; background: #080a0f;
    font: 12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; color: #c8d0df;
    white-space: pre-wrap; overflow-wrap: anywhere; touch-action: pan-y;
    cursor: text;
  }
  .term-screen.focused::after {
    content: "▌"; margin-left: 1px; color: #7aa2ff;
    animation: term-blink 1s steps(1) infinite;
  }
  @keyframes term-blink {
    50% { opacity: 0; }
  }
  .term-keys { display: none; }
  .term-keys button {
    flex: none; min-width: 34px; height: 36px; padding: 0 7px;
    border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: 11px ui-monospace, monospace; white-space: nowrap;
    cursor: pointer;
  }
  .term-keys button:hover { border-color: #3a4560; color: #fff; }
  .term-keys button.armed { background: #24406e; border-color: #7aa2ff; color: #fff; }
  .term-keys button:disabled { opacity: .4; cursor: default; }
  .term-keys button svg { display: block; width: 15px; height: 15px; }
  .term-input {
    position: absolute; left: 8px; bottom: 8px; width: 1px; height: 1px;
    padding: 0; border: 0; opacity: 0; pointer-events: none;
    font-size: 16px; caret-color: transparent; outline: none;
    overflow: hidden; white-space: pre;
  }
  .term-status { margin: 0; min-height: 16px; flex: none; font-size: 12px; color: #667087; }
  .term-status.bad { color: #ffb3ba; }
  @media (max-width: 560px) {
    .term { gap: 8px; }
    .term-bar {
      display: grid; grid-template-columns: minmax(0, 1fr) auto;
      align-items: center; gap: 6px 8px;
    }
    .term-bar .who {
      grid-column: 1; grid-row: 1; min-width: 0;
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .term-bar .fixture { display: none; }
    .term-bar .state {
      grid-column: 2; grid-row: 1; margin-left: 0;
      font-size: 11px; white-space: nowrap;
    }
    .term-controls {
      grid-column: 1 / -1; grid-row: 2;
      display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px;
    }
    .term-controls button {
      width: 100%; min-height: 38px; padding: 0 6px;
      font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .term-keys { display: flex; gap: 6px; flex: none; min-width: 0; }
    .term-modifiers {
      display: flex; gap: 4px; flex: none; padding-right: 6px;
      border-right: 1px solid #202634;
    }
    .term-key-scroll {
      display: flex; gap: 4px; flex: 1; min-width: 0; overflow-x: auto;
      overflow-y: hidden; overscroll-behavior-x: contain; touch-action: pan-x;
      -webkit-overflow-scrolling: touch; scrollbar-width: thin;
    }
    .term-keys button.armed { box-shadow: inset 0 0 0 1px #7aa2ff; }
    .term-screen { padding: 10px 12px; font-size: 12px; }
    .term-status { font-size: 11.5px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .term-screen.focused::after { animation: none; }
  }
`;

// src/main.ts
var decoder = new TextDecoder;
function mount(root, context) {
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
  const screen = box.querySelector(".term-screen");
  const input = box.querySelector(".term-input");
  const status = box.querySelector(".term-status");
  const state = box.querySelector(".state");
  const sessionControls = [...box.querySelectorAll("[data-session-control]")];
  const keyBar = box.querySelector(".term-keys");
  const keyScroll = box.querySelector("[data-key-scroll]");
  const ctrlButton = box.querySelector('[data-mod="ctrl"]');
  const altButton = box.querySelector('[data-mod="alt"]');
  const keyboardButton = box.querySelector("[data-keyboard]");
  const history = [];
  let historyIndex = 0;
  let session = null;
  let cursor = 0;
  let timer = null;
  let disposed = false;
  let ctrlArmed = false;
  let altArmed = false;
  let composing = false;
  let draft = "";
  let output = "";
  let pendingEcho = "";
  const updateModifiers = () => {
    for (const [button, armed] of [
      [ctrlButton, ctrlArmed],
      [altButton, altArmed]
    ]) {
      button.classList.toggle("armed", armed);
      button.setAttribute("aria-pressed", String(armed));
    }
  };
  const clearModifiers = () => {
    ctrlArmed = false;
    altArmed = false;
    updateModifiers();
  };
  const inputEnabled = () => input.getAttribute("contenteditable") !== "false";
  const setInputEnabled = (enabled) => {
    input.setAttribute("contenteditable", enabled ? "true" : "false");
    input.setAttribute("aria-disabled", String(!enabled));
    if (!enabled) {
      input.blur();
      input.textContent = "";
    }
  };
  const updateKeyboard = () => {
    const visible = document.activeElement === input;
    keyboardButton.classList.toggle("armed", visible);
    keyboardButton.setAttribute("aria-pressed", String(visible));
    const label = visible ? "Hide keyboard" : "Show keyboard";
    keyboardButton.title = label;
    keyboardButton.setAttribute("aria-label", label);
  };
  const toggleKeyboard = () => {
    if (document.activeElement === input)
      input.blur();
    else
      focusInput();
    updateKeyboard();
  };
  const focusInput = () => {
    if (inputEnabled())
      input.focus({ preventScroll: true });
  };
  const getFontSize = () => {
    const value = Number.parseFloat(getComputedStyle(screen).fontSize);
    return Number.isFinite(value) ? value : 12.5;
  };
  const setFontSize = (size) => {
    const next = Math.max(10, Math.min(20, Math.round(size * 10) / 10));
    screen.style.fontSize = `${next}px`;
  };
  const keyButton = (label, title) => {
    const node = document.createElement("button");
    node.type = "button";
    node.innerHTML = label;
    node.title = title;
    node.setAttribute("aria-label", title);
    node.dataset.sessionKey = "";
    return node;
  };
  const arrow = (path) => `<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${path}</svg>`;
  const keyButtons = [
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
    ["ctrlz", "^Z", "Suspend"]
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
    ["A+", 1]
  ]) {
    const node = keyButton(label, delta < 0 ? "Smaller text" : "Larger text");
    delete node.dataset.sessionKey;
    node.dataset.font = String(delta);
    keyScroll.append(node);
  }
  const sessionKeys = [...box.querySelectorAll("[data-session-key]")];
  const setSessionControls = (enabled) => {
    for (const control of sessionControls)
      control.disabled = !enabled;
    for (const control of sessionKeys)
      control.disabled = !enabled;
    setInputEnabled(enabled);
    if (!enabled)
      clearModifiers();
    updateKeyboard();
  };
  setSessionControls(false);
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "term-status bad" : "term-status";
  };
  const render = () => {
    screen.textContent = output + draft;
    screen.scrollTop = screen.scrollHeight;
  };
  const append = (bytes) => {
    const text = decoder.decode(Uint8Array.from(bytes)).replace(/\r\n?/g, `
`);
    if (pendingEcho && text === pendingEcho) {
      pendingEcho = "";
      return;
    }
    output += text;
    render();
  };
  async function submitLine() {
    if (!session)
      return;
    const line = draft;
    draft = "";
    input.textContent = "";
    if (line.trim())
      history.push(line);
    historyIndex = history.length;
    output += `${line}
`;
    render();
    pendingEcho = `${line}
`;
    try {
      await writeText(`${line}\r`);
    } catch (error) {
      pendingEcho = "";
      setStatus(String(error), true);
    }
    focusInput();
  }
  async function writeText(value) {
    if (!session)
      return;
    await call(context, "/write", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, data: encodeText(value) })
    });
  }
  const historyStep = (direction) => {
    if (history.length === 0)
      return;
    historyIndex = direction === "up" ? Math.max(0, historyIndex - 1) : Math.min(history.length, historyIndex + 1);
    draft = history[historyIndex] ?? "";
    render();
  };
  async function sendKey(key) {
    if (key === "ctrlc") {
      draft = "";
      input.textContent = "";
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
    if (!sequence)
      return;
    try {
      await writeText(sequence);
      clearModifiers();
    } catch (error) {
      setStatus(String(error), true);
    } finally {
      focusInput();
    }
  }
  async function sendSymbol(symbol) {
    if (!ctrlArmed && !altArmed) {
      draft += symbol;
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
    if (disposed || !session)
      return;
    try {
      const output = await call(context, `/read?id=${encodeURIComponent(session.id)}&seq=${cursor}&wait-ms=500`);
      if (disposed)
        return;
      if (output) {
        append(output.data);
        cursor = output.seq + 1;
        timer = setTimeout(poll, 20);
      } else {
        timer = setTimeout(poll, 200);
      }
    } catch (error) {
      if (disposed)
        return;
      const text = error instanceof Error ? error.message : String(error);
      if (text.startsWith("not-found:")) {
        session = null;
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
    if (!session)
      return;
    const cols = Math.max(40, Math.floor(screen.clientWidth / 7.6));
    const rows = Math.max(12, Math.floor(screen.clientHeight / 18));
    call(context, "/resize", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, cols, rows })
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
          method: "DELETE"
        }).catch(() => {});
      }
      output = "";
      pendingEcho = "";
      draft = "";
      input.textContent = "";
      render();
      cursor = 0;
    }
    setInputEnabled(false);
    setSessionControls(false);
    try {
      session = await call(context, "/open", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ shell: "/bin/sh", cols: 80, rows: 24 })
      });
      if (disposed)
        return;
      cursor = 0;
      setInputEnabled(true);
      setSessionControls(true);
      state.textContent = `session ${session.id}`;
      setStatus("Connected to the scripted fixture.");
      resize();
      poll();
      focusInput();
    } catch (error) {
      session = null;
      setSessionControls(false);
      state.textContent = "unavailable";
      setStatus(String(error), true);
    }
  }
  async function sendSignal(name) {
    if (!session)
      return;
    if (name === "INT") {
      draft = "";
      input.textContent = "";
      render();
    }
    try {
      await call(context, "/signal", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: session.id, signal: name })
      });
      setStatus(`${name} sent to ${session.id}`);
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  const consumeInput = () => {
    const text = input.textContent ?? "";
    input.textContent = "";
    if (!text) {
      render();
      return;
    }
    draft += text;
    render();
  };
  input.addEventListener("input", () => {
    if (!composing)
      consumeInput();
  });
  input.addEventListener("beforeinput", (event) => {
    if (composing)
      return;
    if (event.inputType === "insertText" || event.inputType === "insertFromPaste") {
      if (event.data) {
        event.preventDefault();
        draft += event.data;
        render();
      }
      return;
    }
    if (event.inputType === "deleteContentBackward") {
      event.preventDefault();
      const characters = [...draft];
      characters.pop();
      draft = characters.join("");
      render();
      return;
    }
    if (event.inputType === "insertLineBreak" || event.inputType === "insertParagraph") {
      event.preventDefault();
      submitLine();
    }
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
    screen.classList.add("focused");
    updateKeyboard();
  });
  input.addEventListener("blur", () => {
    input.textContent = "";
    screen.classList.remove("focused");
    updateKeyboard();
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      submitLine();
      return;
    }
    if (event.key === "Backspace" && !composing) {
      event.preventDefault();
      const characters = [...draft];
      characters.pop();
      draft = characters.join("");
      render();
      return;
    }
    if ((ctrlArmed || altArmed) && event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
      event.preventDefault();
      const data = applyModifiers(event.key, ctrlArmed, altArmed);
      clearModifiers();
      if (data === "\x03") {
        draft = "";
        input.textContent = "";
        render();
        sendSignal("INT");
        return;
      }
      writeText(data).catch((error) => setStatus(String(error), true));
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown")
      return;
    if (history.length === 0)
      return;
    event.preventDefault();
    historyStep(event.key === "ArrowUp" ? "up" : "down");
  });
  for (const control of box.querySelectorAll("[data-signal]")) {
    control.addEventListener("click", () => {
      sendSignal(control.dataset.signal);
    });
  }
  box.querySelector("[data-clear]").addEventListener("click", () => {
    output = "";
    pendingEcho = "";
    draft = "";
    input.textContent = "";
    render();
    setStatus("Screen cleared");
  });
  keyBar.addEventListener("pointerdown", (event) => {
    if (event.target.closest("button"))
      event.preventDefault();
  });
  screen.addEventListener("pointerdown", (event) => {
    if (event.pointerType === "mouse")
      focusInput();
  });
  keyBar.addEventListener("click", (event) => {
    const target = event.target.closest("button");
    if (!target || target.disabled)
      return;
    if (target.hasAttribute("data-keyboard")) {
      toggleKeyboard();
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
    if (target.dataset.enter === "true") {
      submitLine();
      return;
    }
    const key = target.dataset.key;
    if (key) {
      sendKey(key);
      return;
    }
    const symbol = target.dataset.symbol;
    if (symbol) {
      sendSymbol(symbol);
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
    setFontSize
  });
  const onViewport = () => {
    if (document.activeElement === input) {
      input.scrollIntoView({ block: "nearest" });
    }
  };
  window.visualViewport?.addEventListener("resize", onViewport);
  const onResize = () => resize();
  window.addEventListener("resize", onResize);
  box.querySelector("[data-reconnect]").addEventListener("click", () => {
    open(true);
  });
  updateModifiers();
  open();
  return () => {
    disposed = true;
    if (timer)
      clearTimeout(timer);
    unbindGestures();
    window.visualViewport?.removeEventListener("resize", onViewport);
    window.removeEventListener("resize", onResize);
    if (session) {
      call(context, `/session?id=${encodeURIComponent(session.id)}`, {
        method: "DELETE"
      }).catch(() => {});
    }
    root.replaceChildren();
  };
}
export {
  mount
};
