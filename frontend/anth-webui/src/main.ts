import { AnthApp } from "./app";
import { HttpHostInfoClient } from "./host-info-client";
import { AnthRepl } from "./repl-element";
import { HttpReplBackend } from "./repl";
import { HttpWidgetHost } from "./widget-slots";

AnthApp.client = new HttpHostInfoClient();
AnthApp.widgets = new HttpWidgetHost();
AnthRepl.backend = new HttpReplBackend();

if (!customElements.get("anth-repl")) {
  customElements.define("anth-repl", AnthRepl);
}
