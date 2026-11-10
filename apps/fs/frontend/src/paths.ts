export function join(path: string, name: string): string {
  return path === "/" ? `/${name}` : `${path}/${name}`;
}

export function parent(path: string): string {
  if (path === "/") return "/";
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return `/${parts.join("/")}`;
}

export function baseName(path: string): string {
  return path === "/" ? "/" : path.substring(path.lastIndexOf("/") + 1);
}

export function normalizePath(input: string): string | null {
  let path = String(input).trim();
  if (!path) path = "/";
  if (!path.startsWith("/")) path = `/${path}`;
  const parts = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!parts.length) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return `/${parts.join("/")}`;
}
