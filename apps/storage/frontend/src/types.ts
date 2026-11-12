/// The app/fetch subset shared by a view and a modal frame.
export type Caller = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
};

/// The subset of the panel frame context this view uses.
export type FrameContext = Caller & {
  fill?: () => void;
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
  onAction?: (handler: (tag: string) => void) => () => void;
};

/// The context a `panel.modal` module receives.
export type ModalContext = Caller & {
  params?: unknown;
  close: () => void;
};
