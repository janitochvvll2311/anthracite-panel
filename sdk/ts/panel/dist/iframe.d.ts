export type FrameContext = {
    app: {
        id: string;
        version: string;
        risk: string;
    };
    contribution: {
        id: string;
        label: string;
        icon?: string;
        order: number;
        tag: string;
        module: string;
        sha256: string;
        app: string;
    };
    panel: {
        slots: string[];
        version: string;
        viewport?: string;
    };
    kind: "view" | "action" | "settings" | "modal";
    params?: unknown;
};
export type OpenViewRequest = {
    tag: string;
    params?: unknown;
    mode?: "tab" | "float";
    size?: {
        width?: number;
        height?: number;
    };
};
export type OpenModalRequest = {
    tag: string;
    params?: unknown;
};
export type FrameHandle = {
    update: (params: unknown) => void;
    requestClose: () => Promise<boolean>;
    snapshot: () => Promise<unknown>;
    setVisible: (visible: boolean) => void;
    setViewport: (viewport: string) => void;
    action: (tag: string, payload?: unknown) => void;
    focus: () => void;
};
export type FrameOptions = {
    source: string;
    context: FrameContext;
    close?: () => void;
    openView?: (request: OpenViewRequest) => boolean | void;
    openModal?: (request: OpenModalRequest) => boolean | void;
    onParams?: (params: unknown) => void;
    onReady?: (frame: FrameHandle) => void;
    request?: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;
    frameUrl?: string;
};
export declare function mountFrame(host: HTMLElement, options: FrameOptions): Promise<() => void>;
