import { AnthApp } from "./app";
import { HttpHostInfoClient } from "./host-info-client";
import { AnthRepl } from "./repl-element";
import { HttpReplBackend } from "./repl";

AnthApp.client = new HttpHostInfoClient();
AnthRepl.backend = new HttpReplBackend();

if (!customElements.get("anth-repl")) {
  customElements.define("anth-repl", AnthRepl);
}
