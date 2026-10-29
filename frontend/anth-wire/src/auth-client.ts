export interface AuthState {
  required: boolean
  registered: boolean
  authenticated: boolean
  bootstrap: 'web' | 'closed'
  username: string
}

export interface AuthClient {
  state(): Promise<AuthState>
  login(username: string, password: string): Promise<void>
  register(password: string): Promise<void>
  logout(): Promise<void>
  /** Changes the panel password and revokes the other web sessions. */
  password(current: string, next: string): Promise<void>
}
