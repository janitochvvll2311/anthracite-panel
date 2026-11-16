/// The app/fetch subset shared by a view and a modal frame.
export type Caller = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
};

/// The subset of the panel frame context this view uses.
export type FrameContext = Caller & {
  fill?: () => void;
  panel?: { slots: string[] };
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
  onAction?: (handler: (tag: string) => void) => () => void;
  openModal?: (request: { tag: string; params?: unknown }) => Promise<boolean>;
};

/// The context a `panel.modal` module receives.
export type ModalContext = Caller & {
  params?: unknown;
  close: () => void;
};

export type Certificate = {
  id: string;
  names: string[];
  issuer: string;
  "not-before": number;
  "not-after": number;
  "key-type": string;
  source: string;
  "fingerprint-sha256": string;
  status: string;
  "auto-renew": boolean;
};
