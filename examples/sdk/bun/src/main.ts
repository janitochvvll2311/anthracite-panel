// The panel module. It exports the `mount` entry point the panel calls.
export function mount(
  root: HTMLElement,
  context: { app: { id: string; version: string } },
): void {
  root.innerHTML = "";
  const heading = document.createElement("h2");
  heading.textContent = "Sdk Bun";
  const line = document.createElement("p");
  line.textContent = `${context.app.id} · v${context.app.version}`;
  root.append(heading, line);
}
