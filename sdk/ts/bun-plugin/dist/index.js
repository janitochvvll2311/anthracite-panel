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
        contents: 'export * from "@anth-panel/client";',
        loader: "js"
      }));
    }
  };
}
var src_default = anthPlugin;
export {
  anthPlugin,
  src_default as default
};
