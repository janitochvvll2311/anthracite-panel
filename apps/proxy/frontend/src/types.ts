/// The app/fetch subset shared by a view and a modal frame.
export type Caller = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
};

export type FrameContext = Caller & {
  fill?: () => void;
  panel?: { slots: string[] };
  /// Parameters the panel passed when it opened this view (a modal handoff).
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

export type ProxyHeader = { name: string; value: string };

export type ProxyRoute = {
  id: string;
  match: string;
  upstream: string | null;
  "strip-prefix": boolean;
  websocket: boolean;
  redirect: string | null;
  headers: ProxyHeader[];
  "body-limit": number | null;
};

export type ProxyHost = {
  id: string;
  names: string[];
  certificate: string | null;
  acme: boolean;
  "redirect-https": boolean;
  hsts: boolean;
  routes: ProxyRoute[];
};

export type ProxyUpstream = {
  id: string;
  url: string;
  "connect-timeout-ms": number;
  "read-timeout-ms": number;
  "tls-verify": boolean;
};

export type ProxyListener = {
  id: string;
  bind: string;
  tls: boolean;
  "default-host": string | null;
  forwarded: boolean;
};

export type ProxyConfig = {
  version: number;
  listeners: ProxyListener[];
  upstreams: ProxyUpstream[];
  hosts: ProxyHost[];
};

export type ProxyStatus = {
  running: boolean;
  listeners: string[];
  "last-reload": number;
  "last-error": string | null;
  version: number;
  dropped: number;
  "handshake-errors": number;
};

export type AccessEvent = {
  at: number;
  host: string;
  method: string;
  path: string;
  status: number;
  bytes: number;
  "duration-ms": number;
  upstream: string;
};

export type UpstreamCheck = {
  id: string;
  ok: boolean;
  status: number | null;
  "latency-ms": number;
  error: string | null;
};
