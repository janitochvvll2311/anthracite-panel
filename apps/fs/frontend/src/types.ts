/// The subset of the panel frame context this app uses.
export type FrameContext = {
  app: { id: string };
  contribution: { tag: string };
  fetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
  fill?: () => void;
  params?: any;
  updateParams?: (params: any) => void;
  openView?: (view: any) => void;
};
