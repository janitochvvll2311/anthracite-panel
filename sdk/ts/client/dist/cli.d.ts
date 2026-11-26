import type { Transport } from "./transport";
export declare function run(transport: Transport, verb: string, args: string[]): Promise<string>;
