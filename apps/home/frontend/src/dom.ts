import { percent } from "./format";

export function span(text: string, className?: string): HTMLSpanElement {
  const node = document.createElement("span");
  if (className) node.className = className;
  node.textContent = text;
  return node;
}

export function rows(
  target: HTMLElement,
  pairs: Array<[string, string]>,
): void {
  target.replaceChildren();
  for (const [label, value] of pairs) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    target.append(term, detail);
  }
}

export function meter(
  used: number,
  total: number,
  warm = false,
): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = warm ? "home-bar warm" : "home-bar";
  const fill = document.createElement("span");
  fill.style.width = `${percent(used, total)}%`;
  wrap.append(fill);
  return wrap;
}

export function empty(text: string): HTMLParagraphElement {
  const node = document.createElement("p");
  node.className = "home-empty";
  node.textContent = text;
  return node;
}

export function query<T extends HTMLElement>(
  root: HTMLElement,
  selector: string,
): T | null {
  return root.querySelector<T>(selector);
}
