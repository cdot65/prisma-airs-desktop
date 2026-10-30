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
import { DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { ACADEMY_ORIGIN, ACCOUNT, GUEST, academySnapshot, scriptedNative as academyNative } from '../../features/academy/test-support'
import { scriptedNative } from '../../test/native'
import { fxSwitcher } from '../fixtures'
import { resetShellStore } from '../store'
import { Academy } from './academy'

beforeEach(() => resetShellStore())

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
    unitsDone: 1,
    locked: false,
    lockReason: null,
}
const GATEWAY = {
    ...PATH,
    id: 'path-gateway',
    slug: 'gateway-path',
    title: 'AI Gateway Path',
    description: 'Route and govern model traffic.',
    familyKey: 'gateway',
    unitsDone: 0,
    estMinutes: 45,
    locked: true,
}

const replies = { 'paths.list': [PATH, GATEWAY], 'library.list': [] }

describe('C42 Academy board', () => {
    it('draws the chrome around the guest catalog: the pills, the frame, Your place, the Learning paths row from paths.list, through the guest generation only', async () => {
        const native = academyNative(replies)
        render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={vi.fn()} />)
        expect(screen.getByRole('heading', { level: 1, name: 'AI Security Academy' })).toBeInTheDocument()
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

    it('a path card opens its page within the destination and the trail shows the place', async () => {
        const user = userEvent.setup()
        const native = academyNative({ ...replies, 'paths.get': { ...PATH, courses: [] } })
        render(<Academy snapshot={academySnapshot(false)} client={native.client} onNavigate={vi.fn()} />)
        const paths = await screen.findByRole('region', { name: 'Learning paths' })
        await user.click(await within(paths).findByRole('link', { name: 'Open Runtime Security Path' }))
        const pages = await screen.findByRole('region', { name: 'Academy pages' })
        await waitFor(() => expect(within(pages).queryByRole('heading', { level: 1, name: 'What will you take up next?' })).toBeNull())
        expect(native.calls.some((i) => i.procedure.startsWith('paths.'))).toBe(true)
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

    it('mounts in the v2 shell for the Academy destination with the Rust-owned theme', async () => {
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
        expect(await screen.findByRole('heading', { level: 1, name: 'AI Security Academy' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="academy-v2"]')).not.toBeNull()
        expect(document.querySelector('[data-slot="academy"]')).toBeNull()
        expect(screen.getByRole('region', { name: 'Your place' })).toBeInTheDocument()
    })
})
