/**
 * Every Academy route the desktop carries, rendered for a signed-in learner
 * over a native core that admits exactly what the real one does: the
 * learner allowlist (with its procedure kinds) and the public queries,
 * refusing anything else before it is sent. Academy never answers here, so
 * only the refusals can surface. No route may show an error toast, and
 * every refusal must be one the truth graph names as refused by design with
 * a reason: two Home queries, and the silent product beacons.
 */
import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import procedures from '@prisma-airs/desktop-contracts/fixtures/academy/learner-procedures.json'
import { AcademyView } from './academy-view'
import { academySnapshot, held, refuse, scriptedNative } from './test-support'

const ADMITTED = new Map<string, string>([
    ...procedures.learner.map((p) => [p.procedure, p.kind] as [string, string]),
    ...procedures.public.map((p) => [p, 'query'] as [string, string]),
])

// Queries tools/academy/truth-graph.mjs lists as refused by design, each
// with the reason the page stays truthful without them.
const REFUSED_QUERIES = new Set(['gamification.firstRotation', 'paths.recommended'])
const REFUSED_MUTATIONS = new Set(['telemetry.track'])

const ROUTES = [
    '/',
    '/learn',
    '/program',
    '/assessment',
    '/assessment/intro',
    '/assessment/identity-practical',
    '/certificate',
    '/certificate/cert-certification',
    '/shift',
    '/dossier',
    '/leaderboard',
    '/settings',
    '/notifications',
    '/shop',
    '/orientation',
    '/onboarding',
]

beforeEach(() => {
    window.scrollTo = vi.fn() as never
    window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }) as never
})
afterEach(() => vi.unstubAllGlobals())

describe('desktop learner routes', () => {
    it.each(ROUTES)('%s sends only admitted procedures and shows no refusal', async (path) => {
        const native = scriptedNative((procedure) => (ADMITTED.has(procedure) ? held() : refuse()))
        render(<AcademyView snapshot={academySnapshot(true)} client={native.client} onNavigate={vi.fn()} initialPath={path} />)
        await new Promise((r) => setTimeout(r, 300))
        const refused = native.calls.filter((c) => !ADMITTED.has(c.procedure))
        const unexplained = refused.filter((c) =>
            c.procedureKind === 'query' ? !REFUSED_QUERIES.has(c.procedure) : !REFUSED_MUTATIONS.has(c.procedure)
        )
        expect(unexplained.map((c) => `${c.procedureKind} ${c.procedure}`)).toEqual([])
        // Admitted procedures are called with their admitted kind.
        for (const c of native.calls.filter((c) => ADMITTED.has(c.procedure))) {
            expect([c.procedure, c.procedureKind]).toEqual([c.procedure, ADMITTED.get(c.procedure)])
        }
        expect(screen.queryByRole('alert')).toBeNull()
    })
})
