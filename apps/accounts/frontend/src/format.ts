export function cell(className: string, text: string): HTMLSpanElement {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
}

export function table(headers: string[], entries: Node[], rowClass = ""): HTMLDivElement {
  const node = document.createElement("div");
  node.className = "acc-table";
  const head = document.createElement("div");
  head.className = `acc-row head${rowClass ? ` ${rowClass}` : ""}`;
  for (const label of headers) head.append(cell("", label));
  node.append(head);
  for (const entry of entries) node.append(entry);
  return node;
}
