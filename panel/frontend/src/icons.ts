// FAB and sidebar icons. The manifest carries a name from this set, or a
// `web/` asset path paired with a hash. Names never become markup: only this
// trusted table does.

type IconOptions = {
  appId: string;
  icon?: string;
  icon_sha256?: string;
  fallback: string;
};

const SVG = (body: string): string =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

const ICONS: Record<string, string> = {
  home: SVG('<path d="M2.5 7.2 8 2.5l5.5 4.7"/><path d="M4 6.5v6.8h8V6.5"/><path d="M6.5 13.3V9.5h3v3.8"/>'),
  folder: SVG('<path d="M2 4.5h4l1.3 1.5H14v6.5H2z"/><path d="M2 6h12"/>'),
  pen: SVG('<path d="m3 13 1.2-3.2 6.6-6.6 2.2 2.2-6.6 6.6z"/><path d="m9.8 4.2 2.2 2.2"/>'),
  terminal: SVG('<rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="m5 6 2 2-2 2"/><path d="M9 10h2.5"/>'),
  activity: SVG('<path d="M1.5 8h3l1.7-4 3 8 1.8-4h3.5"/>'),
  network: SVG('<circle cx="8" cy="3.5" r="1.5"/><circle cx="3.5" cy="12" r="1.5"/><circle cx="12.5" cy="12" r="1.5"/><path d="M8 5v3m0 0-3.5 2.5M8 8l3.5 2.5"/>'),
  disk: SVG('<ellipse cx="8" cy="4" rx="4.5" ry="1.8"/><path d="M3.5 4v8c0 1 2 1.8 4.5 1.8s4.5-.8 4.5-1.8V4"/><path d="M3.5 8c0 1 2 1.8 4.5 1.8s4.5-.8 4.5-1.8"/>'),
  users: SVG('<circle cx="6" cy="5" r="2"/><path d="M2.5 13c0-2 1.6-3.2 3.5-3.2S9.5 11 9.5 13"/><path d="M10 3.5a2 2 0 1 1 0 3.8M11.5 9.8c1.2.4 2 1.4 2 3.2"/>'),
  user: SVG('<circle cx="8" cy="5" r="2.2"/><path d="M3.5 13c0-2.2 2-3.6 4.5-3.6s4.5 1.4 4.5 3.6"/>'),
  group: SVG('<circle cx="5.5" cy="5.5" r="1.8"/><circle cx="10.5" cy="5.5" r="1.8"/><path d="M2.5 13c0-1.8 1.4-3 3-3s3 1.2 3 3M7.5 13c0-1.8 1.4-3 3-3s3 1.2 3 3"/>'),
  shield: SVG('<path d="M8 2.2 13 4v4c0 3-2.1 4.8-5 5.8C5.1 12.8 3 11 3 8V4z"/><path d="m5.8 8 1.5 1.5L10.5 6"/>'),
  refresh: SVG('<path d="M13 8a5 5 0 1 1-1.6-3.7"/><path d="M13 3v3h-3"/>'),
  plus: SVG('<path d="M8 3v10M3 8h10"/>'),
  upload: SVG('<path d="M8 13V3M4.5 6.5 8 3l3.5 3.5"/><path d="M3 13h10"/>'),
  download: SVG('<path d="M8 3v10M4.5 9.5 8 13l3.5-3.5"/><path d="M3 13h10"/>'),
  gear: SVG('<circle cx="8" cy="8" r="2"/><path d="M8 2.2v1.2M8 12.6v1.2M2.2 8h1.2M12.6 8h1.2M3.9 3.9l.9.9M11.2 11.2l.9.9M12.1 3.9l-.9.9M4.8 11.2l-.9.9"/>'),
  bolt: SVG('<path d="M9 2 4 9h3l-1 5 5-7H8z"/>'),
  hello: SVG('<path d="M8 2.5 9.3 6 13 7.3 9.3 8.6 8 12l-1.3-3.4L3 7.3 6.7 6z"/>'),
  file: SVG('<path d="M4 2.5h5l3 3v8H4z"/><path d="M9 2.5v3h3"/>'),
  link: SVG('<path d="M6.5 9.5 9.5 6.5"/><path d="M7.2 5.2 8.4 4a2.7 2.7 0 0 1 3.8 3.8l-1.2 1.2M8.8 10.8 7.6 12a2.7 2.7 0 0 1-3.8-3.8l1.2-1.2"/>'),
};

export function iconSvg(name: string | undefined): string | undefined {
  const key = name?.trim();
  return key ? ICONS[key] : undefined;
}

export function iconInitial(
  icon: string | undefined,
  fallback: string,
): string {
  const source = icon?.trim() || fallback.trim();
  return [...source][0] ?? "•";
}

/// The engine's asset route, with the hash as the cache key. Icon names are not
/// assets and never reach this path.
export function iconAssetUrl(appId: string, path: string, sha256: string): string {
  const segments = path.split("/").map(encodeURIComponent).join("/");
  return `/api/apps/${encodeURIComponent(appId)}/assets/${segments}?v=${sha256.slice(0, 16)}`;
}

export function renderIcon(node: HTMLElement, options: IconOptions): void {
  node.replaceChildren();
  node.classList.remove("icon-asset");
  node.removeAttribute("style");
  node.removeAttribute("role");
  const icon = options.icon?.trim();
  if (icon && options.icon_sha256) {
    node.classList.add("icon-asset");
    const url = `url("${iconAssetUrl(options.appId, icon, options.icon_sha256)}")`;
    node.style.setProperty("mask-image", url);
    node.style.setProperty("-webkit-mask-image", url);
    node.setAttribute("role", "img");
    node.setAttribute("aria-label", options.fallback);
    return;
  }
  const svg = iconSvg(icon);
  if (svg) {
    node.innerHTML = svg;
    node.setAttribute("role", "img");
    node.setAttribute("aria-label", options.fallback);
    return;
  }
  node.textContent = iconInitial(icon, options.fallback);
}
