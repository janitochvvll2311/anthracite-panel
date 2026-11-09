import type { WitDoc, WitFunction } from "./model";
import { camel, fail, rustField } from "./util";
import type { Types } from "./types";

/// The typed HTTP/route client for one exported interface. It owns only the
/// method body; the type graph and the two runtimes come from `Types`.
export function createHttp(doc: WitDoc, types: Types) {
  const {
    collect,
    returnRef,
    tsType,
    rustType,
    isOption,
    inner,
    isRecord,
    tsParamType,
    rustParamType,
  } = types;

  function tsParams(fn: WitFunction): string {
    return fn.params
      .map((param) => {
        for (const ref of [param.type]) collect(ref);
        const optional = isOption(param.type);
        return `${camel(param.name)}: ${tsParamType(param.type)}${optional ? " | null" : ""}`;
      })
      .join(", ");
  }

  function rustParams(fn: WitFunction): string {
    return fn.params
      .map((param) => {
        for (const ref of [param.type]) collect(ref);
        return `${rustField(param.name)}: ${rustParamType(param.type)}`;
      })
      .join(", ");
  }

  function tsQuery(fn: WitFunction): { declarations: string[]; appends: string[] } {
    const declarations = ["    const __gen_query = new URLSearchParams();"];
    const appends: string[] = [];
    for (const param of fn.params) {
      const ref = isOption(param.type) ? inner(param.type) : param.type;
      const name = camel(param.name);
      const list = typeof ref === "number" && Boolean(witType(doc, ref).kind?.list);
      const key = list ? `${param.name}[]` : param.name;
      const append = (value: string) =>
        `      __gen_query.append(${JSON.stringify(key)}, ${value});`;
      if (isOption(param.type)) {
        appends.push(
          `    if (${name} !== undefined && ${name} !== null) {`,
        );
      }
      if (list) {
        appends.push(`    for (const __gen_value of ${name}) {`);
        appends.push(append("String(__gen_value)"));
        appends.push("    }");
      } else {
        if (typeof ref === "number" && witType(doc, ref).kind?.record) {
          fail("a record cannot be used in a query; use POST/PUT/PATCH");
        }
        appends.push(append(`String(${name})`));
      }
      if (isOption(param.type)) appends.push("    }");
    }
    return { declarations, appends };
  }

  function rustQuery(fn: WitFunction): { declarations: string[]; appends: string[] } {
    const declarations = ["        let mut __gen_query = Vec::new();"];
    const appends: string[] = [];
    for (const param of fn.params) {
      const ref = isOption(param.type) ? inner(param.type) : param.type;
      const name = rustField(param.name);
      const list = typeof ref === "number" && Boolean(witType(doc, ref).kind?.list);
      const key = list ? `${param.name}[]` : param.name;
      const append = (value: string) =>
        `            __gen_query.push(format!("{}={}", ${JSON.stringify(key)}, crate::query_value(&${value})?));`;
      if (isOption(param.type)) {
        appends.push(`        if let Some(__gen_value) = &${name} {`);
        if (list) {
          appends.push("            for __gen_value in __gen_value {");
          appends.push(append("__gen_value"));
          appends.push("            }");
        } else {
          appends.push(append("__gen_value"));
        }
        appends.push("        }");
      } else if (list) {
        appends.push(`        for __gen_value in &${name} {`);
        appends.push(append("__gen_value"));
        appends.push("        }");
      } else {
        if (typeof ref === "number" && witType(doc, ref).kind?.record) {
          fail("a record cannot be used in a query; use POST/PUT/PATCH");
        }
        appends.push(append(name));
      }
    }
    return { declarations, appends };
  }

  function tsBody(fn: WitFunction, method: string): string | null {
    if (["GET", "HEAD", "DELETE"].includes(method)) return null;
    if (fn.params.length === 1 && isRecord(fn.params[0].type)) {
      return `    const __gen_body = JSON.stringify(${camel(fn.params[0].name)});`;
    }
    if (fn.params.length === 0) return null;
    const fields = fn.params
      .map((param) => `${JSON.stringify(param.name)}: ${camel(param.name)}`)
      .join(", ");
    return `    const __gen_body = JSON.stringify({ ${fields} });`;
  }

  function rustBody(fn: WitFunction, method: string): string | null {
    if (["GET", "HEAD", "DELETE"].includes(method)) return null;
    if (fn.params.length === 1 && isRecord(fn.params[0].type)) {
      return `        let __gen_body = serde_json::to_string(&${rustField(fn.params[0].name)})
            .map_err(|error| crate::ClientError::new(format!("cannot encode the body: {error}")))?;`;
    }
    if (fn.params.length === 0) return null;
    const fields = fn.params
      .map((param) => `${JSON.stringify(param.name)}: ${rustField(param.name)}`)
      .join(", ");
    return `        let __gen_body = serde_json::to_string(&serde_json::json!({ ${fields} }))
            .map_err(|error| crate::ClientError::new(format!("cannot encode the body: {error}")))?;`;
  }

  function tsMethod(fn: WitFunction, route: any): string {
    const method = String(route.method).toUpperCase();
    const params = tsParams(fn);
    const returnType = returnRef(fn);
    for (const ref of returnType === null ? [] : [returnType]) collect(ref);
    const usesQuery = ["GET", "HEAD", "DELETE"].includes(method);
    const query = usesQuery ? tsQuery(fn) : { declarations: [], appends: [] };
    const body = tsBody(fn, method);
    const lines: string[] = [];
    lines.push(
      `  async ${camel(fn.name)}(${params}): Promise<${returnType === null ? "void" : tsType(returnType)}> {`,
    );
    if (query.appends.length > 0) {
      lines.push(...query.declarations, ...query.appends);
      lines.push(`    const __gen_qs = __gen_query.toString();`);
      lines.push(
        `    const __gen_path = \`\${this.base}${route.path}\${__gen_qs ? \`?\${__gen_qs}\` : ""}\`;`,
      );
    } else {
      lines.push(`    const __gen_path = \`\${this.base}${route.path}\`;`);
    }
    if (body) lines.push(body);
    if (returnType === null) {
      lines.push(
        `    await this.request("${method}", __gen_path${body ? ", __gen_body" : ""});`,
      );
    } else {
      lines.push(
        `    const __gen_text = await this.request("${method}", __gen_path${body ? ", __gen_body" : ""});`,
      );
      lines.push(`    return JSON.parse(__gen_text) as ${tsType(returnType)};`);
    }
    lines.push("  }");
    return lines.join("\n");
  }

  function rustMethod(fn: WitFunction, route: any): string {
    const method = String(route.method).toUpperCase();
    const params = rustParams(fn);
    const returnType = returnRef(fn);
    for (const ref of returnType === null ? [] : [returnType]) collect(ref);
    const usesQuery = ["GET", "HEAD", "DELETE"].includes(method);
    const query = usesQuery ? rustQuery(fn) : { declarations: [], appends: [] };
    const body = rustBody(fn, method);
    const lines: string[] = [];
    const result = returnType === null ? "()" : rustType(returnType);
    lines.push(
      `    pub fn ${rustField(fn.name)}(&self${params ? `, ${params}` : ""}) -> Result<${result}, crate::ClientError> {`,
    );
    if (query.appends.length > 0) lines.push(...query.declarations, ...query.appends);
    if (query.appends.length > 0) {
      lines.push(`        let mut __gen_path = format!("{}{}", self.base, ${JSON.stringify(route.path)});`);
      lines.push("        if !__gen_query.is_empty() {");
      lines.push("            __gen_path.push('?');");
      lines.push("            __gen_path.push_str(&__gen_query.join(\"&\"));");
      lines.push("        }");
    } else {
      lines.push(`        let __gen_path = format!("{}{}", self.base, ${JSON.stringify(route.path)});`);
    }
    if (body) lines.push(body);
    lines.push(
      `        let __gen_text = self.transport.request("${method}", &__gen_path, ${body ? "Some(&__gen_body)" : "None"})?;`,
    );
    if (returnType === null) {
      lines.push("        Ok(())");
    } else {
      lines.push(
        `        serde_json::from_str::<${rustType(returnType)}>(&__gen_text)
            .map_err(|error| crate::ClientError::new(format!("invalid JSON: {error}")))`,
      );
    }
    lines.push("    }");
    return lines.join("\n");
  }


  return { tsMethod, rustMethod };
}
