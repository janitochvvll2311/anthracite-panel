import { vi } from 'vitest'
import type {
  PluginClient,
  PluginInstallResult,
  PluginSummary,
  PluginsCatalogue,
} from '../lib/plugins/plugin-client'

export function pluginSummary(
  overrides: Partial<PluginSummary> = {},
): PluginSummary {
  return {
    id: 'org.anth.widget',
    name: 'Widget',
    version: '0.1.0',
    enabled: true,
    capabilities: [],
    api: [],
    ui: [],
    ...overrides,
  }
}

export function pluginsCatalogue(
  overrides: Partial<PluginsCatalogue> = {},
): PluginsCatalogue {
  return {
    enabled: true,
    install_from_web: true,
    plugins: [pluginSummary()],
    problems: [],
    ...overrides,
  }
}

export class FakePluginClient implements PluginClient {
  current = pluginsCatalogue()
  listError?: Error
  installError?: Error
  warning?: string | null
  installCalls: File[] = []

  list = vi.fn(async (): Promise<PluginsCatalogue> => {
    if (this.listError) throw this.listError
    return this.current
  })

  install = vi.fn(async (file: File): Promise<PluginInstallResult> => {
    if (this.installError) throw this.installError
    this.installCalls.push(file)
    return {
      id: 'org.anth.widget',
      version: '0.1.0',
      name: 'Widget',
      warning: this.warning,
    }
  })

  moduleUrl(pluginId: string, module: string): string {
    return `/api/plugins/${pluginId}/web/${module.replace(/^web\//, '')}`
  }

  emit(catalogue: PluginsCatalogue): void {
    this.current = catalogue
  }
}
