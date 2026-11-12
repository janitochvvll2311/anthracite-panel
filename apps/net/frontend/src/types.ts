export type Address = {
  family: "ipv4" | "ipv6";
  address: string;
  prefix: number;
};

export type Interface = {
  name: string;
  up: boolean;
  mac: string;
  mtu: number;
  addresses: Address[];
  "rx-bytes": number;
  "tx-bytes": number;
};

export type Route = {
  family: "ipv4" | "ipv6";
  destination: string;
  gateway: string | null;
  device: string;
  metric: number;
};

export type Snapshot = {
  interfaces: Interface[];
  routes: Route[];
};

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
