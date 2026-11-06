export function mount(root, ctx) {
  root.textContent = `Hello from ${ctx.app.id}`;
  return () => {
    root.textContent = "";
  };
}
