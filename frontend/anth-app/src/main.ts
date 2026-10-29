import { applyInitialLocale, setupLocalization } from 'anth-widget'
import {
  HttpAccountClient,
  HttpAppsClient,
  HttpAuthClient,
  HttpFilesClient,
  HttpFirewallClient,
  HttpHostInfoClient,
  HttpNetworkClient,
  HttpProcessesClient,
  HttpSettingsClient,
  HttpStorageClient,
  HttpTransport,
} from 'anth-client'
import { AnthApp } from './app'

// One transport, many typed views. A deployment that moves the API to another
// origin only changes this line (or passes a token here).
const transport = new HttpTransport()

setupLocalization()

AnthApp.authClient = new HttpAuthClient(transport)
AnthApp.hostClient = new HttpHostInfoClient(transport)
AnthApp.filesClient = new HttpFilesClient(transport)
AnthApp.processesClient = new HttpProcessesClient(transport)
AnthApp.networkClient = new HttpNetworkClient(transport)
AnthApp.firewallClient = new HttpFirewallClient(transport)
AnthApp.storageClient = new HttpStorageClient(transport)
AnthApp.accountsClient = new HttpAccountClient(transport)
AnthApp.appsClient = new HttpAppsClient(transport)
AnthApp.settingsClient = new HttpSettingsClient(transport)

await applyInitialLocale()
