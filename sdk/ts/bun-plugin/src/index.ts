// A Bun build plugin for the developer workflow. Pass it to
// `Bun.build({ plugins: [anthPlugin()] })` to resolve the virtual `anth:client`
// module the generated app code imports.

export interface AnthBuild {
  onResolve(
    options: { filter: RegExp },
    callback: (args: { path: string }) => { path: string; namespace: string } | undefined,
  ): void;
  onLoad(
    options: { filter: RegExp; namespace: string },
    callback: (args: { path: string }) => { contents: string; loader: string } | undefined,
  ): void;
}

export function anthPlugin(): { name: string; setup(build: AnthBuild): void } {
  return {
    name: "anthracite",
    setup(build) {
      build.onResolve({ filter: /^anth:client$/ }, () => ({
        path: "anth:client",
        namespace: "anthracite",
      }));
      build.onLoad({ filter: /.*/, namespace: "anthracite" }, () => ({
        contents: 'export * from "@anth-panel/client";',
        loader: "js",
      }));
    },
  };
}

export default anthPlugin;
