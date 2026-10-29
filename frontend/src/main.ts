import { applyInitialLocale, setupLocalization } from './i18n'
import { AnthApp } from './app'
import { HttpAccountClient } from './accounts-client'
import { HttpAuthClient } from './auth-client'
import { HttpFilesClient } from './files-client'
import { HttpFirewallClient } from './firewall-client'
import { HttpHostInfoClient } from './host-info-client'
import { HttpNetworkClient } from './network-client'
import { HttpAppsClient } from './apps-client'
import { HttpProcessesClient } from './processes-client'
import { HttpSettingsClient } from './settings-client'
import { HttpStorageClient } from './storage-client'

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
