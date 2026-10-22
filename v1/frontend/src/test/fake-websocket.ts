/** A minimal WebSocket double shared by the transport tests. */
export class FakeWebSocket {
  static instances: FakeWebSocket[] = []

  readonly url: string
  readonly sent: string[] = []
  readyState = 0
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null

  constructor(url: string | URL) {
    this.url = String(url)
    FakeWebSocket.instances.push(this)
  }

  send(data: string): void {
    this.sent.push(data)
  }

  close(code = 1000, reason = ''): void {
    this.readyState = 3
    this.onclose?.({ code, reason, wasClean: true } as CloseEvent)
  }

  open(): void {
    this.readyState = 1
    this.onopen?.(new Event('open'))
  }

  message(data: unknown): void {
    this.onmessage?.({ data } as MessageEvent)
  }

  static reset(): void {
    FakeWebSocket.instances.length = 0
  }
}
