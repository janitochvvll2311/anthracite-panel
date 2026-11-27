// src/main.ts
function mount(root, context) {
  root.innerHTML = "";
  const heading = document.createElement("h2");
  heading.textContent = "Sdk Bun";
  const line = document.createElement("p");
  line.textContent = `${context.app.id} · v${context.app.version}`;
  root.append(heading, line);
}
export {
  mount
};
