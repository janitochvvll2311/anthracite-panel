import { readFileSync } from "node:fs";
import { join } from "node:path";

import type {
  Generation,
  TypeRef,
  WitFunction,
  WitInterface,
  WitType,
} from "./model";
import {
  camel,
  fail,
  formatRust,
  header,
  pascal,
  rustField,
  rustVariant,
  snake,
  token,
} from "./util";
import {
  interfaceId,
  packageName,
  parseWit,
  rustPrimitive,
  splitPackage,
} from "./wit";
import { createTypes } from "./types";
import { createHttp } from "./http";
import { createCli } from "./cli";

export function generate(appDir: string, catalog: string): Generation {
  const manifest = Bun.TOML.parse(
    readFileSync(join(appDir, "manifest.toml"), "utf8"),
  ) as any;
  const doc = parseWit(appDir, catalog);
  const routes = new Map<string, any>();
  for (const route of manifest.route ?? []) {
    routes.set(route.call, route);
  }
  const verbs = new Map<string, string>();
  for (const entry of manifest.cli ?? []) {
    verbs.set(entry.call, entry.verb);
  }

  const exported: { id: string; iface: WitInterface }[] = [];
  for (const entry of manifest.export ?? []) {
    const iface = doc.interfaces.find((candidate) =>
      interfaceId(doc, candidate) === entry.interface,
    );
    if (!iface) fail(`exported interface \`${entry.interface}\` is missing from WIT`);
    exported.push({ id: entry.interface, iface });
  }

  const types = createTypes(doc);
  const {
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
  } = types;

  const http = createHttp(doc, types);

  const tsTypes: string[] = [];
  const rustTypes: string[] = [];
  const tsInterfaces: string[] = [];
  const rustInterfaces: string[] = [];
  const httpExportsTs: string[] = [];
  const httpExportsRust: string[] = [];
  const classByInterface = new Map<string, { className: string; module: string }>();

  const generatedInterfaces: string[] = [];
  const shortNameCounts = new Map<string, number>();
  for (const { iface } of exported) {
    const short = pascal(iface.name ?? "client");
    shortNameCounts.set(short, (shortNameCounts.get(short) ?? 0) + 1);
  }
  for (const { iface } of exported) {
    const methods: { fn: WitFunction; route: any }[] = [];
    for (const fn of Object.values(iface.functions)) {
      const call = `${interfaceId(doc, iface)}#${fn.name}`;
      const route = routes.get(call);
      if (route) methods.push({ fn, route });
    }
    if (methods.length === 0) continue;
    const ifaceName = iface.name ?? "client";
    const short = pascal(ifaceName);
    const pkg = splitPackage(packageName(doc, iface.package));
    const packageToken = pascal(token(pkg.base));
    const module = `${token(pkg.base)}_${token(ifaceName)}_routes`;
    const classname = `${(shortNameCounts.get(short) ?? 0) > 1 ? packageToken : ""}${short}HttpClient`;
    const tsLines = [
      header,
      `import type { Transport } from "./transport";`,
      `import type * as types from "./types";`,
      "",
      `// HTTP/route client. The transport is an HTTP adapter (UDS, TCP, fetch).`,
      `export class ${classname} {`,
      `  constructor(private readonly request: Transport, private readonly base = "/api/apps/${manifest.app.id}") {}`,
      ...methods.map(({ fn, route }) => http.tsMethod(fn, route)),
      "}",
      "",
    ];
    tsInterfaces.push(tsLines.join("\n"));
    const rustLines = [
      header,
      `/// HTTP/route client. The transport is an HTTP adapter (UDS, TCP, fetch).`,
      `pub struct ${classname}<T: crate::Transport> {`,
      "    transport: T,",
      "    base: String,",
      "}",
      "",
      `impl<T: crate::Transport> ${classname}<T> {`,
      "    pub fn new(transport: T) -> Self {",
      `        Self { transport, base: "/api/apps/${manifest.app.id}".to_string() }`,
      "    }",
      "",
      "    pub fn with_base(transport: T, base: impl Into<String>) -> Self {",
      "        Self { transport, base: base.into() }",
      "    }",
      ...methods.map(({ fn, route }) => http.rustMethod(fn, route)),
      "}",
      "",
    ];
    rustInterfaces.push(rustLines.join("\n"));
    generatedInterfaces.push(module);
    httpExportsTs.push(`export * from "./${module}";`);
    httpExportsRust.push(`pub use crate::${module}::*;`);
    classByInterface.set(interfaceId(doc, iface), { className: classname, module });
  }

  // Named records and enums referenced by route signatures.
  for (const type of [...named.values()].sort((a, b) =>
    typeKey(a) < typeKey(b) ? -1 : 1,
  )) {
    const name = typeKey(type);
    if (type.kind.record) {
      const fields = type.kind.record.fields as { name: string; type: TypeRef }[];
      const tsFields = fields
        .map((field) => {
          collect(field.type);
          const key = /^[A-Za-z_$][\w$]*$/.test(field.name)
            ? field.name
            : JSON.stringify(field.name);
          return `  ${key}: ${tsType(field.type)};`;
        })
        .join("\n");
      tsTypes.push(`export interface ${name} {\n${tsFields}\n}\n`);
      const rustFields = fields
        .map((field) => {
          collect(field.type);
          return `    #[serde(rename = ${JSON.stringify(field.name)})]\n    pub ${rustField(field.name)}: ${rustType(field.type)},`;
        })
        .join("\n");
      rustTypes.push(
        `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct ${name} {\n${rustFields}\n}\n`,
      );
    } else if (type.kind.enum) {
      const cases = type.kind.enum.cases as { name: string }[];
      tsTypes.push(
        `export type ${name} = ${cases.map((c) => JSON.stringify(c.name)).join(" | ")};\n`,
      );
      rustTypes.push(
        `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub enum ${name} {\n${cases
          .map(
            (c) =>
              `    #[serde(rename = ${JSON.stringify(c.name)})]\n    ${rustVariant(c.name)},`,
          )
          .join("\n")}\n}\n`,
      );
      rustTypes.push(
        `impl ${name} {\n    pub fn parse(value: &str) -> Option<Self> {\n        match value {\n${cases
          .map(
            (c) =>
              `            ${JSON.stringify(c.name)} => Some(Self::${rustVariant(c.name)}),`,
          )
          .join("\n")}\n            _ => None,\n        }\n    }\n}\n`,
      );
    } else {
      fail(`named type \`${type.name}\` is not a record or enum`);
    }
  }

  const { commands, cliTs, cliRust } = createCli(
    doc,
    manifest,
    routes,
    types,
    classByInterface,
  );


  const tsHttpFile = `${header}\n${httpExportsTs.join("\n")}\n`;
  const rustHttpFile = `${header}\n${httpExportsRust.join("\n")}\n`;

  const tsTypesFile = tsTypes.length
    ? `${header}\n${tsTypes.join("\n")}`
    : `${header}\n// No named types are used by the generated routes.\nexport {};\n`;
  const rustTypesFile = rustTypes.length
    ? `${header}\nuse serde::{Deserialize, Serialize};\n\n${rustTypes.join("\n")}`
    : `${header}\n// No named types are used by the generated routes.\n`;

  const tsCommands = [
    header,
    "export type CommandParam = { name: string; type: string; optional: boolean };",
    "export type Command = {",
    "  verb: string;",
    "  method: string | null;",
    "  path: string | null;",
    "  interface: string;",
    "  function: string;",
    "  params: CommandParam[];",
    "  returns: string;",
    "};",
    "",
    `export const commands: Command[] = ${JSON.stringify(commands, null, 2)};`,
    "",
  ].join("\n");

  const rustCommands = [
    header,
    "pub struct CommandParam {",
    "    pub name: &'static str,",
    "    pub r#type: &'static str,",
    "    pub optional: bool,",
    "}",
    "",
    "pub struct Command {",
    "    pub verb: &'static str,",
    "    pub method: Option<&'static str>,",
    "    pub path: Option<&'static str>,",
    "    pub interface: &'static str,",
    "    pub function: &'static str,",
    "    pub params: &'static [CommandParam],",
    "    pub returns: &'static str,",
    "}",
    "",
    "pub const COMMANDS: &[Command] = &[",
    ...commands.map((command) =>
      [
        "    Command {",
        `        verb: ${JSON.stringify(command.verb)},`,
        `        method: ${command.method ? `Some(${JSON.stringify(command.method)})` : "None"},`,
        `        path: ${command.path ? `Some(${JSON.stringify(command.path)})` : "None"},`,
        `        interface: ${JSON.stringify(command.interface)},`,
        `        function: ${JSON.stringify(command.function)},`,
        `        params: &[${command.params
          .map(
            (param: any) =>
              `CommandParam { name: ${JSON.stringify(param.name)}, r#type: ${JSON.stringify(
                param.type,
              )}, optional: ${param.optional} }`,
          )
          .join(", ")}],`,
        `        returns: ${JSON.stringify(command.returns)},`,
        "    },",
      ].join("\n"),
    ),
    "];",
    "",
  ].join("\n");

  const tsIndex = [
    header,
    'export * from "./transport";',
    'export * from "./types";',
    'export * from "./commands";',
    'export * from "./http";',
    'export * from "./cli";',
    "",
  ].join("\n");

  const rustLib = [
    header,
    "use std::fmt;",
    "",
    "#[derive(Debug, Clone)]",
    "pub struct ClientError {",
    "    pub message: String,",
    "}",
    "",
    "impl ClientError {",
    "    pub fn new(message: impl Into<String>) -> Self {",
    "        Self { message: message.into() }",
    "    }",
    "}",
    "",
    "impl fmt::Display for ClientError {",
    "    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {",
    "        formatter.write_str(&self.message)",
    "    }",
    "}",
    "",
    "impl std::error::Error for ClientError {}",
    "",
    "/// An HTTP request adapter. Implement it over a UDS (HTTP over the socket), TCP, or fetch.",
    "pub trait Transport {",
    "    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, ClientError>;",
    "}",
    "",
    "pub fn query_value<T: serde::Serialize>(value: &T) -> Result<String, ClientError> {",
    "    let text = serde_json::to_string(value)",
    '        .map_err(|error| ClientError::new(format!("cannot encode a query value: {error}")))?;',
    '    let text = if text.starts_with(\'"\') {',
    "        serde_json::from_str::<String>(&text).unwrap_or_default()",
    "    } else {",
    "        text",
    "    };",
    "    Ok(percent_encoding::utf8_percent_encode(&text, percent_encoding::NON_ALPHANUMERIC).to_string())",
    "}",
    "",
    "pub mod commands;",
    "pub mod types;",
    ...generatedInterfaces.map((module) => `pub mod ${module};`),
    "pub mod http;",
    "pub mod cli;",
    "pub use commands::{Command, CommandParam, COMMANDS};",
    "pub use http::*;",
    "pub use cli::run;",
    ...(named.size > 0 ? ["pub use types::*;"] : []),
    "",
  ].join("\n");

  const tsTransport = `${header}\n/** An HTTP request adapter: UDS (HTTP over the socket), TCP, or fetch. */\nexport type Transport = (method: string, path: string, body?: string) => Promise<string>;\n`;
  const tsPackage = `${JSON.stringify(
    {
      name: "@anthracite/client",
      version: "0.5.0",
      private: true,
      type: "module",
      description: "Generated clients for Anthracite apps",
      exports: { ".": "./src/index.ts" },
      scripts: { test: "bun test", typecheck: "tsc --noEmit" },
      devDependencies: { typescript: "5.6.3" },
    },
    null,
    2,
  )}\n`;
  const cargo = `[package]
name = "anth-client"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Generated clients for Anthracite apps"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
percent-encoding = "2"
`;

  const files: Record<string, string> = {
    "sdk/ts/client/package.json": tsPackage,
    "sdk/ts/client/src/index.ts": tsIndex,
    "sdk/ts/client/src/transport.ts": tsTransport,
    "sdk/ts/client/src/types.ts": tsTypesFile,
    "sdk/ts/client/src/commands.ts": tsCommands,
    "sdk/ts/client/src/http.ts": tsHttpFile,
    "sdk/ts/client/src/cli.ts": cliTs.join("\n"),
    "sdk/rust/client/Cargo.toml": cargo,
    "sdk/rust/client/src/lib.rs": rustLib,
    "sdk/rust/client/src/types.rs": rustTypesFile,
    "sdk/rust/client/src/commands.rs": rustCommands,
    "sdk/rust/client/src/http.rs": rustHttpFile,
    "sdk/rust/client/src/cli.rs": cliRust.join("\n"),
  };
  for (let index = 0; index < generatedInterfaces.length; index += 1) {
    files[`sdk/ts/client/src/${generatedInterfaces[index]}.ts`] = tsInterfaces[index];
    files[`sdk/rust/client/src/${generatedInterfaces[index]}.rs`] = rustInterfaces[index];
  }
  for (const [relative, content] of Object.entries(files)) {
    if (relative.endsWith(".rs")) files[relative] = formatRust(content);
  }
  return { files };
}
