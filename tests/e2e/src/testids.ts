/// The panel test-id vocabulary lives in the SDK next to the frame contract, so
/// the shell, the frame and the suite read one map. Import it by path rather
/// than through `node_modules`: Playwright transforms TypeScript it can see.
export { TESTID } from "../../../sdk/ts/panel/src/testids";
export type { TestId } from "../../../sdk/ts/panel/src/testids";
