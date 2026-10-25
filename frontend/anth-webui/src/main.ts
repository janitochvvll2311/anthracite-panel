import "./theme.css";
import "./tab-panel";
import "./fab";
import "./nav-drawer";

import { AnthApp } from "./app";
import { HttpHostInfoClient } from "./host-info-client";
import { HttpWidgetHost } from "./widget-slots";

AnthApp.client = new HttpHostInfoClient();
AnthApp.widgets = new HttpWidgetHost();
