export type FrameSocketHandlers = {
  onOpen?: () => void;
  onMessage?: (data: ArrayBuffer) => void | Promise<void>;
  onText?: (text: string) => void | Promise<void>;
  /// `code` is the WebSocket close code: 1000/1005 is a clean end, 1011 and
  /// 1006 are failures.
  onClose?: (error?: string, code?: number) => void;
};

export type FrameSocket = {
  send: (data: ArrayBuffer) => void;
  close: () => void;
};

/// The subset of the panel frame context this app uses.
export type FrameContext = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  /// Present on panels that relay a WebSocket route. Without it the app falls
  /// back to the chunked `/stream` route and HTTP writes.
  socket?: (
    route: string,
    handlers?: FrameSocketHandlers,
  ) => FrameSocket;
  fill?: () => void;
};
