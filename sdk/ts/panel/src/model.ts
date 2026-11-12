// The contribution shapes the panel contract exchanges. Kept separate from
// `index.ts` so `dom.ts` can type its helpers without a runtime/type cycle.

/// One slot contribution, as `GET /api/engine/composition` reports it.
export type FabAction = {
  label: string;
  icon?: string;
  tag: string;
  target?: string;
  action?: string;
};

export type Slot = {
  id: string;
  label: string;
  icon?: string;
  order: number;
  /// Hidden contributions can be opened by other views, but not listed.
  hidden?: boolean;
  /// `<app-id>#<slot-tag>` of the view this contribution is grouped under.
  parent?: string;
  /// Sub-tab label when this contribution is the parent of a group.
  tab?: string;
  /// View tag this FAB is scoped to.
  view?: string;
  /// Actions of a declarative `panel.fab`.
  actions?: FabAction[];
  tag: string;
  module: string;
  sha256: string;
};

/// The fields of a running app the panel reads.
export type App = {
  id: string;
  version: string;
  risk: string;
  slots: Slot[];
};

/// A slot with the app that owns it.
export type Contribution = {
  app: App;
  slot: Slot;
};
