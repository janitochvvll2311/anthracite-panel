import type { Transport } from "./transport.js";
export declare class GreetingHttpClient {
    private readonly request;
    private readonly base;
    constructor(request: Transport, base?: string);
    greet(name: string): Promise<string>;
}
