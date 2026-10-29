import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'happy-dom',
    include: ['anth-*/src/**/*.test.ts'],
    clearMocks: true,
    restoreMocks: true,
    unstubGlobals: true,
  },
})
