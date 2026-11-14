// The acid-test app's panel module: mounted by the panel, it asks the app's own
// route and renders the answer. It is plain DOM on purpose — an outside author
// should need nothing from this repository but the SDK.

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

export function mount(root, context) {
  const box = document.createElement("div");
  box.style.cssText = "display:grid;gap:12px;max-width:560px;";
  box.innerHTML = `
    <div>
      <h3 style="margin:0;font-size:18px;">Ping</h3>
      <p style="margin:0;color:#8f98ab;font-size:12.5px;">
        ${escape(context.app.id)} · v${escape(context.app.version)}
      </p>
    </div>
    <button type="button" style="justify-self:start;padding:8px 14px;border-radius:8px;
      border:1px solid #3a4560;background:#222b3f;color:#e8ebf2;cursor:pointer;">
      Ping the app
    </button>
    <p role="status" style="margin:0;font-family:ui-monospace,monospace;font-size:12.5px;
      color:#b9c3d6;">Not called yet.</p>
  `;

  const output = box.querySelector("p[role=status]");
  box.querySelector("button").addEventListener("click", async () => {
    output.textContent = "Calling…";
    try {
      const response = await context.fetch(
        `/api/apps/${encodeURIComponent(context.app.id)}/ping`,
        { headers: { accept: "application/json" } },
      );
      const text = await response.text();
      let value = text;
      try {
        const parsed = JSON.parse(text);
        value = typeof parsed === "string" ? parsed : JSON.stringify(parsed);
      } catch {
        // The route answered something that is not JSON; show it as it arrived.
      }
      output.textContent = response.ok ? value : `${response.status}: ${value}`;
    } catch (error) {
      output.textContent = String(error);
    }
  });

  root.append(box);
  return () => {
    root.replaceChildren();
  };
}
