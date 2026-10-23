import { AnthApp } from "./app";
import { HttpHostInfoClient } from "./host-info-client";

AnthApp.client = new HttpHostInfoClient();
