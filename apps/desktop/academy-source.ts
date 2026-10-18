/**
 * Academy learner source compiled into the desktop (C05). The learner
 * package (@ai-security-academy/learner) exports Academy's own pages; their
 * `@/` imports are Academy's source in the prepared, hash-verified input
 * (.work/academy, xtask shared-prepare). Every bare import that source makes
 * resolves to this app's pinned copy (`dedupe`), so one React, one React
 * Query and one router instance exist and versions follow the desktop lock.
 */
// A file-system path without Node typings in the app's tsconfig (the
// repository path has no characters that URL-encode other than these).
export const ACADEMY_SRC = decodeURIComponent(new URL('../../.work/academy/src', import.meta.url).pathname)

export const academyAlias = [{ find: /^@\//, replacement: `${ACADEMY_SRC}/` }]

export const academyDedupe = [
    'react',
    'react-dom',
    '@ai-security-academy/presentation',
    '@ai-security-academy/shared',
    '@codemirror/commands',
    '@codemirror/lang-python',
    '@codemirror/lang-yaml',
    '@codemirror/language',
    '@codemirror/state',
    '@codemirror/view',
    '@dnd-kit/core',
    '@excalidraw/excalidraw',
    '@lezer/highlight',
    '@radix-ui/react-dialog',
    '@radix-ui/react-slot',
    '@tanstack/react-query',
    '@tanstack/react-router',
    '@trpc/client',
    '@trpc/react-query',
    '@trpc/server',
    'class-variance-authority',
    'clsx',
    'dompurify',
    'lucide-react',
    'mermaid',
    'qrcode.react',
    'sanitize-html',
    'tailwind-merge',
    'zustand',
]
