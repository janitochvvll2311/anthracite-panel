// The sandboxed entry point: the panel loads this module for both the Files
// and the Editor slots, with the same exports as before the split.

export { mount } from "./files";
export { beforeClose, onParams, snapshot } from "./editor";
