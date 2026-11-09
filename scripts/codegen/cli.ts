import type { TypeRef, WitDoc, WitFunction } from "./model";
import { camel, fail, header, rustField, snake } from "./util";
import type { Types } from "./types";
import { interfaceId, rustPrimitive, witType } from "./wit";

export type ClientRef = { className: string; module: string };

/// The generated CLI layer: flag parsing plus a call into the typed HTTP client.
/// The client table comes from the interface emitter, so this module never
/// decides a module name or class name on its own.
export function createCli(
  doc: WitDoc,
  manifest: any,
  routes: Map<string, any>,
  types: Types,
  classByInterface: Map<string, ClientRef>,
) {
  const {
    collect,
    inner,
    isOption,
    returnRef,
    rustType,
    tsType,
    typeKey,
    typeLabel,
  } = types;

  const commands: any[] = [];
  const cliEntries: {
    verb: string;
    fn: WitFunction;
    route: any | undefined;
    client: { className: string; module: string } | undefined;
  }[] = [];
  for (const entry of manifest.cli ?? []) {
    const [ifaceId, functionName] = String(entry.call).split("#");
    const iface = doc.interfaces.find(
      (candidate) => interfaceId(doc, candidate) === ifaceId,
    );
    if (!iface) fail(`CLI call \`${entry.call}\` names an unknown interface`);
    const fn = iface.functions[functionName];
    if (!fn) fail(`CLI call \`${entry.call}\` names an unknown function`);
    for (const param of fn.params) collect(param.type);
    const route = routes.get(entry.call);
    commands.push({
      verb: entry.verb,
      method: route ? String(route.method).toUpperCase() : null,
      path: route ? route.path : null,
      interface: ifaceId,
      function: functionName,
      params: fn.params.map((param) => ({
        name: param.name,
        type: typeLabel(param.type),
        optional: isOption(param.type),
      })),
      returns: typeLabel(returnRef(fn)),
    });
    cliEntries.push({
      verb: entry.verb,
      fn,
      route,
      client: classByInterface.get(ifaceId),
    });
  }

  // -------------------------------------------------------------------------
  // CLI clients. They receive the same Transport as the HTTP clients; the
  // transport is the adapter (UDS, TCP, fetch, ...) and this module only parses
  // flags and calls the typed route methods.

  function cliBase(ref: TypeRef): TypeRef {
    return isOption(ref) ? inner(ref) : ref;
  }

  function cliList(ref: TypeRef): boolean {
    return typeof ref === "number" && Boolean(witType(doc, ref).kind?.list);
  }

  function cliBool(ref: TypeRef): boolean {
    return typeof ref === "string" && ref === "bool";
  }

  function tsParseValue(ref: TypeRef, expression: string): string {
    if (typeof ref === "string") {
      if (ref === "string") return expression;
      if (ref === "bool") return `(${expression} === "true" || ${expression} === "1")`;
      if (ref === "bytes") return `new TextEncoder().encode(${expression})`;
      if (["u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64"].includes(ref)) {
        return `Number(${expression})`;
      }
      fail(`unsupported CLI parameter type \`${ref}\``);
    }
    const type = witType(doc, ref);
    if (type.name) {
      if (type.kind.enum) return `(${expression} as types.${typeKey(type)})`;
      if (type.kind.record) {
        return `(JSON.parse(${expression}) as types.${typeKey(type)})`;
      }
      fail(`named type \`${type.name}\` cannot be parsed by the CLI`);
    }
    const kind = type.kind;
    if (kind.list) return `(JSON.parse(${expression}) as ${tsType(ref)})`;
    if (kind.option) return tsParseValue(kind.option, expression);
    if (kind.type) return tsParseValue(kind.type, expression);
    fail(`unsupported CLI parameter type: ${JSON.stringify(kind)}`);
  }

  function rustParseValue(ref: TypeRef, expression: string): string {
    if (typeof ref === "string") {
      if (ref === "string") return expression;
      if (ref === "bool") {
        return `${expression}.parse::<bool>().map_err(|_| crate::ClientError::new("expected a boolean"))?`;
      }
      if (ref === "bytes") return `${expression}.into_bytes()`;
      if (["u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64"].includes(ref)) {
        return `${expression}.parse::<${rustPrimitive(ref)}>().map_err(|_| crate::ClientError::new("expected a number"))?`;
      }
      fail(`unsupported CLI parameter type \`${ref}\``);
    }
    const type = witType(doc, ref);
    if (type.name) {
      if (type.kind.enum) {
        return `crate::types::${typeKey(type)}::parse(&${expression}).ok_or_else(|| crate::ClientError::new("invalid enum value"))?`;
      }
      if (type.kind.record) {
        return `serde_json::from_str::<crate::types::${typeKey(type)}>(&${expression}).map_err(|error| crate::ClientError::new(format!("invalid JSON: {error}")))?`;
      }
      fail(`named type \`${type.name}\` cannot be parsed by the CLI`);
    }
    const kind = type.kind;
    if (kind.list) {
      return `serde_json::from_str::<${rustType(ref)}>(&${expression}).map_err(|error| crate::ClientError::new(format!("invalid JSON: {error}")))?`;
    }
    if (kind.option) return rustParseValue(kind.option, expression);
    if (kind.type) return rustParseValue(kind.type, expression);
    fail(`unsupported CLI parameter type: ${JSON.stringify(kind)}`);
  }

  const cliClients = [
    ...new Map(
      cliEntries
        .filter((entry) => entry.client)
        .map((entry) => [entry.client!.className, entry.client!] as const),
    ).values(),
  ];
  const cliTs: string[] = [
    header,
    `import type { Transport } from "./transport";`,
    `import * as types from "./types";`,
    "",
    `// CLI client. Parses flags and delegates to the HTTP client over the same transport.`,
  ];
  if (cliClients.length > 0) {
    cliTs.push(
      `import { ${cliClients.map((client) => client.className).join(", ")} } from "./http";`,
    );
  }
  cliTs.push(
    "",
    "export async function run(transport: Transport, verb: string, args: string[]): Promise<string> {",
    "  switch (verb) {",
  );

  const cliRust: string[] = [
    header,
    "use crate::{ClientError, Transport};",
    "",
    "/// CLI client. Parses flags and delegates to the HTTP client over the same transport.",
  ];
  if (cliClients.length === 1) {
    cliRust.push(`use crate::http::${cliClients[0].className};`);
  } else if (cliClients.length > 1) {
    cliRust.push(
      `use crate::http::{${cliClients.map((client) => client.className).join(", ")}};`,
    );
  }
  cliRust.push(
    "",
    "pub fn run<T: Transport>(transport: T, verb: &str, args: &[String]) -> Result<String, ClientError> {",
    "    let _ = &transport;",
    "    match verb {",
  );

  for (const entry of cliEntries) {
    if (!entry.route || !entry.client) {
      cliTs.push(
        `    case ${JSON.stringify(entry.verb)}:`,
        `      throw new Error(${JSON.stringify(`verb \`${entry.verb}\` is unavailable: no route`)});`,
      );
      cliRust.push(
        `        ${JSON.stringify(entry.verb)} => Err(ClientError::new(${JSON.stringify(
          `verb \`${entry.verb}\` is unavailable: no route`,
        )})),`,
      );
      continue;
    }

    const declarations: string[] = [];
    const cases: string[] = [];
    const finals: string[] = [];
    const argsUsed: string[] = [];

    for (const param of entry.fn.params) {
      const base = cliBase(param.type);
      const optional = isOption(param.type);
      const name = `__arg_${camel(param.name)}`;
      const flag = `--${param.name}`;
      argsUsed.push(isOption(param.type) ? `(${name} ?? null)` : name);
      if (cliList(base)) {
        const listRef = witType(doc, base as number).kind.list;
        if (optional) {
          declarations.push(`      let ${name}: ${tsType(base)} | undefined;`);
          cases.push(
            `        case ${JSON.stringify(flag)}: {`,
            `          const __gen_value = ${tsParseValue(listRef, "__gen_take()")};`,
            `          ${name} ??= [];`,
            `          ${name}.push(__gen_value);`,
            "          break;",
            "        }",
          );
        } else {
          declarations.push(`      const ${name}: ${tsType(base)} = [];`);
          cases.push(
            `        case ${JSON.stringify(flag)}: {`,
            `          const __gen_value = ${tsParseValue(listRef, "__gen_take()")};`,
            `          ${name}.push(__gen_value);`,
            "          break;",
            "        }",
          );
        }
      } else if (cliBool(base)) {
        declarations.push(`      let ${name}: boolean | undefined;`);
        cases.push(
          `        case ${JSON.stringify(flag)}: ${name} = true; break;`,
          `        case ${JSON.stringify(`--no-${param.name}`)}: ${name} = false; break;`,
        );
      } else {
        declarations.push(`      let ${name}: ${tsType(base)} | undefined;`);
        cases.push(
          `        case ${JSON.stringify(flag)}: ${name} = ${tsParseValue(base, "__gen_take()")}; break;`,
        );
      }
      if (!optional && !cliList(base)) {
        finals.push(
          `      if (${name} === undefined) throw new Error(${JSON.stringify(`missing ${flag}`)});`,
        );
      }
    }

    cliTs.push(
      `    case ${JSON.stringify(entry.verb)}: {`,
      ...declarations,
      "      for (let __gen_index = 0; __gen_index < args.length; __gen_index += 1) {",
      "        const __gen_argument = args[__gen_index];",
      "        const __gen_equals = __gen_argument.indexOf(\"=\");",
      "        const __gen_flag = __gen_equals >= 0 ? __gen_argument.slice(0, __gen_equals) : __gen_argument;",
      "        const __gen_inline = __gen_equals >= 0 ? __gen_argument.slice(__gen_equals + 1) : undefined;",
      "        const __gen_take = (): string => {",
      "          if (__gen_inline !== undefined) return __gen_inline;",
      "          __gen_index += 1;",
      "          const __gen_value = args[__gen_index];",
      '          if (__gen_value === undefined) throw new Error(`${__gen_flag} needs a value`);',
      "          return __gen_value;",
      "        };",
      "        switch (__gen_flag) {",
      ...cases,
      "          default:",
      '            throw new Error(`unknown flag \\`${__gen_argument}\\``);',
      "        }",
      "      }",
      ...finals,
      `      const __gen_client = new ${entry.client.className}(transport);`,
    );
    const result = returnRef(entry.fn);
    const call = `__gen_client.${camel(entry.fn.name)}(${argsUsed.join(", ")})`;
    if (result === null) {
      cliTs.push(`      await ${call};`, '      return "null";');
    } else {
      cliTs.push(`      return JSON.stringify(await ${call});`);
    }
    cliTs.push("    }");

    // Rust arm
    const rustDeclarations: string[] = [];
    const rustCases: string[] = [];
    const rustFinals: string[] = [];
    const rustArgs: string[] = [];
    for (const param of entry.fn.params) {
      const base = cliBase(param.type);
      const optional = isOption(param.type);
      const name = `__arg_${snake(param.name)}`;
      const flag = `--${param.name}`;
      rustArgs.push(name);
      if (cliList(base)) {
        const listRef = witType(doc, base as number).kind.list;
        if (optional) {
          rustDeclarations.push(`        let mut ${name}: Option<Vec<${rustType(listRef)}>> = None;`);
          rustCases.push(
            `            ${JSON.stringify(flag)} => {`,
            `                let __gen_value = ${rustParseValue(
              listRef,
              `next_value(args, &mut __gen_index, __gen_inline, ${JSON.stringify(flag)})?`,
            )};`,
            `                ${name}.get_or_insert_with(Vec::new).push(__gen_value);`,
            "            }",
          );
        } else {
          rustDeclarations.push(`        let mut ${name}: Vec<${rustType(listRef)}> = Vec::new();`);
          rustCases.push(
            `            ${JSON.stringify(flag)} => {`,
            `                let __gen_value = ${rustParseValue(
              listRef,
              `next_value(args, &mut __gen_index, __gen_inline, ${JSON.stringify(flag)})?`,
            )};`,
            `                ${name}.push(__gen_value);`,
            "            }",
          );
        }
      } else if (cliBool(base)) {
        rustDeclarations.push(`        let mut ${name}: Option<bool> = None;`);
        rustCases.push(
          `            ${JSON.stringify(flag)} => ${name} = Some(true),`,
          `            ${JSON.stringify(`--no-${param.name}`)} => ${name} = Some(false),`,
        );
      } else {
        rustDeclarations.push(`        let mut ${name}: Option<${rustType(base)}> = None;`);
        rustCases.push(
          `            ${JSON.stringify(flag)} => ${name} = Some(${rustParseValue(
            base,
            `next_value(args, &mut __gen_index, __gen_inline, ${JSON.stringify(flag)})?`,
          )}),`,
        );
      }
      if (!optional && !cliList(base)) {
        rustFinals.push(
          `        let ${name} = ${name}.ok_or_else(|| ClientError::new(${JSON.stringify(
            `missing ${flag}`,
          )}))?;`,
        );
      } else {
        rustFinals.push(`        let ${name} = ${name};`);
      }
    }

    cliRust.push(
      `        ${JSON.stringify(entry.verb)} => {`,
      ...rustDeclarations,
      "            let mut __gen_index = 0;",
      "            while __gen_index < args.len() {",
      "                let __gen_argument = &args[__gen_index];",
      "                let (__gen_flag, __gen_inline) = match __gen_argument.split_once('=') {",
      "                    Some((__gen_flag, __gen_value)) => (__gen_flag, Some(__gen_value)),",
      "                    None => (__gen_argument.as_str(), None),",
      "                };",
      "                match __gen_flag {",
      ...rustCases,
      "                    _ => return Err(ClientError::new(format!(\"unknown flag `{__gen_argument}`\"))),",
      "                }",
      "                __gen_index += 1;",
      "            }",
      ...rustFinals,
      `            let __gen_client = ${entry.client.className}::new(transport);`,
    );
    const rustResult = returnRef(entry.fn);
    const rustCall = `__gen_client.${rustField(entry.fn.name)}(${rustArgs.join(", ")})`;
    if (rustResult === null) {
      cliRust.push(
        `            ${rustCall}?;`,
        '            Ok("null".to_string())',
      );
    } else {
      cliRust.push(
        `            let __gen_result = ${rustCall}?;`,
        "            serde_json::to_string(&__gen_result)",
        '                .map_err(|error| ClientError::new(format!("cannot encode the result: {error}")))',
      );
    }
    cliRust.push("        }");
  }

  cliTs.push(
    "    default:",
    '      throw new Error(`unknown verb \\`${verb}\\``);',
    "  }",
    "}",
    "",
  );
  cliRust.push(
    "        _ => Err(ClientError::new(format!(\"unknown verb `{verb}`\"))),",
    "    }",
    "}",
    "",
    "#[allow(dead_code)]",
    "fn next_value(",
    "    args: &[String],",
    "    index: &mut usize,",
    "    inline: Option<&str>,",
    "    flag: &str,",
    ") -> Result<String, ClientError> {",
    "    if let Some(value) = inline {",
    "        return Ok(value.to_string());",
    "    }",
    "    *index += 1;",
    "    args.get(*index)",
    "        .cloned()",
    "        .ok_or_else(|| ClientError::new(format!(\"{flag} needs a value\")))",
    "}",
    "",
  );

  return { commands, cliTs, cliRust };
}
