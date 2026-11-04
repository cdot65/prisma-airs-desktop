import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { desktopAliases, academyDedupe } from './academy-source'

// Packaged assets only: no remote scripts, no sourcemaps in the bundle.
export default defineConfig({
    plugins: [react(), tailwindcss()],
    resolve: { alias: desktopAliases, dedupe: academyDedupe },
    clearScreen: false,
    base: './',
    build: {
        target: 'safari16',
        sourcemap: false,
        outDir: 'dist',
        emptyOutDir: true,
        assetsInlineLimit: 0,
    },
    server: { port: 1420, strictPort: true },
})
