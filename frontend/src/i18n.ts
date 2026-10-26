import { configureLocalization } from '@lit/localize'

export type PanelLocale = 'en' | 'es'

const SUPPORTED: readonly PanelLocale[] = ['en', 'es']
const STORAGE_KEY = 'anth.locale'

interface Accessors {
  getLocale: () => string
  setLocale: (locale: string) => Promise<void>
}

let accessors: Accessors | undefined

function storedLocale(): PanelLocale | undefined {
  try {
    const value = globalThis.localStorage?.getItem(STORAGE_KEY)
    return SUPPORTED.includes(value as PanelLocale) ? (value as PanelLocale) : undefined
  } catch {
    return undefined
  }
}

export function setupLocalization(): Accessors {
  if (accessors) return accessors

  const { getLocale, setLocale } = configureLocalization({
    sourceLocale: 'en',
    targetLocales: ['es'],
    loadLocale: async (locale) => {
      if (locale === 'es') return import('./generated/locales/es')
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
        // A blocked storage API does not stop the current page.
      }
    },
  }
  return accessors
}

export async function applyInitialLocale(): Promise<void> {
  const locale = storedLocale()
  if (locale && locale !== 'en') {
    await setupLocalization().setLocale(locale)
  }
}

export async function changeLocale(locale: PanelLocale): Promise<void> {
  await setupLocalization().setLocale(locale)
}

export function currentLocale(): PanelLocale {
  const locale = setupLocalization().getLocale()
  return SUPPORTED.includes(locale as PanelLocale) ? (locale as PanelLocale) : 'en'
}
