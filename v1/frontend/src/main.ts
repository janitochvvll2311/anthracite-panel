/** Panel entry point: applies the stored locale, then mounts the shell. */
import './app/anth-app'
import { applyInitialLocale } from './app/i18n'

await applyInitialLocale()

const root = document.getElementById('app')
if (root) root.replaceChildren(document.createElement('anth-app'))
