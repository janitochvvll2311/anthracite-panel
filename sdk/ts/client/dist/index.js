// src/commands.ts
var commands = [
  {
    verb: "greet",
    method: "GET",
    path: "/hello",
    interface: "anthracite:hello/greeting@1.0.0",
    function: "greet",
    params: [
      {
        name: "name",
        type: "string",
        optional: false
      }
    ],
    returns: "string"
  }
];
// src/anthracite_hello_greeting_routes.ts
class GreetingHttpClient {
  request;
  base;
  constructor(request, base = "/api/apps/org.anthracite.hello") {
    this.request = request;
    this.base = base;
  }
  async greet(name) {
    const __gen_query = new URLSearchParams;
    __gen_query.append("name", String(name));
    const __gen_qs = __gen_query.toString();
    const __gen_path = `${this.base}/hello${__gen_qs ? `?${__gen_qs}` : ""}`;
    const __gen_text = await this.request("GET", __gen_path);
    return JSON.parse(__gen_text);
  }
}
// src/cli.ts
async function run(transport, verb, args) {
  switch (verb) {
    case "greet": {
      let __arg_name;
      for (let __gen_index = 0;__gen_index < args.length; __gen_index += 1) {
        const __gen_argument = args[__gen_index];
        const __gen_equals = __gen_argument.indexOf("=");
        const __gen_flag = __gen_equals >= 0 ? __gen_argument.slice(0, __gen_equals) : __gen_argument;
        const __gen_inline = __gen_equals >= 0 ? __gen_argument.slice(__gen_equals + 1) : undefined;
        const __gen_take = () => {
          if (__gen_inline !== undefined)
            return __gen_inline;
          __gen_index += 1;
          const __gen_value = args[__gen_index];
          if (__gen_value === undefined)
            throw new Error(`${__gen_flag} needs a value`);
          return __gen_value;
        };
        switch (__gen_flag) {
          case "--name":
            __arg_name = __gen_take();
            break;
          default:
            throw new Error(`unknown flag \`${__gen_argument}\``);
        }
      }
      if (__arg_name === undefined)
        throw new Error("missing --name");
      const __gen_client = new GreetingHttpClient(transport);
      return JSON.stringify(await __gen_client.greet(__arg_name));
    }
    default:
      throw new Error(`unknown verb \`${verb}\``);
  }
}
export {
  GreetingHttpClient,
  commands,
  run
};
