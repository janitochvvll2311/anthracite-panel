import type { TypeRef, WitDoc, WitFunction, WitType } from "./model";
import { fail, pascal, token } from "./util";
import {
  packageName,
  primitive,
  rustPrimitive,
  splitPackage,
  tsPrimitive,
  witType,
} from "./wit";

/// The type graph for one WIT document: named records/enums plus the TS and
/// Rust mappings. Everything is a closure so the functions can call each other
/// without a `this` receiver.
export function createTypes(doc: WitDoc) {
  const named = new Map<string, WitType>();
  const tsTypeCache = new Map<number, string>();
  const rustTypeCache = new Map<number, string>();

  function typeKey(type: WitType): string {
    if (!type.name) fail("an anonymous type cannot be named");
    const owner = type.owner?.interface;
    const iface =
      typeof owner === "number" ? doc.interfaces[owner] : undefined;
    const pkg = iface
      ? splitPackage(packageName(doc, iface.package))
      : undefined;
    return `${pascal(token(pkg?.base ?? ""))}${iface?.name ? pascal(iface.name) : ""}${pascal(type.name)}`;
  }

  function ensureNamed(type: WitType): void {
    if (!type.name) return;
    const key = typeKey(type);
    if (named.has(key)) return;
    named.set(key, type);
    if (type.kind.record) {
      for (const field of type.kind.record.fields) collect(field.type);
    }
    // Enums have no payloads. Variants are not accepted in route signatures.
  }

  function collect(ref: TypeRef): void {
    if (typeof ref === "string") {
      if (!primitive(ref)) fail(`unsupported WIT type \`${ref}\``);
      return;
    }
    const type = witType(doc, ref);
    if (type.name) {
      ensureNamed(type);
      return;
    }
    const kind = type.kind;
    if (kind.list) collect(kind.list);
    else if (kind.option) collect(kind.option);
    else if (kind.type) collect(kind.type);
    else if (kind.result) {
      if (kind.result.ok !== null && kind.result.ok !== undefined) {
        collect(kind.result.ok);
      }
    } else {
      fail(`unsupported anonymous WIT type ${JSON.stringify(kind)}`);
    }
  }

  function returnRef(fn: WitFunction): TypeRef | null {
    if (fn.result === undefined || fn.result === null) return null;
    if (typeof fn.result === "number") {
      const type = witType(doc, fn.result);
      if (!type.name && type.kind.result) return type.kind.result.ok ?? null;
    }
    return fn.result;
  }

  function tsType(ref: TypeRef): string {
    if (typeof ref === "string") return tsPrimitive(ref);
    const type = witType(doc, ref);
    if (type.name) return `types.${typeKey(type)}`;
    if (tsTypeCache.has(ref)) return tsTypeCache.get(ref)!;
    const kind = type.kind;
    let value: string;
    if (kind.list) value = `${tsType(kind.list)}[]`;
    else if (kind.option) value = `${tsType(kind.option)} | null`;
    else if (kind.type) value = tsType(kind.type);
    else if (kind.result) {
      value = kind.result.ok === null || kind.result.ok === undefined
        ? "void"
        : tsType(kind.result.ok);
    } else {
      fail(`unsupported WIT type in a route signature: ${JSON.stringify(kind)}`);
    }
    tsTypeCache.set(ref, value);
    return value;
  }

  function rustType(ref: TypeRef): string {
    if (typeof ref === "string") return rustPrimitive(ref);
    const type = witType(doc, ref);
    if (type.name) return `crate::types::${typeKey(type)}`;
    if (rustTypeCache.has(ref)) return rustTypeCache.get(ref)!;
    const kind = type.kind;
    let value: string;
    if (kind.list) value = `Vec<${rustType(kind.list)}>`;
    else if (kind.option) value = `Option<${rustType(kind.option)}>`;
    else if (kind.type) value = rustType(kind.type);
    else if (kind.result) {
      value = kind.result.ok === null || kind.result.ok === undefined
        ? "()"
        : rustType(kind.result.ok);
    } else {
      fail(`unsupported WIT type in a route signature: ${JSON.stringify(kind)}`);
    }
    rustTypeCache.set(ref, value);
    return value;
  }

  function typeLabel(ref: TypeRef | null): string {
    if (ref === null) return "unit";
    if (typeof ref === "string") return ref;
    const type = witType(doc, ref);
    if (type.name) return typeKey(type);
    const kind = type.kind;
    if (kind.list) return `list<${typeLabel(kind.list)}>`;
    if (kind.option) return `option<${typeLabel(kind.option)}>`;
    if (kind.type) return typeLabel(kind.type);
    if (kind.result) return typeLabel(kind.result.ok ?? null);
    return "unknown";
  }

  function isOption(ref: TypeRef): boolean {
    if (typeof ref !== "number") return false;
    const type = witType(doc, ref);
    return !type.name && Boolean(type.kind.option);
  }

  function inner(ref: TypeRef): TypeRef {
    if (typeof ref !== "number") return ref;
    const type = witType(doc, ref);
    return type.kind.option ?? ref;
  }

  function isRecord(ref: TypeRef): boolean {
    if (typeof ref !== "number") return false;
    const type = witType(doc, ref);
    if (type.name) return Boolean(type.kind.record);
    return false;
  }

  function tsParamType(ref: TypeRef): string {
    const value = isOption(ref) ? inner(ref) : ref;
    return tsType(value);
  }

  function rustParamType(ref: TypeRef): string {
    const value = isOption(ref) ? inner(ref) : ref;
    return isOption(ref) ? `Option<${rustType(value)}>` : rustType(value);
  }

  return {
    named,
    typeKey,
    collect,
    returnRef,
    tsType,
    rustType,
    typeLabel,
    isOption,
    inner,
    isRecord,
    tsParamType,
    rustParamType,
  };
}

export type Types = ReturnType<typeof createTypes>;
