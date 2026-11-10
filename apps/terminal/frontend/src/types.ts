/// The subset of the panel frame context this app uses.
export type FrameContext = {
  app: { id: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  fill?: () => void;
};
