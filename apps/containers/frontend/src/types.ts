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
  openModal?: (request: { tag: string; params?: unknown }) => Promise<boolean>;
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

export type Label = { name: string; value: string };

export type Container = {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  health: string | null;
  "created-at": number;
  ports: string[];
  labels: Label[];
};

export type Image = {
  id: string;
  tags: string[];
  size: number;
  "created-at": number;
  digest: string | null;
};

export type Volume = {
  name: string;
  driver: string;
  labels: Label[];
};

export type Network = {
  name: string;
  driver: string;
  subnet: string | null;
  containers: number;
};

export type Sample = {
  "cpu-percent": number;
  "memory-bytes": number;
  "memory-limit": number;
  "net-rx": number;
  "net-tx": number;
  pids: number;
};
