/** A minimal EventSource double for the process SSE tests. */
export class FakeEventSource {
  static instances: FakeEventSource[] = []

  readonly url: string
  readonly listeners = new Map<string, Set<(event: Event) => void>>()
  closed = false
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null

  constructor(url: string | URL) {
    this.url = String(url)
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: EventListener): void {
    const listeners = this.listeners.get(type) ?? new Set()
    listeners.add(listener)
    this.listeners.set(type, listeners)
  }

  removeEventListener(type: string, listener: EventListener): void {
    this.listeners.get(type)?.delete(listener)
  }

  close(): void {
    this.closed = true
  }

  open(): void {
    this.onopen?.(new Event('open'))
  }

  emit(type: string, data: unknown): void {
    const payload = typeof data === 'string' ? data : JSON.stringify(data)
    const event = new MessageEvent(type, { data: payload })
    this.listeners.get(type)?.forEach((listener) => listener(event))
  }

  fail(): void {
    this.onerror?.(new Event('error'))
  }

  static reset(): void {
    FakeEventSource.instances.length = 0
  }
}
