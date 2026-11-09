// The WIT shapes `wasm-tools component wit --json` produces, narrowed to the
// parts the generator reads.

export type TypeRef = string | number;

export type WitType = { name: string | null; kind: any; owner: any };

export type WitFunction = {
  name: string;
  kind: string;
  params: { name: string; type: TypeRef }[];
  result?: TypeRef;
};

export type WitInterface = {
  name: string | null;
  types: Record<string, TypeRef>;
  functions: Record<string, WitFunction>;
  package: number;
};

export type WitDoc = {
  packages: { name: string; interfaces: Record<string, number> }[];
  interfaces: WitInterface[];
  types: WitType[];
};

export type Generation = {
  files: Record<string, string>;
};
