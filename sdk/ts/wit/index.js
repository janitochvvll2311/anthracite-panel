// The catalog directory inside the installed package. A build script copies the
// same WIT the Rust guest crate ships, so both SDKs use one format.
import { fileURLToPath } from "node:url";

export const catalog = fileURLToPath(new URL("./wit/deps/", import.meta.url));
