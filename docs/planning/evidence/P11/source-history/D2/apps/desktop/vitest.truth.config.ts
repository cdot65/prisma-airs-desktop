import { defineConfig } from 'vitest/config'

// The C05 artifact/import-graph and seeded truth-canary check: a production
// build with a module-graph plugin (tools/academy/truth-graph.mjs), kept out
// of the general suite because it builds the app.
export default defineConfig({
    test: {
        environment: 'node',
        include: ['src/**/*.truth.ts'],
        testTimeout: 600_000,
    },
})
