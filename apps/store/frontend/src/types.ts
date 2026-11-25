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

export type Source = {
  id: string;
  name: string;
  kind: string;
  url: string;
  trusted: boolean;
  "refreshed-at": number;
};

export type Summary = {
  id: string;
  name: string;
  summary: string;
  publisher: string;
  categories: string[];
  latest: string;
};

export type Version = {
  version: string;
  channel: string;
  "published-at": number;
  sha256: string;
  size: number;
  "bundle-url": string;
  signer: string;
  yanked: boolean;
};

export type AppDetail = {
  id: string;
  name: string;
  summary: string;
  description: string;
  publisher: string;
  license: string;
  homepage: string | null;
  categories: string[];
  versions: Version[];
};
