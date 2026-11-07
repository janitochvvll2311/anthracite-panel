// The second panel's whole opinion about the surface: it mounts panel.view and
// reports everything else as ignored. Keeping that policy here makes it
// testable without a DOM.

import {
  VIEW,
  ignored,
  ordered,
  type App,
  type Contribution,
} from "@anthracite/panel";

export function visibleViews(apps: App[]): Contribution[] {
  return ordered(apps, VIEW);
}

export function ignoredViews(apps: App[]): Contribution[] {
  return ignored(apps, [VIEW]);
}
