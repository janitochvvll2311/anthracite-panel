// @bun
// src/index.ts
function anthPlugin() {
  return {
    name: "anthracite",
    setup(build) {
      build.onResolve({ filter: /^anth:client$/ }, () => ({
        path: "anth:client",
        namespace: "anthracite"
      }));
      build.onLoad({ filter: /.*/, namespace: "anthracite" }, () => ({
        contents: 'export const virtual = true; export const generated = "anth:client";',
        loader: "js"
      }));
    }
  };
}
var src_default = anthPlugin;
var runtime = globalThis.Bun;
if (runtime?.plugin) {
  runtime.plugin(anthPlugin());
}
export {
  anthPlugin,
  src_default as default
};
