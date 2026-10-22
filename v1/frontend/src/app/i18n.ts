/**
 * Runtime localization for the panel.
 *
 * Source strings are English and live in the components; `lit-localize`
 * extracts them into `xliff/es.xlf` and generates `src/generated/locales/es.ts`.
 * Only Spanish ships today, so the loader has one branch and no locale codes
 * module is imported at runtime.
 *
 * `configureLocalization` may only run once per page. Tests import modules in
 * arbitrary orders, so the wrapper below is idempotent and the second call
 * returns the already configured accessors instead of throwing.
 */

import { configureLocalization } from '@lit/localize'

export type PanelLocale = 'en' | 'es'

const SUPPORTED: readonly PanelLocale[] = ['en', 'es']
const STORAGE_KEY = 'anth.locale'

interface LocalizationAccessors {
  getLocale: () => string
  setLocale: (locale: string) => Promise<void>
}

let accessors: LocalizationAccessors | undefined

function storedLocale(): PanelLocale | undefined {
  try {
    const value = globalThis.localStorage?.getItem(STORAGE_KEY)
    return SUPPORTED.includes(value as PanelLocale)
      ? (value as PanelLocale)
      : undefined
  } catch {
    // A blocked storage API is not a reason to lose the UI.
    return undefined
  }
}

/** Locale to start with: the stored choice, else the source locale. */
export function initialLocale(): PanelLocale {
  return storedLocale() ?? 'en'
}

/**
 * Configures the localization runtime and returns its accessors.
 *
 * Safe to call from more than one module: the first call wins and the rest get
 * the same accessors.
 */
export function setupLocalization(): LocalizationAccessors {
  if (accessors) return accessors

  const { getLocale, setLocale } = configureLocalization({
    sourceLocale: 'en',
    targetLocales: ['es'],
    loadLocale: async (locale) => {
      if (locale === 'es') return import('../generated/locales/es')
      throw new Error(`unsupported locale: ${locale}`)
    },
  })

  accessors = {
    getLocale,
    setLocale: async (locale) => {
      if (!SUPPORTED.includes(locale as PanelLocale)) return
      await setLocale(locale)
      try {
        globalThis.localStorage?.setItem(STORAGE_KEY, locale)
      } catch {
        // The language still applies to this page.
      }
    },
  }

  return accessors
}

export function currentLocale(): PanelLocale {
  const locale = setupLocalization().getLocale()
  return SUPPORTED.includes(locale as PanelLocale)
    ? (locale as PanelLocale)
    : 'en'
}

export async function changeLocale(locale: PanelLocale): Promise<void> {
  await setupLocalization().setLocale(locale)
}

/** Applies the stored locale, if it is not the source locale. */
export async function applyInitialLocale(): Promise<void> {
  const locale = initialLocale()
  if (locale === 'en') return
  await changeLocale(locale)
}
