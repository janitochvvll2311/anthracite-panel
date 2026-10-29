import type { Change } from './actions'

export interface AppAnchor {
  method: string
  path: string
}

export interface AppUiSlot {
  slot: string
  tag: string
  module: string
  label?: string
  sha256: string
}

export interface AppView {
  id: string
  alias?: string
  name: string
  version: string
  enabled: boolean
  state: 'enabled' | 'disabled' | 'error' | 'unapproved'
  error?: string
  capabilities: string[]
  api: AppAnchor[]
  ui: AppUiSlot[]
}

export interface AppsView {
  enabled: boolean
  install_from_web: boolean
  apps: AppView[]
}

export interface AppInstallResult {
  id: string
  name: string
  version: string
  capabilities: string[]
  warning?: string
}

export interface AppsClient {
  get(): Promise<AppsView>
  install(bundleBase64: string, acceptCapabilities: boolean): Promise<AppInstallResult>
  enable(id: string): Promise<Change>
  disable(id: string): Promise<Change>
  remove(id: string): Promise<Change>
  approve(id: string, acceptCapabilities: boolean): Promise<Change>
}
