/** The anchor this plugin's modules read. Must match the `[[api]]` block. */
export const ANCHOR = "/greeting";

/** The payload that anchor answers. */
export interface Greeting {
  message: string;
}
