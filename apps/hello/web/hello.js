// The hello app's panel module. One module serves the three portable slots;
// the context says which slot mounted it, so the app decides what to draw.

const escape = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[character],
  );

const styles = `
  .hello { max-width: 640px; display: grid; gap: 16px; }
  .hello-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .hello h3 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -0.01em; }
  .hello p { margin: 0; color: #8f98ab; font-size: 12.5px; }
  .hello-badge {
    padding: 3px 9px; border-radius: 999px; font-size: 11px; text-transform: uppercase;
    letter-spacing: .08em; color: #9d7bff; background: #9d7bff18; border: 1px solid #9d7bff3d;
  }
  .hello-form { display: flex; gap: 8px; }
  .hello input {
    flex: 1; min-width: 0; padding: 9px 11px; border-radius: 8px;
    border: 1px solid #2a3142; background: #0f1219; color: #e8ebf2; font: inherit;
  }
  .hello input:focus { outline: 2px solid #7aa2ff66; outline-offset: 1px; border-color: #3a4560; }
  .hello button {
    padding: 9px 14px; border-radius: 8px; border: 1px solid #3a4560; cursor: pointer;
    background: linear-gradient(180deg, #2a3550, #222b3f); color: #e8ebf2; font: inherit;
  }
  .hello button:hover { border-color: #4c5c80; }
  .hello-output {
    min-height: 22px; padding: 10px 12px; border-radius: 8px; background: #0f121900;
    border: 1px dashed #2a3142; color: #b9c3d6 !important; font-family: ui-monospace, monospace;
    font-size: 12.5px !important; white-space: pre-wrap; overflow-wrap: anywhere;
  }
  .hello-output.ok { border-style: solid; border-color: #52d1a44d; color: #9fe6cb !important; }
  .hello-output.bad { border-style: solid; border-color: #ff7d8a4d; color: #ffb3ba !important; }
  .hello-compact { gap: 10px; }
  .hello-compact h3 { font-size: 15px; }
  .hello-settings dl {
    display: grid; grid-template-columns: max-content 1fr; gap: 6px 16px; margin: 0;
    font-size: 12.5px;
  }
  .hello-settings dt { color: #667087; }
  .hello-settings dd { margin: 0; color: #c8d0df; overflow-wrap: anywhere; }
`;

function shell(context, compact) {
  const box = document.createElement("div");
  box.className = compact ? "hello hello-compact" : "hello";
  box.innerHTML = `
    <div class="hello-head">
      <div>
        <h3>Hello</h3>
        <p>${escape(context.app.id)} · v${escape(context.app.version)}</p>
      </div>
      <span class="hello-badge">${escape(context.app.risk)}</span>
    </div>
    <form class="hello-form">
      <input name="name" value="Ada" aria-label="Name" autocomplete="off" />
      <button type="submit">Greet</button>
    </form>
    <p class="hello-output" role="status">Ask the app through its own route.</p>
  `;

  const form = box.querySelector("form");
  const input = box.querySelector("input");
  const output = box.querySelector(".hello-output");

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const name = input.value.trim() || "world";
    output.className = "hello-output";
    output.textContent = "Calling the app route…";
    try {
      const response = await fetch(
        `/api/apps/${encodeURIComponent(context.app.id)}/hello?name=${encodeURIComponent(name)}`,
        { headers: { accept: "application/json" } },
      );
      const text = await response.text();
      let value = text;
      try {
        const parsed = JSON.parse(text);
        value = typeof parsed === "string" ? parsed : JSON.stringify(parsed);
      } catch {
        // The route did not answer JSON; show the body as it arrived.
      }
      output.textContent = value;
      output.className = `hello-output ${response.ok ? "ok" : "bad"}`;
    } catch (error) {
      output.textContent = String(error);
      output.className = "hello-output bad";
    }
  });

  if (compact) input.focus();
  return box;
}

function settings(context) {
  const box = document.createElement("div");
  box.className = "hello hello-settings";
  box.innerHTML = `
    <div class="hello-head">
      <div>
        <h3>Hello settings</h3>
        <p>Mounted from the ${escape(context.contribution.id)} slot.</p>
      </div>
      <span class="hello-badge">${escape(context.app.risk)}</span>
    </div>
    <dl>
      <dt>App</dt><dd>${escape(context.app.id)}</dd>
      <dt>Version</dt><dd>${escape(context.app.version)}</dd>
      <dt>Module</dt><dd>${escape(context.contribution.module)}</dd>
      <dt>Hash</dt><dd>${escape(context.contribution.sha256.slice(0, 24))}…</dd>
      <dt>Panel</dt><dd>${escape(context.panel.version)} · ${context.panel.slots.map(escape).join(", ")}</dd>
    </dl>
  `;
  return box;
}

export function mount(root, context) {
  const style = document.createElement("style");
  style.textContent = styles;
  root.append(style);

  if (context.contribution.id === "panel.settings") {
    root.append(settings(context));
  } else {
    root.append(shell(context, context.contribution.id === "panel.action"));
  }

  return () => {
    root.replaceChildren();
  };
}
