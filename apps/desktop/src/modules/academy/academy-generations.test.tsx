/**
 * Academy's learner-scoped state follows the native learner generation
 * through the real shell: a completion moment and a notice of account A do
 * not return after leaving Academy, resetting local data and signing in to
 * the same account again in Settings, and the new generation never watches
 * A's earlier course.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient, type DesktopTransport, type Intent, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { fxSwitcher } from '../../v2/fixtures'

// C44: one presentation; the v2 fixture's finished onboarding puts the shell on screen.
const base = fxSwitcher()
const signedIn = (learner: string, revision: string): ShellSnapshot =>
    ({
        ...base,
        revision,
        learner,
        academy: { state: 'signedIn', displayName: 'Ada Lovelace', accountId: 'user_2fixturelearner', learner },
    }) as ShellSnapshot

const moduleRow = (complete: boolean) => ({
    moduleId: 'm1',
    title: 'BEFORE RESET MODULE',
    description: '',
    concepts: [],
    estMinutes: 5,
    order: 1,
    unitsDone: complete ? 1 : 0,
    unitCount: 1,
    percent: complete ? 100 : 0,
    xpEarned: complete ? 100 : 0,
    xpTotal: 100,
    complete,
    reuseCount: 1,
    resumeUnitId: 'u1',
})
const lesson = {
    id: 'u1',
    moduleId: 'm1',
    kind: 'content',
    order: 1,
    slug: '1',
    title: 'Only lesson',
    xp: 100,
    estMinutes: 5,
    status: 'not_started',
    bodyHtml: '<p>Text.</p>',
    objectives: [],
    inlineChecks: [],
    sourceLessonId: null,
    siblings: [{ id: 'u1', kind: 'content', order: 1, slug: '1', title: 'Only lesson', xp: 100, estMinutes: 5, status: 'not_started', locked: false }],
    pathContext: { course: { courseId: 'course-a', title: 'Course A', slug: 'course-a' }, moduleTitle: 'BEFORE RESET MODULE', modules: [moduleRow(false)] },
}

function scripted() {
    const listeners: ((update: unknown) => void)[] = []
    const dispatched: (Intent & { kind: 'academyCall' })[] = []
    const results = new Map<string, unknown>()
    let snapshot = signedIn('lx-gen1', '1')
    let completed = false
    let completeCalls = 0
    let n = 0
    const reply = (procedure: string): unknown => {
        const data: Record<string, unknown> = {
            'paths.list': [],
            'library.list': [
                { type: 'course', id: 'course-a', slug: 'course-a', title: 'Course A', description: '', durationMinutes: 5, level: 'Associate', courseCount: null, labKind: null, courseSlug: null },
            ],
            'courses.list': [{ id: 'course-a', slug: 'course-a', title: 'Course A', description: '', iconKey: null, estMinutes: 5 }],
            'paths.forCourse': [],
            'enrollment.affectedByCourseRemoval': { certifications: [] },
            'progress.courseSummary': { hasProgress: false, units: 0, quizzes: 0, labs: 0, objectives: 0 },
            'modules.intro': null,
            'units.listByPath': [moduleRow(completed)],
            'units.listByModule': lesson.siblings,
            'units.get': lesson,
        }
        if (procedure === 'units.complete') {
            completeCalls += 1
            if (completeCalls === 1) {
                // The first completion's reply is lost: a notice says it may have applied.
                return { kind: 'failed', failure: 'network', outcome: 'unknown' }
            }
            completed = true
            return { kind: 'data', data: JSON.stringify({ json: { completed: true, award: null } }) }
        }
        return procedure in data
            ? { kind: 'data', data: JSON.stringify({ json: data[procedure] }) }
            : { kind: 'failed', failure: 'network', outcome: 'notSent' }
    }
    const transport: DesktopTransport = {
        async observe(bytes) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') return { kind: 'deliverySession', session: `ds-${'a'.repeat(32)}`, nextSequence: '1' }
            if (body.scope.kind === 'academyCall') return { kind: 'academyCall', result: results.get(body.scope.operation ?? '') }
            return { kind: 'shell', snapshot }
        },
        async dispatch(bytes) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            n += 1
            if (intent.kind === 'academyCall') {
                dispatched.push(intent)
                results.set(`op-${n}`, reply(intent.procedure))
                return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
            }
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: null } }
        },
        openChannel(onMessage) {
            listeners.push(onMessage)
            return { token: `__CHANNEL__:${listeners.length}`, close: () => undefined }
        },
    }
    return {
        transport,
        dispatched,
        push: (next: ShellSnapshot) =>
            act(() => {
                snapshot = next
                listeners.at(-1)?.({ kind: 'shell', snapshot: next })
            }),
    }
}

beforeEach(() => {
    window.scrollTo = vi.fn() as never
})
afterEach(() => localStorage.clear())

describe('Academy state across native generations', () => {
    it('does not bring back an earlier generation after reset and signing in to the same account', async () => {
        const user = userEvent.setup()
        const matchMedia = vi
            .spyOn(window, 'matchMedia')
            .mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() } as never)
        try {
            const s = scripted()
            render(<App client={new DesktopClient(s.transport)} />)
            // C44: one presentation; the core names the destination in the snapshot.
            await screen.findByRole('navigation', { name: 'Destinations' })
            s.push({ ...signedIn('lx-gen1', '2'), activeFeature: 'academy' })
            await user.click(await screen.findByRole('button', { name: 'Courses', pressed: false }))
            await user.click(await screen.findByRole('link', { name: /Course A/ }))
            await user.click(await screen.findByRole('link', { name: /Only lesson/ }))
            await screen.findByRole('heading', { level: 1, name: 'Only lesson' })
            const finish = screen.getByRole('button', { name: /Finish module|Complete & continue/ })
            await user.click(finish)
            await waitFor(() => expect(screen.getAllByRole('alert').some((a) => a.textContent?.includes('may have recorded'))).toBe(true))
            await user.click(finish)
            const overlay = () => document.querySelector('[data-slot="module-secure-overlay"]')
            await waitFor(() => expect(overlay()).toHaveTextContent('Module secure: BEFORE RESET MODULE'), { timeout: 5000 })

            // Settings: reset local data, then sign in to the same account.
            s.push({ ...signedIn('lx-gen1', '3'), activeFeature: 'settings', storage: 'resetting' } as ShellSnapshot)
            s.push({ ...base, revision: '4', learner: 'lx-gen2', academy: { state: 'signedOut' }, activeFeature: 'settings' } as ShellSnapshot)
            s.push({ ...signedIn('lx-gen3', '5'), activeFeature: 'settings' })
            const mark = s.dispatched.length

            s.push({ ...signedIn('lx-gen3', '6'), activeFeature: 'academy' })
            await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })
            await new Promise((r) => setTimeout(r, 150))
            expect(overlay()).toBeNull()
            expect(document.body.textContent).not.toContain('BEFORE RESET MODULE')
            expect(screen.queryAllByRole('alert').some((a) => a.textContent?.includes('may have recorded'))).toBe(false)
            const later = s.dispatched.slice(mark)
            expect(later.every((i) => i.learner === 'lx-gen3')).toBe(true)
            expect(later.filter((i) => i.procedure === 'units.listByPath')).toEqual([])
        } finally {
            matchMedia.mockRestore()
        }
    }, 30000)
})
