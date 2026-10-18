/**
 * The Academy destination hosting Academy's own learner pages over the
 * native learner transport: guest catalog, account generations, learner
 * routes the desktop does and does not carry, heading focus after
 * navigation, and presentation storage forgotten by a completed reset.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient, type Intent, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import shellResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-shell-response.json'
import { AcademyView } from './academy-view'
import { forgetAcademyPresentation } from './presentation-storage'

const GUEST = 'lx-aaaaaaaaaaaaaaaaaaaaaaaa'
const ACCOUNT = 'lx-bbbbbbbbbbbbbbbbbbbbbbbb'
const base = shellResponse.snapshot as ShellSnapshot

const PATH = {
    id: 'path-runtime',
    slug: 'runtime-path',
    title: 'Runtime Security Path',
    description: 'Protect prompts and responses at runtime.',
    familyKey: 'prisma-airs',
    iconKey: null,
    tone: 'info',
    courseCount: 1,
    estMinutes: 30,
    unitCount: 4,
    unitsDone: 0,
    locked: false,
    lockReason: null,
}

/** Native core answering learner calls by procedure (SuperJSON `json`). */
function native(replies: Record<string, unknown> = {}) {
    const dispatched: (Intent & { kind: 'academyCall' })[] = []
    const results = new Map<string, unknown>()
    let n = 0
    const client = new DesktopClient({
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') {
                return { kind: 'deliverySession', session: `ds-${'e'.repeat(32)}`, nextSequence: '1' }
            }
            return { kind: 'academyCall', result: results.get(body.scope.operation ?? '') }
        },
        async dispatch(bytes: Uint8Array) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent & { kind: 'academyCall' } })
                .intent
            dispatched.push(intent)
            n += 1
            const reply = { paths: { list: [PATH] }, library: { list: [] }, ...replies }
            const value = intent.procedure
                .split('.')
                .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], reply)
            results.set(
                `op-${n}`,
                value === undefined
                    ? { kind: 'failed', failure: 'network', outcome: 'notSent' }
                    : { kind: 'data', data: JSON.stringify({ json: value }) }
            )
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: '__CHANNEL__:1', close: () => undefined }),
    })
    return { client, dispatched }
}

/** Like native(), but the replies depend on the calling generation. */
function nativeByLearner(replies: (learner: string) => Record<string, unknown>) {
    const dispatched: (Intent & { kind: 'academyCall' })[] = []
    const results = new Map<string, unknown>()
    let n = 0
    const client = new DesktopClient({
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') {
                return { kind: 'deliverySession', session: `ds-${'f'.repeat(32)}`, nextSequence: '1' }
            }
            return { kind: 'academyCall', result: results.get(body.scope.operation ?? '') }
        },
        async dispatch(bytes: Uint8Array) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent & { kind: 'academyCall' } })
                .intent
            dispatched.push(intent)
            n += 1
            const value = intent.procedure
                .split('.')
                .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], replies(intent.learner))
            // Scripted native outcomes: a refusal before sending, or a raw
            // AcademyCallResult (server errors, unknown outcomes).
            if (value && typeof value === 'object' && '__refusal__' in value) {
                return { outcome: 'refusal', refusal: (value as { __refusal__: string }).__refusal__ }
            }
            if (value && typeof value === 'object' && '__result__' in value) {
                results.set(`op-${n}`, (value as { __result__: unknown }).__result__)
                return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
            }
            results.set(
                `op-${n}`,
                value === undefined
                    ? { kind: 'failed', failure: 'network', outcome: 'notSent' }
                    : { kind: 'data', data: JSON.stringify({ json: value }) }
            )
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: '__CHANNEL__:1', close: () => undefined }),
    })
    return { client, dispatched }
}

function snapshot(signedIn: boolean, learner = signedIn ? ACCOUNT : GUEST): ShellSnapshot {
    return {
        ...base,
        learner,
        academy: signedIn
            ? { state: 'signedIn', displayName: 'Ada Lovelace', accountId: 'user_2fixturelearner', learner }
            : { state: 'signedOut' },
    } as ShellSnapshot
}

let fetchSpy: ReturnType<typeof vi.fn>
beforeEach(() => {
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    window.scrollTo = vi.fn() as never
    // The WebView has matchMedia; jsdom does not (desktop-width layout).
    window.matchMedia = vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
    }) as never
})
afterEach(() => {
    expect(fetchSpy).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
})

describe('Academy destination', () => {
    it('shows the catalog to a guest through the guest generation only', async () => {
        const { client, dispatched } = native()
        render(<AcademyView snapshot={snapshot(false)} client={client} onNavigate={vi.fn()} />)
        expect(await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeInTheDocument()
        expect(screen.getByText(/Browsing as a guest/)).toBeInTheDocument()
        expect(await screen.findByText('Runtime Security Path')).toBeInTheDocument()
        expect(dispatched.map((i) => [i.procedure, i.procedureKind, i.learner])).toEqual(
            expect.arrayContaining([
                ['paths.list', 'query', GUEST],
                ['library.list', 'query', GUEST],
            ])
        )
        expect(new Set(dispatched.map((i) => i.learner))).toEqual(new Set([GUEST]))
        // Learner chrome stays inside the destination and never offers a
        // web sign-in page.
        expect(document.querySelector('[data-slot="academy"]')).not.toBeNull()
        expect(screen.queryByRole('link', { name: /sign up/i })).toBeNull()
    })

    it('starts a fresh cache and route for each account generation', async () => {
        // Each generation's catalog reply differs, so a reused cache shows.
        const titled = (title: string) => ({ ...PATH, title })
        const replies = new Map([
            [GUEST, titled('Guest Path')],
            [ACCOUNT, titled('Account Path')],
        ])
        const generations = nativeByLearner((learner) => ({ paths: { list: [replies.get(learner)] }, library: { list: [] } }))
        const view = render(
            <AcademyView snapshot={snapshot(false)} client={generations.client} onNavigate={vi.fn()} initialPath='/learn' />
        )
        expect(await screen.findByText('Guest Path')).toBeInTheDocument()
        await act(async () =>
            view.rerender(
                <AcademyView snapshot={snapshot(true)} client={generations.client} onNavigate={vi.fn()} initialPath='/learn' />
            )
        )
        // Nothing from the guest generation's cache is shown to the account.
        expect(screen.queryByText('Guest Path')).toBeNull()
        expect(screen.queryByText(/Browsing as a guest/)).toBeNull()
        expect(await screen.findByText('Account Path')).toBeInTheDocument()
        const order = generations.dispatched.map((i) => i.learner)
        expect(order.indexOf(ACCOUNT)).toBeGreaterThan(order.lastIndexOf(GUEST))
    })

    it('returns to the catalog when the account changes on another page', async () => {
        const { client } = native()
        const view = render(<AcademyView snapshot={snapshot(false)} client={client} onNavigate={vi.fn()} initialPath='/sign-in' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeInTheDocument()
        await act(async () =>
            view.rerender(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/sign-in' />)
        )
        // A new generation starts its own router at the destination's entry path.
        expect(await screen.findByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeInTheDocument()
    })

    it('sends sign-in to Settings and moves focus to the new page heading', async () => {
        const user = userEvent.setup()
        const onNavigate = vi.fn()
        const { client } = native()
        render(<AcademyView snapshot={snapshot(false)} client={client} onNavigate={onNavigate} initialPath='/sign-in' />)
        await user.click(await screen.findByRole('button', { name: 'Open Settings' }))
        expect(onNavigate).toHaveBeenCalledWith('settings')
        await user.click(screen.getByRole('link', { name: 'Back to the catalog' }))
        const heading = await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })
        await waitFor(() => expect(heading).toHaveFocus())
    })

    it('carries Academy redirects and says plainly what this build does not include', async () => {
        const { client, dispatched } = native()
        const view = render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/program' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Not in this desktop build yet' })).toBeInTheDocument()
        expect(screen.getByText(/Lessons and knowledge checks are available now/)).toBeInTheDocument()
        expect(dispatched).toEqual([])
        view.unmount()

        // The retired paths list goes to the catalog, as on Academy web.
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/paths' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeInTheDocument()
    })

    it('resolves a retired course slug before loading anything', async () => {
        const { client, dispatched } = native()
        render(
            <AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/courses/airs-model-security' />
        )
        await waitFor(() => expect(dispatched.length).toBeGreaterThan(0))
        const inputs = dispatched.map((i) => i.input ?? '')
        expect(inputs.some((input) => input.includes('airs-agent-supply-chain'))).toBe(true)
        expect(inputs.some((input) => input.includes('airs-model-security'))).toBe(false)
    })

    it('shows the learner trail as links within the destination', async () => {
        const { client } = native()
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} />)
        const trail = await screen.findByRole('navigation', { name: 'Academy location' })
        expect(within(trail).getByText('Learn')).toHaveAttribute('aria-current', 'page')
    })
})

describe('Academy presentation storage', () => {
    it('forgets only Academy keys', () => {
        localStorage.setItem('academy-completion-moments:user_2fixturelearner', '{"v":1}')
        localStorage.setItem('academy-unit-contents-sidebar', 'closed')
        localStorage.setItem('unrelated', 'kept')
        expect(forgetAcademyPresentation()).toBe(2)
        expect(localStorage.getItem('academy-completion-moments:user_2fixturelearner')).toBeNull()
        expect(localStorage.getItem('academy-unit-contents-sidebar')).toBeNull()
        expect(localStorage.getItem('unrelated')).toBe('kept')
        localStorage.clear()
    })
})

describe('lesson links', () => {
    it('routes learner paths in place and keeps other addresses out of the window', async () => {
        const user = userEvent.setup()
        const unit = {
            id: 'u-links',
            moduleId: 'm1',
            kind: 'content',
            order: 1,
            slug: '1',
            title: 'Links in lessons',
            xp: 10,
            estMinutes: 3,
            status: 'not_started',
            bodyHtml:
                '<p>Read <a href="/learn">the catalog</a>, <a href="https://docs.example.com/airs">the docs</a> or <a href="#later">later</a>.</p><h2 id="later">Later</h2>',
            objectives: [],
            inlineChecks: [],
            sourceLessonId: null,
            siblings: [],
            pathContext: null,
        }
        const { client } = native({ units: { get: unit, listByModule: [] } })
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-links' />)
        await user.click(await screen.findByRole('link', { name: 'the docs' }))
        expect(screen.getByText(/This link goes to docs.example.com, outside the desktop app, and was not opened/)).toBeInTheDocument()
        await user.click(screen.getByRole('link', { name: 'later' }))
        expect(screen.getByRole('heading', { name: 'Links in lessons' })).toBeInTheDocument()
        await user.click(screen.getByRole('link', { name: 'the catalog' }))
        expect(await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeInTheDocument()
        expect(screen.queryByText(/outside the desktop app/)).toBeNull()
    })
})

describe('desktop host capabilities', () => {
    const base = {
        moduleId: 'm1',
        order: 1,
        slug: '1',
        xp: 10,
        estMinutes: 3,
        status: 'not_started',
        siblings: [],
        pathContext: null,
    }

    it('previews a lab instead of mounting a runner', async () => {
        const lab = {
            ...base,
            id: 'u-lab',
            kind: 'lab',
            title: 'Lab unit',
            locked: false,
            unlockHint: null,
            labKind: 'scan',
            labId: 'lab-1',
            placement: { courseId: 'c1', order: 1 },
            lab: { id: 'lab-1' },
        }
        const { client, dispatched } = native({
            units: { get: lab, listByModule: [] },
            labs: { get: { lab: { kind: 'scan', id: 'lab-1', title: 'Block it', brief: 'Brief.' }, progress: {} } },
        })
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-lab' />)
        expect(await screen.findByText(/no lab sandbox was started/, {}, { timeout: 15000 })).toBeInTheDocument()
        expect(dispatched.filter((i) => i.procedure.startsWith('labs.') && i.procedure !== 'labs.get')).toEqual([])
    }, 20000)

    it('shows pre-rendered diagrams without loading the diagram renderer or video', async () => {
        const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg"></svg>')
        const unit = {
            ...base,
            id: 'u-diagram',
            kind: 'content',
            title: 'Diagram lesson',
            bodyHtml: `<p>Intro.</p><div class="mermaid-diagram" data-mermaid data-diagram-hash="0123456789abcdef" data-diagram-light="${svg}" data-diagram-dark="${svg}">graph LR; A--&gt;B</div>`,
            objectives: [],
            inlineChecks: [],
            sourceLessonId: null,
        }
        const { client, dispatched } = native({ units: { get: unit, listByModule: [] } })
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-diagram' />)
        const img = await screen.findByRole('img', { name: 'Lesson diagram' })
        expect(img.getAttribute('src')).toBe(`data:image/svg+xml;charset=utf-8,${svg}`)
        expect(dispatched.some((i) => i.procedure === 'modules.diagramVideo')).toBe(false)
    })
})

describe('page failures', () => {
    it('shows a plain failure view without the error text when a learner page throws', async () => {
        const quiet = vi.spyOn(console, 'error').mockImplementation(() => undefined)
        const unit = { id: 'u-bad', kind: 'content', title: 'Broken', moduleId: 'm1', bodyHtml: null, siblings: 'not-a-list' }
        const { client } = native({ units: { get: unit, listByModule: [] } })
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-bad' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'This page could not be shown' })).toBeInTheDocument()
        expect(screen.queryByText(/Something went wrong/)).toBeNull()
        expect(screen.queryByText(/is not a function|undefined|TypeError/)).toBeNull()
        expect(screen.getByRole('link', { name: 'Back to the catalog' })).toBeInTheDocument()
        quiet.mockRestore()
    })
})

describe('fixture-only measurement', () => {
    it('offers the learner call probe only in fixture builds', async () => {
        const { client } = native()
        const view = render(<AcademyView snapshot={{ ...snapshot(true), fixtureBuild: false }} client={client} onNavigate={vi.fn()} />)
        await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })
        expect(screen.queryByRole('button', { name: 'Run learner call probe' })).toBeNull()
        view.unmount()
        render(<AcademyView snapshot={{ ...snapshot(true), fixtureBuild: true }} client={client} onNavigate={vi.fn()} />)
        expect(await screen.findByRole('button', { name: 'Run learner call probe' })).toBeInTheDocument()
    })
})

describe('learner effects across generations', () => {
    it('never shows or watches a previous account completion in the next generation', async () => {
        const user = userEvent.setup()
        const COURSE_ID = 'course-a'
        const moduleRow = (complete: boolean) => ({
            moduleId: 'm1',
            title: 'ACCOUNT A PRIVATE MODULE',
            description: '',
            concepts: [],
            estMinutes: 5,
            order: 1,
            unitsDone: complete ? 2 : 1,
            unitCount: 2,
            percent: complete ? 100 : 50,
            xpEarned: complete ? 200 : 100,
            xpTotal: 200,
            complete,
            reuseCount: 1,
            resumeUnitId: 'u-last',
        })
        let completed = false
        const unit = {
            id: 'u-last',
            moduleId: 'm1',
            kind: 'content',
            order: 2,
            slug: '2',
            title: 'Last lesson',
            xp: 100,
            estMinutes: 3,
            status: 'in_progress',
            bodyHtml: '<p>The end.</p>',
            objectives: [],
            inlineChecks: [],
            sourceLessonId: null,
            siblings: [],
            pathContext: { course: { courseId: COURSE_ID, title: 'Course A', slug: 'course-a' }, moduleTitle: 'M', modules: [moduleRow(false)] },
        }
        const replies = (learner: string) => ({
            units: {
                get: unit,
                listByModule: [],
                listByPath: learner === ACCOUNT ? [moduleRow(completed)] : [],
                complete: { completed: true, award: null },
            },
            paths: { list: [PATH] },
            library: { list: [] },
        })
        const { client, dispatched } = nativeByLearner(replies)
        const view = render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-last' />)
        await screen.findByRole('heading', { level: 1, name: 'Last lesson' })
        await waitFor(() => expect(dispatched.some((i) => i.procedure === 'units.listByPath')).toBe(true))
        completed = true
        await user.click(screen.getByRole('button', { name: /Finish module|Complete & continue/ }))
        const overlay = () => document.querySelector('[data-slot="module-secure-overlay"]')
        await waitFor(() => expect(overlay()).toHaveTextContent('Module secure: ACCOUNT A PRIVATE MODULE'), { timeout: 5000 })

        // Signed out: the guest generation shows nothing of account A.
        await act(async () =>
            view.rerender(<AcademyView snapshot={snapshot(false)} client={client} onNavigate={vi.fn()} initialPath='/learn' />)
        )
        await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })
        expect(overlay()).toBeNull()
        expect(document.body.textContent).not.toContain('ACCOUNT A PRIVATE MODULE')
        view.unmount()

        // Another account, entering Academy again: no celebration and no
        // completion watch on account A's course.
        const OTHER = 'lx-cccccccccccccccccccccccc'
        const mark = dispatched.length
        render(
            <AcademyView
                snapshot={{
                    ...snapshot(true, OTHER),
                    academy: { state: 'signedIn', displayName: 'Grace Hopper', accountId: 'user_2other', learner: OTHER },
                } as ShellSnapshot}
                client={client}
                onNavigate={vi.fn()}
                initialPath='/learn'
            />
        )
        await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' })
        await new Promise((r) => setTimeout(r, 100))
        expect(overlay()).toBeNull()
        expect(document.body.textContent).not.toContain('ACCOUNT A PRIVATE MODULE')
        const later = dispatched.slice(mark)
        expect(later.filter((i) => i.procedure === 'units.listByPath')).toEqual([])
        expect(later.every((i) => i.learner === OTHER)).toBe(true)
    }, 20000)
})

describe('learner failures through the desktop transport', () => {
    const lesson = {
        id: 'u-read',
        moduleId: 'm1',
        kind: 'content',
        order: 1,
        slug: '1',
        title: 'A lesson',
        xp: 100,
        estMinutes: 3,
        status: 'not_started',
        bodyHtml: '<p>Text.</p>',
        objectives: [],
        inlineChecks: [],
        sourceLessonId: null,
        siblings: [
            { id: 'u-read', kind: 'content', order: 1, slug: '1', title: 'A lesson', xp: 100, estMinutes: 3, status: 'not_started', locked: false },
            { id: 'u-next', kind: 'content', order: 2, slug: '2', title: 'Next', xp: 100, estMinutes: 3, status: 'not_started', locked: false },
        ],
        pathContext: null,
    }

    it('asks a guest to sign in when completing a unit, which the desktop never sends', async () => {
        const user = userEvent.setup()
        const { client, dispatched } = nativeByLearner(() => ({
            units: { get: lesson, listByModule: [], complete: { __refusal__: 'capabilityMissing' } },
        }))
        render(<AcademyView snapshot={snapshot(false)} client={client} onNavigate={vi.fn()} initialPath='/units/u-read' />)
        await user.click(await screen.findByRole('button', { name: /Complete & continue/ }))
        expect(await screen.findByText('Sign in to Academy to save your progress. You can keep reading as a guest.')).toBeInTheDocument()
        expect(screen.getAllByRole('alert').some((a) => a.textContent === 'Sign in to Academy to save your progress.')).toBe(true)
        expect(screen.getByRole('heading', { level: 1, name: 'A lesson' })).toBeInTheDocument()
        expect(dispatched.filter((i) => i.procedure === 'units.complete')).toHaveLength(1)
    })

    it('says a knowledge-check submission may have been recorded when the server fails after receiving it', async () => {
        const user = userEvent.setup()
        const check = { ...lesson, id: 'u-kc', kind: 'knowledge_check', title: 'A check', quizDrawCount: 1 }
        const attempt = {
            attemptId: '00000000-0000-4000-8000-000000000009',
            unitId: 'u-kc',
            questions: [{ id: 'q1', type: 'mcq', prompt: 'Which input is untrusted?', options: ['Retrieved content', 'Constants'], weight: 1, scope: 'module', lessonId: null, moduleId: 'm1', objectiveId: 'o1' }],
        }
        const serverFailure = {
            kind: 'error',
            error: JSON.stringify({ json: { message: 'Internal server error', code: -32603, data: { code: 'INTERNAL_SERVER_ERROR', httpStatus: 500, path: 'units.submitKnowledgeCheck' } } }),
            httpStatus: 500,
            outcome: 'unknown',
        }
        const { client, dispatched } = nativeByLearner(() => ({
            units: { get: check, listByModule: [], startKnowledgeCheck: attempt, submitKnowledgeCheck: { __result__: serverFailure } },
        }))
        render(<AcademyView snapshot={snapshot(true)} client={client} onNavigate={vi.fn()} initialPath='/units/u-kc' />)
        await user.click(await screen.findByRole('button', { name: 'Start knowledge check' }))
        await user.click(await screen.findByLabelText('Retrieved content', { exact: true }))
        const reads = dispatched.filter((i) => i.procedure === 'units.get').length
        await user.click(screen.getByRole('button', { name: 'Submit' }))
        expect(await screen.findByText(/Academy may have recorded this submission/)).toBeInTheDocument()
        expect(screen.getByLabelText('Retrieved content', { exact: true })).toBeChecked()
        expect(screen.queryByText('Internal server error')).toBeNull()
        // What Academy recorded is read back (the unit's progress), not resubmitted.
        await waitFor(() => expect(dispatched.filter((i) => i.procedure === 'units.get').length).toBeGreaterThan(reads))
        expect(dispatched.filter((i) => i.procedure === 'units.submitKnowledgeCheck')).toHaveLength(1)
    })
})
