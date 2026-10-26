import "./theme.css";
import "./tab-panel";
import "./fab";
import "./nav-drawer";
import "./auth-gate";

import { AnthApp } from "./app";
import { AnthAuthGate } from "./auth-gate";
import { HttpAuthClient } from "./auth-client";
import { HttpHostInfoClient } from "./host-info-client";
import { HttpMaintenanceClient } from "./maintenance-client";
import { AnthMaintenance } from "./maintenance";
import { HttpWidgetHost } from "./widget-slots";

AnthApp.client = new HttpHostInfoClient();
AnthApp.widgets = new HttpWidgetHost();
AnthAuthGate.client = new HttpAuthClient();
AnthMaintenance.client = new HttpMaintenanceClient();

// The gate is the first screen, and the shell does not exist until a session
// does: a mounted `anth-app` fetches a snapshot on connect, so creating it
// before the cookie arrives would spend its first paint on a `401` and leave an
// error card behind the form. Mounting it after means its first request is
// authorised, and a reload with a live cookie lands straight in the panel.
const gate = document.querySelector("anth-auth-gate");
window.addEventListener(AnthAuthGate.SIGNED_IN, () => {
  gate?.remove();
  document.body.append(document.createElement("anth-app"));
});
