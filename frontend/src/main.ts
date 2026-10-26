import { applyInitialLocale, setupLocalization } from './i18n'
import { AnthApp } from './app'
import { HttpAuthClient } from './auth-client'
import { HttpFilesClient } from './files-client'
import { HttpHostInfoClient } from './host-info-client'
import { HttpSettingsClient } from './settings-client'

setupLocalization()

AnthApp.authClient = new HttpAuthClient()
AnthApp.hostClient = new HttpHostInfoClient()
AnthApp.filesClient = new HttpFilesClient()
AnthApp.settingsClient = new HttpSettingsClient()

await applyInitialLocale()
