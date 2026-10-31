import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { academyAlias, academyDedupe } from './academy-source'

export default defineConfig({
    plugins: [react()],
    resolve: { alias: academyAlias, dedupe: academyDedupe },
    test: {
        environment: 'jsdom',
        globals: true,
        setupFiles: ['./src/test/setup.ts'],
        include: ['src/**/*.test.{ts,tsx}'],
        // Stylesheets are not processed, except one read as text (`?raw`) by a test asserting a rule's scope.
        css: { include: [/\.css\?raw$/] },
    },
})
