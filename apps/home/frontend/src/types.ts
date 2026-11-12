export type Cpu = {
  model: string;
  cores: number;
  "usage-percent": number;
  "load-1": number;
  "load-5": number;
  "load-15": number;
};

export type Memory = {
  total: number;
  used: number;
  available: number;
  "swap-total": number;
  "swap-used": number;
};

export type Disk = {
  name: string;
  mount: string;
  "file-system": string;
  total: number;
  used: number;
  available: number;
};

export type NetworkInterface = {
  name: string;
  mac: string | null;
  addresses: string[];
  state: string;
  received: number;
  transmitted: number;
  "packets-received": number;
  "packets-transmitted": number;
  errors: number;
};

export type HostInfo = {
  hostname: string;
  os: string;
  kernel: string;
  arch: string;
  "uptime-seconds": number;
  cpu: Cpu;
  memory: Memory;
  disks: Disk[];
  networks: NetworkInterface[];
};

/// The subset of the panel frame context this view uses.
export type FrameContext = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  /// Slots the hosting panel implements; a panel without `panel.fab` keeps the
  /// toolbar refresh button.
  panel?: { slots: string[] };
  onVisibility?: (handler: (visible: boolean) => void) => () => void;
  /// Local action tags from the panel, e.g. `refresh` after a modal closes.
  onAction?: (handler: (tag: string) => void) => () => void;
};
