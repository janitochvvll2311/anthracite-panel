import { applyInitialLocale, setupLocalization } from './i18n'
import { AnthApp } from './app'
import { HttpAuthClient } from './auth-client'
import { HttpHostInfoClient } from './host-info-client'

setupLocalization()

AnthApp.authClient = new HttpAuthClient()
AnthApp.hostClient = new HttpHostInfoClient()

await applyInitialLocale()
