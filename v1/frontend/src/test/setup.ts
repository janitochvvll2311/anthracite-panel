import { afterEach } from 'vitest'

if (typeof ResizeObserver === 'undefined') {
  class ResizeObserverStub implements ResizeObserver {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }

  globalThis.ResizeObserver = ResizeObserverStub
}

if (typeof requestAnimationFrame === 'undefined') {
  globalThis.requestAnimationFrame = (callback: FrameRequestCallback) =>
    setTimeout(() => callback(performance.now()), 0) as unknown as number
  globalThis.cancelAnimationFrame = (handle: number) => clearTimeout(handle)
}

afterEach(() => {
  document.body.innerHTML = ''
})
