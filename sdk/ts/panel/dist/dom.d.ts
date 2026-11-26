import type { FrameContext } from "./iframe.js";
import type { Contribution } from "./model.js";
export declare function byId<T extends HTMLElement>(id: string): T;
export declare function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K];
export declare function button(className: string, text: string, title?: string): HTMLButtonElement;
export declare function safeDispose(dispose: () => void): void;
export type ErrorCardOptions = {
    className?: string;
    title?: string;
    detailTag?: "p" | "span";
};
export declare function errorCard(error: unknown, { className, title, detailTag, }?: ErrorCardOptions): HTMLElement;
export declare function frameContext(contribution: Contribution, kind: FrameContext["kind"], panel: {
    slots: readonly string[];
    version: string;
    viewport?: string;
}, params?: unknown): FrameContext;
