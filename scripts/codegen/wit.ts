import { execFileSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { join } from "node:path";

import type { TypeRef, WitDoc, WitInterface, WitType } from "./model";
import { fail, scratch, wasmTools } from "./util";

export function witType(doc: WitDoc, ref: TypeRef): WitType {
  if (typeof ref !== "number") fail(`expected a type id, got \`${ref}\``);
  const type = doc.types[ref];
  if (!type) fail(`type id ${ref} is missing`);
  return type;
}

export function packageName(doc: WitDoc, index: number): string {
  return doc.packages[index]?.name ?? fail(`package ${index} is missing`);
}

export function splitPackage(name: string): { base: string; version: string } {
  const at = name.lastIndexOf("@");
  if (at < 0) return { base: name, version: "0.0.0" };
  return { base: name.slice(0, at), version: name.slice(at + 1) };
}

export function interfaceId(doc: WitDoc, iface: WitInterface): string {
  const pkg = splitPackage(packageName(doc, iface.package));
  return `${pkg.base}/${iface.name}@${pkg.version}`;
}

export function parseWit(app: string, deps: string): WitDoc {
  if (!existsSync(deps)) fail(`the guest WIT catalog is missing at ${deps}`);
  mkdirSync(scratch, { recursive: true });
  const tmp = mkdtempSync(join(scratch, "codegen-"));
  try {
    mkdirSync(join(tmp, "deps"), { recursive: true });
    for (const file of readdirSync(join(app, "wit"))) {
      if (file.endsWith(".wit")) {
        cpSync(join(app, "wit", file), join(tmp, file));
      }
    }
    cpSync(deps, join(tmp, "deps"), { recursive: true });
    const json = execFileSync(
      wasmTools,
      ["component", "wit", tmp, "--json"],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    );
    return JSON.parse(json) as WitDoc;
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

export function primitive(ref: string): boolean {
  return new Set([
    "string", "bool", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64",
    "f32", "f64", "bytes",
  ]).has(ref);
}

export function tsPrimitive(ref: string): string {
  switch (ref) {
    case "string":
      return "string";
    case "bool":
      return "boolean";
    case "u8":
    case "u16":
    case "u32":
    case "u64":
    case "s8":
    case "s16":
    case "s32":
    case "s64":
    case "f32":
    case "f64":
      return "number";
    case "bytes":
      return "Uint8Array";
    default:
      return fail(`unsupported WIT type \`${ref}\``);
  }
}

export function rustPrimitive(ref: string): string {
  if (primitive(ref)) {
    return ref === "string" ? "String" : ref === "bytes" ? "Vec<u8>" : ref;
  }
  return fail(`unsupported WIT type \`${ref}\``);
}

