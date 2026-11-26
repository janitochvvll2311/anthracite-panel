/** An HTTP request adapter: UDS (HTTP over the socket), TCP, or fetch. */
export type Transport = (method: string, path: string, body?: string) => Promise<string>;
