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

/// The subset of the panel frame context this app uses.
export type FrameContext = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  fill?: () => void;
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
};
