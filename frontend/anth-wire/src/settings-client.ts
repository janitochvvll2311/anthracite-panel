import type { Rebooting, Restarting } from './actions'

export interface Settings {
  version: number
  socket_path: string
  state_dir: string
  server: { addr: string }
  log: { level: string; format: string; out: string }
  terminal: { enabled: boolean; shell: string; max_sessions: number }
  process: { control: boolean; spawn: boolean }
  network: { control: boolean; manage: boolean }
  firewall: { control: boolean }
  volumes: { control: boolean; partition: boolean; format: boolean }
  accounts: { control: boolean; manage_passwords: boolean }
  files: { write: boolean; trash: boolean }
  auth: { enabled: boolean; username: string; cookie_secure: boolean; bootstrap: string }
  apps: { enabled: boolean }
}

export interface SessionSummary {
  sid: string
  expires: number
  current: boolean
}

export interface SessionsView {
  sessions: SessionSummary[]
  max: number
}

export interface SettingsClient {
  get(): Promise<Settings>
  update(key: string, value: unknown): Promise<{ restart_required: boolean }>
  restart(): Promise<Restarting>
  reboot(delaySeconds: number): Promise<Rebooting>
  sessions(): Promise<SessionsView>
  revokeOthers(): Promise<number>
}
