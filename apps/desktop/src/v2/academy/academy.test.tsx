/**
 * C42: the Academy as R2App draws it (start=academy) around Academy's own
 * learner pages over the native learner transport: the title row's pills,
 * the frame (the pages beside Your place), the Learning paths row read
 * through the generation's client, guest and account generations, sign-in
 * routed to Settings, the offline pill, and the v2 shell mounting it.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createTRPCReact, getQueryKey } from '@trpc/react-query'
import type { AnyRouter } from '@trpc/server'
import { DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { ACADEMY_ORIGIN, ACCOUNT, GUEST, academySnapshot, scriptedNative as academyNative } from '../../modules/academy/test-support'
import { scriptedNative } from '../../test/native'
import { fxSwitcher } from '../fixtures'
import { resetShellStore } from '../store'
import tokensCss from '../../styles/tokens.css?raw'
import { Academy, PATHS_LIST_KEY, themeFor } from './academy'

beforeEach(() => {
    resetShellStore()
    window.scrollTo = vi.fn() as never
    // The WebView has matchMedia; jsdom does not (the lab runner reads the desktop-width layout).
    window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }) as never
})

const PATH = {
    id: 'path-runtime',
    slug: 'runtime-path',
    title: 'Runtime Security Path',
    tagline: 'Protect prompts and responses at runtime.',
    familyKey: 'prisma-airs',
    iconKey: null,
    tone: 'info',
    courseCount: 1,
    estMinutes: 30,
    unitCount: 4,
    unitsDone: 1,
    locked: false,
    lockReason: null,
}
const GATEWAY = {
    ...PATH,
    id: 'path-gateway',
    slug: 'gateway-path',
    title: 'AI Gateway Path',
    tagline: 'Route and govern model traffic.',
    familyKey: 'gateway',
    unitsDone: 0,
    estMinutes: 45,
    locked: true,
}

/** The path's own page as `paths.get` returns it (the router's shape: the sections and their course cards). */
const PATH_PAGE = {
    id: PATH.id,
    slug: PATH.slug,
    title: PATH.title,
    tagline: 'Protect prompts and responses at runtime.',
    description: 'Scan what reaches the model and what comes back.',
    level: 'associate',
    programId: null,
    iconKey: null,
    tone: 'info',
    sections: [
        {
            id: 'sec-1',
            title: 'Foundations',
            description: 'The scan loop and its verdicts.',
            order: 0,
            courses: [
                {
                    id: 'runtime-security',
                    slug: 'runtime-security',
                    title: 'Runtime Security Foundations',
                    description: 'Scan prompts and responses at runtime.',
                    level: 'associate',
                    coursePurpose: null,
                    iconKey: null,
                    tone: 'info',
                    estMinutes: 150,
                    order: 0,
                    unitCount: 4,
                    unitsDone: 1,
                    moduleCount: 2,
                    modulesComplete: 0,
                    status: 'published',
                    teaser: null,
                },
            ],
        },
    ],
}

const replies = { 'paths.list': [PATH, GATEWAY], 'library.list': [] }

describe('C42 Academy board', () => {
    it('draws the chrome around the guest catalog: the pills, the frame, Your place, the Learning paths row from paths.list, through the guest generation only', async () => {
        const native = academyNative(replies)
        render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={vi.fn()} />)
        expect(screen.getByRole('region', { name: 'Academy' })).toBeInTheDocument()
        expect(screen.queryByRole('heading', { name: 'AI Security Academy' })).toBeNull()
        expect(screen.getByText('Learn each product with your own tenant beside you')).toBeInTheDocument()
        expect(document.querySelector('[data-slot="academy-account"]')).toHaveTextContent('Browsing as a guest')
        expect(document.querySelector('[data-slot="academy-offline-pill"]')).toBeNull()
        // The learner pages in the left card, Academy's own catalog heading.
        const pages = await screen.findByRole('region', { name: 'Academy pages' })
        expect(await within(pages).findByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeInTheDocument()
        expect((await within(pages).findAllByText('Runtime Security Path')).length).toBeGreaterThan(0)
        // Your place: the guest note, Open Settings and Learn only.
        const place = screen.getByRole('region', { name: 'Your place' })
        expect(within(place).getByText('Sign in from Settings to track progress and take knowledge checks.')).toBeInTheDocument()
        expect(within(place).getByRole('button', { name: 'Open Settings' })).toBeInTheDocument()
        const nav = within(place).getByRole('navigation', { name: 'Academy sections' })
        expect(
            within(nav)
                .getAllByRole('link')
                .map((l) => l.textContent)
        ).toEqual(['Learn'])
        expect(within(nav).getByRole('link', { name: 'Learn' })).toHaveAttribute('aria-current', 'page')
        // The Learning paths row: the catalog's paths as cards with their units.
        const paths = await screen.findByRole('region', { name: 'Learning paths' })
        const cards = await within(paths).findAllByRole('link', { name: /^Open / })
        expect(cards.map((c) => c.getAttribute('aria-label'))).toEqual(['Open Runtime Security Path', 'Open AI Gateway Path'])
        expect(cards[0]).toHaveTextContent('1 of 4 units · 30 min')
        expect(within(cards[0]!).getByRole('progressbar', { name: 'Runtime Security Path progress' })).toHaveAttribute('aria-valuenow', '25')
        expect(cards[1]).toHaveTextContent('Locked')
        // Every call went through the guest generation, as public queries.
        expect(native.calls.map((i) => [i.procedure, i.procedureKind, i.learner])).toEqual(expect.arrayContaining([['paths.list', 'query', GUEST]]))
        expect(new Set(native.calls.map((i) => i.learner))).toEqual(new Set([GUEST]))
        expect(document.querySelector('[data-slot="academy-v2"]')).not.toBeNull()
        expect(screen.queryByRole('link', { name: /sign up/i })).toBeNull()
    })

    it('a path card opens the real path page within the destination: read by its slug, titled by the path, named in the trail', async () => {
        const user = userEvent.setup()
        // `paths.get` answers the catalog's slug only (the server's NOT_FOUND for any other).
        const native = academyNative((procedure, _learner, input) =>
            procedure === 'paths.list'
                ? [PATH, GATEWAY]
                : procedure === 'library.list'
                  ? []
                  : procedure === 'paths.get' && (input as { pathSlug?: string })?.pathSlug === PATH.slug
                    ? PATH_PAGE
                    : undefined
        )
        render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={vi.fn()} />)
        const paths = await screen.findByRole('region', { name: 'Learning paths' })
        await user.click(await within(paths).findByRole('link', { name: 'Open Runtime Security Path' }))
        const pages = await screen.findByRole('region', { name: 'Academy pages' })
        expect(await within(pages).findByRole('heading', { level: 1, name: 'Runtime Security Path' })).toBeInTheDocument()
        expect(within(pages).queryByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeNull()
        expect(within(pages).queryByText('Path not found.')).toBeNull()
        expect(within(pages).getByText('Foundations')).toBeInTheDocument()
        expect(within(pages).getByText('Runtime Security Foundations')).toBeInTheDocument()
        const trail = within(pages).getByRole('navigation', { name: 'Academy location' })
        expect(trail).toHaveTextContent('Runtime Security Path')
        const get = native.calls.filter((i) => i.procedure === 'paths.get')
        expect(get.map((i) => JSON.parse(i.input ?? '{}'))).toEqual([{ json: { pathSlug: 'runtime-path' } }])
    })

    it('the Learning paths row reads through the key Academy’s own paths.list query uses, holding the raw reply', () => {
        // tRPC react-query's key for `trpc.paths.list.useQuery()`: `utils.paths.list.invalidate()` (after a course reset
        // or an enrollment change) matches it by prefix, and the embed's prefetch fills the same entry.
        const t = createTRPCReact<AnyRouter>() as unknown as { paths: { list: unknown } }
        const key = (getQueryKey as unknown as (procedure: unknown, input?: unknown, type?: string) => unknown)(t.paths.list, undefined, 'query')
        expect(PATHS_LIST_KEY).toEqual(key)
    })

    it('Your place follows the snapshot while the router stays mounted', async () => {
        const native = academyNative(replies)
        const signedIn = academySnapshot(true, ACCOUNT)
        const view = render(<Academy snapshot={signedIn} client={native.client} onNavigate={vi.fn()} />)
        const pages = await screen.findByRole('region', { name: 'Academy pages' })
        const heading = await within(pages).findByRole('heading', { level: 1 })
        expect(document.querySelector('[data-slot="academy-drafts"]')).toBeNull()
        // The same generation (same learner, same account): the router is not rebuilt; the right card re-renders.
        await act(async () =>
            view.rerender(
                <Academy
                    snapshot={academySnapshot(true, ACCOUNT, { localDrafts: [{ id: 'd1', kind: 'lab', attemptId: 'a1', unitId: 'u1', title: 'Lab 1', savedAt: '2026-10-30T00:00:00Z' } as never] })}
                    client={native.client}
                    onNavigate={vi.fn()}
                />
            )
        )
        expect(document.querySelector('[data-slot="academy-drafts"]')).toHaveTextContent('1 piece of lab work saved on this Mac, not sent')
        expect(within(screen.getByRole('region', { name: 'Academy pages' })).getByRole('heading', { level: 1 })).toBe(heading)
    })

    it('signed in: the learner’s name pill, the account sections, the drafts note; a new generation starts its own cache', async () => {
        const titled = (title: string) => ({ ...PATH, title })
        const native = academyNative((procedure, learner) =>
            procedure === 'paths.list' ? [learner === GUEST ? titled('Guest Path') : titled('Account Path')] : procedure === 'library.list' ? [] : undefined
        )
        const view = render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={vi.fn()} />)
        expect(await screen.findByRole('link', { name: 'Open Guest Path' })).toBeInTheDocument()
        await act(async () =>
            view.rerender(
                <Academy
                    snapshot={academySnapshot(true, ACCOUNT, { localDrafts: [{ id: 'd1', kind: 'lab', attemptId: 'a1', unitId: 'u1', title: 'Lab 1', savedAt: '2026-10-30T00:00:00Z' } as never] })}
                    client={native.client}
                    onNavigate={vi.fn()}
                />
            )
        )
        expect(document.querySelector('[data-slot="academy-account"]')).toHaveTextContent('Ada Lovelace')
        expect(screen.queryByText('Browsing as a guest')).toBeNull()
        expect(screen.queryByRole('link', { name: 'Open Guest Path' })).toBeNull()
        expect(await screen.findByRole('link', { name: 'Open Account Path' })).toBeInTheDocument()
        const place = screen.getByRole('region', { name: 'Your place' })
        expect(within(place).getByText('Signed in as Ada Lovelace. Progress and knowledge checks are kept by Academy.')).toBeInTheDocument()
        const nav = within(place).getByRole('navigation', { name: 'Academy sections' })
        expect(
            within(nav)
                .getAllByRole('link')
                .map((l) => l.textContent)
        ).toEqual(['Home', 'Learn', 'Program', 'Downloads', 'Notifications', 'Settings'])
        expect(document.querySelector('[data-slot="academy-drafts"]')).toHaveTextContent('1 piece of lab work saved on this Mac, not sent')
        const order = native.calls.map((i) => i.learner)
        expect(order.indexOf(ACCOUNT)).toBeGreaterThan(order.lastIndexOf(GUEST))
    })

    it('says when Academy cannot be reached (pill and notice) and counts the downloads kept on this Mac', async () => {
        const native = academyNative(replies)
        const offline = {
            reachable: false,
            state: 'ready' as const,
            downloads: [{ courseId: 'c1', title: 'Runtime basics', status: 'current', version: '1', keptAt: '2026-10-30T00:00:00Z' } as never],
        }
        render(<Academy snapshot={academySnapshot(true, ACCOUNT, offline)} client={native.client} onNavigate={vi.fn()} />)
        expect(document.querySelector('[data-slot="academy-offline-pill"]')).toHaveTextContent('Academy cannot be reached')
        expect(document.querySelector('[data-slot="academy-downloads-pill"]')).toHaveTextContent('1 course offline')
        expect(await screen.findByText(/Academy can't be reached\. Downloaded courses/)).toBeInTheDocument()
    })

    it('routes sign-in to Settings and moves focus to the catalog heading on the way back; routes outside this build say so', async () => {
        const user = userEvent.setup()
        const onNavigate = vi.fn()
        const native = academyNative(replies)
        const view = render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={onNavigate} initialPath='/sign-in' />)
        const pages = await screen.findByRole('region', { name: 'Academy pages' })
        expect(await within(pages).findByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeInTheDocument()
        await user.click(within(pages).getByRole('button', { name: 'Open Settings' }))
        expect(onNavigate).toHaveBeenCalledWith('settings')
        await user.click(within(pages).getByRole('link', { name: 'Back to the catalog' }))
        const heading = await within(pages).findByRole('heading', { level: 1, name: 'What will you take up next?' })
        await waitFor(() => expect(heading).toHaveFocus())
        view.unmount()
        render(<Academy snapshot={academySnapshot(true)} client={native.client} onNavigate={onNavigate} initialPath='/admin' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Not in the desktop app' })).toBeInTheDocument()
    })

    it('rereads the Learning paths row when a unit is completed or a knowledge check is submitted in the same generation, and after an uncertain completion reads Academy back without replaying it (attempt-02 R2)', async () => {
        const user = userEvent.setup()
        const sibling = (id: string, kind: string, order: number, title: string) => ({ id, kind, order, slug: String(order), title, xp: 100, estMinutes: 5, status: 'not_started', locked: false })
        const MODULE = {
            moduleId: 'm1',
            title: 'Runtime module',
            description: '',
            concepts: [],
            estMinutes: 10,
            order: 1,
            unitsDone: 0,
            unitCount: 2,
            percent: 0,
            xpEarned: 0,
            xpTotal: 200,
            complete: false,
            reuseCount: 1,
            resumeUnitId: 'u1',
        }
        const LESSON = {
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
            siblings: [sibling('u1', 'content', 1, 'Only lesson'), sibling('u-kc', 'knowledge_check', 2, 'A check')],
            pathContext: { course: { courseId: 'runtime-security', title: 'Runtime Security Foundations', slug: 'runtime-security' }, moduleTitle: 'Runtime module', modules: [MODULE] },
        }
        const CHECK = { ...LESSON, id: 'u-kc', kind: 'knowledge_check', order: 2, slug: '2', title: 'A check', quizDrawCount: 1 }
        const ATTEMPT = {
            attemptId: '00000000-0000-4000-8000-000000000009',
            unitId: 'u-kc',
            questions: [
                {
                    id: 'q1',
                    type: 'mcq',
                    prompt: 'Which input is untrusted?',
                    options: ['Retrieved content', 'Constants'],
                    weight: 1,
                    scope: 'module',
                    lessonId: null,
                    moduleId: 'm1',
                    objectiveId: 'o1',
                },
            ],
        }
        // Academy's own count of units done on the path: the row must show it after each change.
        let done = 1
        let completes = 0
        const native = academyNative((procedure, _learner, input) => {
            switch (procedure) {
                case 'paths.list':
                    return [{ ...PATH, unitsDone: done }]
                case 'library.list':
                    return []
                case 'units.get':
                    return (input as { unitId?: string })?.unitId === 'u-kc' ? CHECK : LESSON
                case 'units.listByModule':
                    return LESSON.siblings
                case 'units.listByPath':
                    return [MODULE]
                case 'units.complete':
                    completes += 1
                    // The first completion's receipt is lost: Academy may have recorded it.
                    if (completes === 1) return { __result__: { kind: 'failed', failure: 'network', outcome: 'unknown' } }
                    done = 2
                    return { completed: true, award: null }
                case 'units.startKnowledgeCheck':
                    return ATTEMPT
                case 'units.checkKcAnswer':
                    return { correct: true, explanation: 'Yes.', correctValue: 0 }
                case 'units.submitKnowledgeCheck':
                    done = 3
                    return { award: null, scorePercent: 100, passed: true, perObjective: [], results: [{ questionId: 'q1', correct: true }] }
                default:
                    return undefined
            }
        })
        render(<Academy snapshot={academySnapshot(true, ACCOUNT)} client={native.client} onNavigate={vi.fn()} initialPath='/units/u1' />)
        const mounted = document.querySelector('[data-slot="academy-v2"]')
        await screen.findByRole('heading', { level: 1, name: 'Only lesson' })
        const paths = await screen.findByRole('region', { name: 'Learning paths' })
        expect(await within(paths).findByText('1 of 4 units · 30 min')).toBeInTheDocument()
        // The card's summary is the contract's tagline, inside the Learning paths card.
        expect(within(paths).getByText('Protect prompts and responses at runtime.')).toBeInTheDocument()
        const reads = () => native.calls.filter((i) => i.procedure === 'paths.list').length
        expect(reads()).toBe(1)
        // An uncertain completion: Academy's truth is read back (the row too), the completion is not replayed.
        await user.click(screen.getByRole('button', { name: /Finish module|Complete & continue/ }))
        await waitFor(() => expect(screen.getAllByRole('alert').some((a) => a.textContent?.includes('may have recorded'))).toBe(true))
        await waitFor(() => expect(reads()).toBe(2))
        expect(completes).toBe(1)
        expect(within(paths).getByText('1 of 4 units · 30 min')).toBeInTheDocument()
        // An ordinary completion: the row shows the new count without remounting the Academy.
        await user.click(screen.getByRole('button', { name: /Finish module|Complete & continue/ }))
        expect(await within(paths).findByText('2 of 4 units · 30 min')).toBeInTheDocument()
        expect(completes).toBe(2)
        // The knowledge check that follows: its submission moves the count again.
        await user.click(await screen.findByRole('button', { name: 'Start knowledge check' }))
        await user.click(await screen.findByLabelText('Retrieved content', { exact: true }))
        await user.click(screen.getByRole('button', { name: 'Submit' }))
        expect(await within(paths).findByText('3 of 4 units · 30 min')).toBeInTheDocument()
        expect(native.calls.filter((i) => i.procedure === 'units.submitKnowledgeCheck')).toHaveLength(1)
        expect(document.querySelector('[data-slot="academy-v2"]')).toBe(mounted)
    })

    it('rereads the Learning paths row when a desktop lab is submitted in the same generation: after a passed lab, and after an uncertain submission as a readback without replaying it (attempt-03 R2)', async () => {
        const user = userEvent.setup()
        const ATTEMPT = '00000000-0000-4000-8000-0000000000a1'
        const LAB = {
            id: 'u-lab',
            moduleId: 'm1',
            kind: 'lab',
            order: 3,
            slug: '3',
            title: 'Lab unit',
            xp: 100,
            estMinutes: 5,
            status: 'not_started',
            siblings: [],
            pathContext: null,
            locked: false,
            unlockHint: null,
            labKind: 'scan',
            labId: 'lab-1',
            placement: { courseId: 'runtime-security', order: 1 },
            lab: { id: 'lab-1' },
        }
        const attempt = (draftVersion: number) => ({
            attemptId: ATTEMPT,
            accountId: 'user_2fixturelearner',
            labId: 'lab-1',
            courseId: 'runtime-security',
            courseTitle: 'Runtime Security Foundations',
            revision: 'rev-1',
            status: 'in_progress',
            draftVersion,
            draft: { commands: [], steps: {}, debrief: '' },
            result: {},
            conflicts: [],
            locked: false,
            lockReason: null,
            createdAt: '2026-10-30T00:00:00.000Z',
            title: 'Guided lab',
            brief: 'Simulated sandbox brief.',
            mode: 'guided',
            seed: null,
            steps: [],
            triage: null,
            engineVersion: 'editorial-1',
            timed: false,
            estMinutes: 10,
        })
        let done = 1
        let version = 0
        let submits = 0
        const native = academyNative((procedure, _learner, input) => {
            switch (procedure) {
                case 'paths.list':
                    return [{ ...PATH, unitsDone: done }]
                case 'library.list':
                    return []
                case 'units.get':
                    return LAB
                case 'units.listByModule':
                    return []
                case 'labs.get':
                    return { lab: { kind: 'scan', id: 'lab-1', title: 'Block it', brief: 'Brief.' }, progress: {} }
                case 'desktop.attempt.list':
                    return { accountId: 'user_2fixturelearner', attempts: [], nextCursor: null }
                case 'desktop.attempt.start':
                case 'desktop.attempt.get':
                    return attempt(version)
                case 'desktop.draft.save':
                    version += 1
                    return { status: 'saved', draftVersion: version, draft: (input as { draft: unknown }).draft }
                case 'desktop.attempt.submit':
                    submits += 1
                    // The first submission's receipt is lost: Academy may have graded it.
                    if (submits === 1) return { __result__: { kind: 'failed', failure: 'network', outcome: 'unknown' } }
                    done = 4
                    return { passed: true, findings: [], decision: null }
                default:
                    return undefined
            }
        })
        render(<Academy snapshot={academySnapshot(true, ACCOUNT)} client={native.client} onNavigate={vi.fn()} initialPath='/units/u-lab' />)
        const mounted = document.querySelector('[data-slot="academy-v2"]')
        const paths = await screen.findByRole('region', { name: 'Learning paths' })
        expect(await within(paths).findByText('1 of 4 units · 30 min')).toBeInTheDocument()
        const reads = () => native.calls.filter((i) => i.procedure === 'paths.list').length
        expect(reads()).toBe(1)
        await user.click(await screen.findByRole('button', { name: 'Start lab' }, { timeout: 15000 }))
        // An uncertain submission: Academy's truth is read back (the row too); the submission is not replayed by the row.
        await user.click(await screen.findByRole('button', { name: 'Submit lab' }, { timeout: 15000 }))
        await waitFor(() => expect(screen.getAllByRole('alert').some((a) => a.textContent?.includes('may have recorded'))).toBe(true))
        await waitFor(() => expect(reads()).toBe(2))
        expect(submits).toBe(1)
        expect(within(paths).getByText('1 of 4 units · 30 min')).toBeInTheDocument()
        // The lab passes on the next submission (the runner's same submission id): the row shows the new count.
        await user.click(screen.getByRole('button', { name: 'Submit lab' }))
        expect(await within(paths).findByText('4 of 4 units · 30 min')).toBeInTheDocument()
        expect(submits).toBe(2)
        const ids = native.calls.filter((i) => i.procedure === 'desktop.attempt.submit').map((i) => (JSON.parse(i.input ?? '{}') as { json: { submissionId: string } }).json.submissionId)
        expect(new Set(ids).size).toBe(1)
        expect(document.querySelector('[data-slot="academy-v2"]')).toBe(mounted)
    }, 30000)

    it('maps the Rust-owned appearance to the learner host’s theme', () => {
        expect(themeFor('dark')).toBe('gruvbox')
        expect(themeFor('light')).toBe('paper')
        expect(['gruvbox', 'paper']).toContain(themeFor('system'))
    })

    it('keeps the accent focus ring on its section links, current or not: the shell\'s ink-on-accent ring is scoped to the accent-filled sidebar destination (attempt-05 R1)', async () => {
        const css: string = tokensCss
        // The override names the sidebar destination (data-inset-focus) and never a bare aria-current='page'.
        expect(css).toMatch(/\[data-v2-shell\] \[data-inset-focus\]\[aria-current='page'\]:focus-visible \{\s*outline-color: var\(--v2-sel-ink\);/)
        expect(css).not.toMatch(/\[data-v2-shell\] \[aria-current='page'\]:focus-visible/)
        const native = academyNative((procedure) => (procedure === 'paths.list' ? [PATH] : procedure === 'library.list' ? [] : undefined))
        render(<Academy snapshot={academySnapshot(true, ACCOUNT)} client={native.client} onNavigate={vi.fn()} />)
        const place = await screen.findByRole('region', { name: 'Your place' })
        const nav = within(place).getByRole('navigation', { name: 'Academy sections' })
        const current = within(nav).getAllByRole('link').find((l) => l.getAttribute('aria-current') === 'page')
        expect(current).toBeDefined()
        // A current section link is an accent-soft link, not an accent-filled destination: no inset-focus marker on any of them.
        for (const link of within(nav).getAllByRole('link')) expect(link).not.toHaveAttribute('data-inset-focus')
    })

    it('mounts in the v2 shell for the Academy destination (the shell hands the Rust-owned appearance to the learner host through themeFor)', async () => {
        const base = fxSwitcher()
        const snapshot: ShellSnapshot = {
            ...base,
            activeFeature: 'academy',
            learner: GUEST,
            academyOrigin: ACADEMY_ORIGIN,
            academy: { state: 'signedOut' },
            academyOffline: { state: 'signedOut', reachable: true, downloads: [], localDrafts: [] },
        }
        const native = scriptedNative(snapshot)
        render(<App client={new DesktopClient(native.transport)} />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Academy' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="academy-v2"]')).not.toBeNull()
        expect(document.querySelector('[data-slot="academy"]')).toBeNull()
        expect(screen.getByRole('region', { name: 'Your place' })).toBeInTheDocument()
    })
})
