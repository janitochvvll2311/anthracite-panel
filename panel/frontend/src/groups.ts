import { contributionKey, type Contribution } from "@anthracite/panel";

/// One top-level tab: the parent view plus the child views grouped under it.
export type ViewGroup = {
  key: string;
  parent: Contribution;
  children: Contribution[];
};

/// A parent reference is `<app-id>#<slot-tag>`.
export function parseParent(
  reference: string,
): { app: string; tag: string } | null {
  const index = reference.lastIndexOf("#");
  if (index <= 0 || index === reference.length - 1) return null;
  return {
    app: reference.slice(0, index),
    tag: reference.slice(index + 1),
  };
}

/// Groups view contributions in one level. A missing, hidden or nested parent
/// leaves the child as a top-level tab; the relative order of the input is the
/// panel's deterministic order.
export function groupViews(views: Contribution[]): ViewGroup[] {
  const byReference = new Map(
    views.map((view) => [`${view.app.id}#${view.slot.tag}`, view]),
  );
  const childParent = new Map<string, Contribution>();

  for (const view of views) {
    const reference = view.slot.parent;
    if (!reference) continue;
    const parsed = parseParent(reference);
    if (!parsed) continue;
    const parent = byReference.get(`${parsed.app}#${parsed.tag}`);
    if (
      !parent ||
      parent === view ||
      parent.slot.hidden ||
      parent.slot.parent
    ) {
      continue;
    }
    childParent.set(contributionKey(view), parent);
  }

  const groups = new Map<string, ViewGroup>();
  const ordered: ViewGroup[] = [];
  const root = (view: Contribution): ViewGroup => {
    const key = contributionKey(view);
    const existing = groups.get(key);
    if (existing) return existing;
    const group = { key, parent: view, children: [view] };
    groups.set(key, group);
    ordered.push(group);
    return group;
  };

  for (const view of views) {
    if (childParent.has(contributionKey(view))) continue;
    root(view);
  }
  for (const view of views) {
    const parent = childParent.get(contributionKey(view));
    if (!parent) continue;
    groups.get(contributionKey(parent))?.children.push(view);
  }

  return ordered;
}

/// The group that contains a contribution, if the current surface still has
/// one (a parent may disappear between refreshes).
export function groupFor(
  groups: ViewGroup[],
  contribution: Contribution,
): ViewGroup | undefined {
  const key = contributionKey(contribution);
  return groups.find((group) => group.children.some((child) => contributionKey(child) === key));
}
