import { applyInitialLocale, setupLocalization } from 'anth-widget'
import { AnthApp } from './app'
import { HttpAccountClient } from 'anth-wire'
import { HttpAuthClient } from 'anth-wire'
import { HttpFilesClient } from 'anth-wire'
import { HttpFirewallClient } from 'anth-wire'
import { HttpHostInfoClient } from 'anth-wire'
import { HttpNetworkClient } from 'anth-wire'
import { HttpAppsClient } from 'anth-wire'
import { HttpProcessesClient } from 'anth-wire'
import { HttpSettingsClient } from 'anth-wire'
import { HttpStorageClient } from 'anth-wire'

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
