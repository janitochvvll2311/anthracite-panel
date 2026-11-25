/// The app/fetch subset shared by the view and the modal.
export type Caller = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
};

export type FrameContext = Caller & {
  fill?: () => void;
  panel?: { slots: string[] };
  params?: unknown;
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
  onAction?: (handler: (tag: string, payload?: unknown) => void) => () => void;
  openView?: (request: {
    tag: string;
    params?: unknown;
    mode?: "tab" | "float";
  }) => Promise<boolean>;
};

export type ModalContext = Caller & {
  contribution: { tag: string };
  params?: unknown;
  openView?: (request: {
    tag: string;
    params?: unknown;
    mode?: "tab" | "float";
  }) => Promise<boolean>;
  close: () => void;
};

export type Account = {
  id: string;
  directory: string;
  contact: string[];
  status: string;
  "created-at": number;
  "key-type": string;
};

export type OrderStatus =
  | "pending"
  | "ready"
  | "processing"
  | "valid"
  | "invalid"
  | "revoked"
  | "issued-unstored";

export type Order = {
  id: string;
  account: string;
  names: string[];
  challenge: string;
  status: OrderStatus;
  "expires-at": number;
  certificate: string | null;
  "last-error": string | null;
};

export type OrderEvent = {
  order: string;
  kind: string;
  at: number;
  detail: string | null;
};

export type Capability = {
  challenge: string;
  solvers: string[];
  directories: string[];
  "staging-directories": string[];
};
