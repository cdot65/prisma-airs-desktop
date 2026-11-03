import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { desktopAliases, academyDedupe } from './academy-source'

export default defineConfig({
    plugins: [react()],
    resolve: { alias: desktopAliases, dedupe: academyDedupe },
    test: {
        environment: 'jsdom',
        globals: true,
        setupFiles: ['./src/test/setup.ts'],
        include: ['src/**/*.test.{ts,tsx}'],
        // C45 ports maintained helpers/tests; approved retired legacy source is archived in evidence.
        exclude: ['**/node_modules/**'],
        // Stylesheets are not processed, except one read as text (`?raw`) by a test asserting a rule's scope.
        css: { include: [/\.css\?raw$/] },
    },
})
