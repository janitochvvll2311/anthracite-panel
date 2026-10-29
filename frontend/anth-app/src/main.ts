import { applyInitialLocale, setupLocalization } from 'anth-widget'
import { AnthApp } from './app'
import { HttpAccountClient } from 'anth-client'
import { HttpAuthClient } from 'anth-client'
import { HttpFilesClient } from 'anth-client'
import { HttpFirewallClient } from 'anth-client'
import { HttpHostInfoClient } from 'anth-client'
import { HttpNetworkClient } from 'anth-client'
import { HttpAppsClient } from 'anth-client'
import { HttpProcessesClient } from 'anth-client'
import { HttpSettingsClient } from 'anth-client'
import { HttpStorageClient } from 'anth-client'

setupLocalization()

AnthApp.authClient = new HttpAuthClient()
AnthApp.hostClient = new HttpHostInfoClient()
AnthApp.filesClient = new HttpFilesClient()
AnthApp.processesClient = new HttpProcessesClient()
AnthApp.networkClient = new HttpNetworkClient()
AnthApp.firewallClient = new HttpFirewallClient()
AnthApp.storageClient = new HttpStorageClient()
AnthApp.accountsClient = new HttpAccountClient()
AnthApp.appsClient = new HttpAppsClient()
AnthApp.settingsClient = new HttpSettingsClient()

await applyInitialLocale()
