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
        // C44: the legacy shell is gone. These suites drove it through the Features rail and test legacy
        // views no v2 screen mounts; their files carry the owner's uncommitted work (docs/planning/evidence/
        // C28/owner-work.json), so they are retired here rather than edited or deleted on this branch. Their
        // behaviours are covered by the v2 suites (evidence/C44/legacy-script-map.md).
        exclude: [
            '**/node_modules/**',
            'src/features/gateway/gateway-view.test.tsx',
            'src/features/gateway/gateway-inference.test.tsx',
            'src/features/harness/harness-view.test.tsx',
            'src/features/library/library-view.test.tsx',
            // Its assertions run on the one presentation in src/v2/settings/management-credential.test.tsx (C44 R3).
            'src/features/tenants/management-credential.test.tsx',
            'src/features/workbench/workbench-start.test.tsx',
        ],
        // Stylesheets are not processed, except one read as text (`?raw`) by a test asserting a rule's scope.
        css: { include: [/\.css\?raw$/] },
    },
})
