/// The subset of the panel frame context this app uses.
export type Caller = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
};

export type FrameContext = Caller & {
  contribution: { tag: string };
  fill?: () => void;
  params?: any;
  updateParams?: (params: any) => void;
  openView?: (view: any) => void;
  panel?: { slots: string[] };
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
  onAction?: (handler: (tag: string) => void) => () => void;
  openModal?: (request: { tag: string; params?: unknown }) => Promise<boolean>;
};

/// The context a `panel.modal` module receives.
export type ModalContext = Caller & {
  params?: any;
  contribution: { tag: string };
  close: () => void;
};
