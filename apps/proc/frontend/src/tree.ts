export type Process = {
  pid: number;
  ppid: number;
  name: string;
  user: string;
  state: string;
  "cpu-percent": number;
  "memory-bytes": number;
  "start-ticks": number;
  [key: string]: unknown;
};

export type Filters = {
  pid?: string;
  ppid?: string;
  name?: string;
  user?: string;
  state?: string;
  /// One query across pid, ppid, name, user and state; the mobile bar uses it.
  query?: string;
};

/// Applies the column filters. In tree mode a match keeps its ancestors, so the
/// path that leads to it does not disappear.
export function filterProcesses(
  processes: Process[],
  filters: Filters,
  tree: boolean,
): Process[] {
  const has = (value: string | undefined): value is string =>
    Boolean(value && value.trim());
  const match = (value: string, needle: string): boolean =>
    value.toLowerCase().includes(needle.trim().toLowerCase());
  const matches = processes.filter((process) => {
    if (has(filters.pid) && !String(process.pid).includes(filters.pid.trim())) return false;
    if (has(filters.ppid) && !String(process.ppid).includes(filters.ppid.trim())) return false;
    if (has(filters.name) && !match(process.name, filters.name)) return false;
    if (has(filters.user) && !match(process.user, filters.user)) return false;
    if (has(filters.state) && !match(process.state, filters.state)) return false;
    if (
      has(filters.query) &&
      ![String(process.pid), String(process.ppid), process.name, process.user, process.state].some(
        (value) => match(value, filters.query!),
      )
    ) {
      return false;
    }
    return true;
  });
  if (!tree) return matches;

  const byPid = new Map(processes.map((process) => [process.pid, process]));
  const keep = new Set<number>();
  for (const found of matches) {
    let current: Process | undefined = found;
    while (current && !keep.has(current.pid)) {
      keep.add(current.pid);
      current = byPid.get(current.ppid);
    }
  }
  return processes.filter((process) => keep.has(process.pid));
}

export type TreeRow = {
  process: Process;
  depth: number;
  hasChildren: boolean;
  /// One slot per ancestor: true draws a continuous vertical line, false is a
  /// blank slot (the ancestor was its parent's last child).
  guides: boolean[];
  /// Whether this node is the last child of its parent; the renderer draws an
  /// elbow instead of a tee for it.
  isLast: boolean;
};

/// Flat order, or a parent-before-children tree. A missing parent (a page
/// boundary or a process that exited) is treated as a root, and a cycle cannot
/// loop forever.
export function orderProcesses(
  processes: Process[],
  tree: boolean,
  collapsed: ReadonlySet<number>,
  compareBy: (left: Process, right: Process) => number = (left, right) =>
    left.pid - right.pid,
): TreeRow[] {
  if (!tree) {
    return [...processes].sort(compareBy).map((process) => ({
      process,
      depth: 0,
      hasChildren: false,
      guides: [],
      isLast: true,
    }));
  }

  const byPid = new Map(processes.map((process) => [process.pid, process]));
  const children = new Map<number, Process[]>();
  const roots: Process[] = [];
  for (const process of processes) {
    const parent = byPid.get(process.ppid);
    if (parent && process.ppid !== process.pid) {
      const siblings = children.get(process.ppid) ?? [];
      siblings.push(process);
      children.set(process.ppid, siblings);
    } else {
      roots.push(process);
    }
  }

  const out: TreeRow[] = [];
  const seen = new Set<number>();
  const reachable = new Set<number>();
  const mark = (process: Process): void => {
    if (reachable.has(process.pid)) return;
    reachable.add(process.pid);
    for (const child of children.get(process.pid) ?? []) mark(child);
  };
  for (const root of roots) mark(root);

  const walk = (
    process: Process,
    depth: number,
    guides: boolean[],
    isLast: boolean,
  ): void => {
    if (seen.has(process.pid)) return;
    seen.add(process.pid);
    const nested = [...(children.get(process.pid) ?? [])].sort(compareBy);
    out.push({
      process,
      depth,
      hasChildren: nested.length > 0,
      guides,
      isLast,
    });
    if (collapsed.has(process.pid)) return;
    nested.forEach((child, index) => {
      walk(
        child,
        depth + 1,
        [...guides, !isLast],
        index === nested.length - 1,
      );
    });
  };

  roots
    .sort(compareBy)
    .forEach((root, index) => walk(root, 0, [], index === roots.length - 1));
  // A cycle with no root still has to appear; the first unseen member becomes
  // its representative and `seen` cuts the loop. Processes hidden by a
  // collapsed ancestor are `reachable`, so they are not re-added as roots.
  for (const process of processes) {
    if (!reachable.has(process.pid)) walk(process, 0, [], true);
  }
  return out;
}
