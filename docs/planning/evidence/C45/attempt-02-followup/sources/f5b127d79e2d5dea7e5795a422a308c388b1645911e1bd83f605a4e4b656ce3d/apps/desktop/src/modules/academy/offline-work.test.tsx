/**
 * Offline Academy work in the destination: the Downloads page for a signed-in
 * account (download, update and remove through the core's offline intents,
 * with each failure said plainly), its absence for guests, the notice shown
 * while Academy cannot be reached, and lab work kept on this Mac.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DownloadView } from '@prisma-airs/desktop-contracts'
import { Academy as AcademyView } from '../../v2/academy/academy'
import { ACCOUNT, academySnapshot, result, scriptedNative } from './test-support'

const COURSES = [
    { id: 'runtime-security', slug: 'runtime-security', title: 'Runtime Security', description: '', iconKey: null, estMinutes: 30 },
    { id: 'red-teaming', slug: 'red-teaming', title: 'Red Teaming', description: '', iconKey: null, estMinutes: 45 },
]

const kept = (overrides: Partial<DownloadView> = {}): DownloadView => ({
    courseId: 'runtime-security',
    title: 'Runtime Security',
    revision: 'rev-1',
    downloadedAt: '2026-10-18T12:00:00.000Z',
    leaseExpiresAt: '2026-10-25T12:00:00.000Z',
    status: 'current',
    units: 6,
    firstUnit: 'm-rt-1-lesson-01',
    ...overrides,
})

const ready = (downloads: DownloadView[], extra: Record<string, unknown> = {}) =>
    academySnapshot(true, ACCOUNT, { state: 'ready', downloads, ...extra })

beforeEach(() => {
    window.scrollTo = vi.fn() as never
    window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }) as never
})
afterEach(() => vi.unstubAllGlobals())

describe('Academy downloads', () => {
    it('downloads a course Academy offers through the core and says so', async () => {
        const native = scriptedNative({ 'courses.list': COURSES, '@downloadCourse': { courseId: 'red-teaming', units: 4 } })
        render(<AcademyView snapshot={ready([kept()])} client={native.client} onNavigate={vi.fn()} initialPath='/downloads' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Downloads' })).toBeTruthy()
        expect(screen.getByText(/Videos, walkthroughs and live labs still need Academy/)).toBeTruthy()
        const onMac = screen.getByRole('region', { name: 'On this Mac' })
        expect(within(onMac).getByText('Runtime Security')).toBeTruthy()
        expect(within(onMac).getByText(/Ready offline · 6 units · until/)).toBeTruthy()
        // A kept course is not offered again; a current one needs no update.
        expect(within(onMac).queryByRole('button', { name: 'Update Runtime Security' })).toBeNull()
        // Reading starts at the course's first unit.
        expect(within(onMac).getByRole('link', { name: 'Open Runtime Security' })).toHaveAttribute('href', '/units/m-rt-1-lesson-01')
        const available = screen.getByRole('region', { name: 'Available to download' })
        await within(available).findByText('Red Teaming')
        expect(within(available).queryByText('Runtime Security')).toBeNull()
        await userEvent.click(within(available).getByRole('button', { name: 'Download Red Teaming' }))
        expect(await screen.findByText('Red Teaming is ready offline.')).toBeTruthy()
        expect(native.intents.filter((i) => i.kind === 'downloadCourse')).toEqual([
            { kind: 'downloadCourse', learner: ACCOUNT, courseId: 'red-teaming' },
        ])
    })

    it.each([
        ['notDownloadable', 'Academy does not offer this course to your account for offline use.'],
        ['offlineFull', 'There is not enough room for this course in offline storage. Remove a download and try again.'],
        ['offlineUnavailable', 'Offline storage could not be opened. Check that Keychain access was allowed.'],
        ['invalidResponse', 'The download did not finish. Nothing was kept.'],
    ])('says why a download was not kept (%s)', async (failure, text) => {
        const native = scriptedNative({
            'courses.list': COURSES,
            '@downloadCourse': result({ kind: 'failed', failure, outcome: 'notSent' }),
        })
        render(<AcademyView snapshot={ready([])} client={native.client} onNavigate={vi.fn()} initialPath='/downloads' />)
        await userEvent.click(await screen.findByRole('button', { name: 'Download Runtime Security' }))
        expect(await screen.findByText(text)).toBeTruthy()
    })

    it('updates a course with a newer version, removes one, and offers nothing for a withdrawn one', async () => {
        const native = scriptedNative({ 'courses.list': COURSES, '@downloadCourse': {}, '@removeDownload': {} })
        render(
            <AcademyView
                snapshot={ready([kept({ status: 'outdated' }), kept({ courseId: 'red-teaming', title: 'Red Teaming', status: 'withdrawn' })])}
                client={native.client}
                onNavigate={vi.fn()}
                initialPath='/downloads'
            />
        )
        const onMac = await screen.findByRole('region', { name: 'On this Mac' })
        expect(within(onMac).getByText(/A newer version is on Academy/)).toBeTruthy()
        expect(within(onMac).getByText('No longer part of your account')).toBeTruthy()
        expect(within(onMac).queryByRole('button', { name: 'Update Red Teaming' })).toBeNull()
        expect(within(onMac).queryByRole('link', { name: 'Open Red Teaming' })).toBeNull()
        await userEvent.click(within(onMac).getByRole('button', { name: 'Update Runtime Security' }))
        expect(await screen.findByText('Runtime Security is ready offline.')).toBeTruthy()
        await userEvent.click(within(onMac).getByRole('button', { name: 'Remove Red Teaming' }))
        expect(await screen.findByText('Red Teaming was removed from this Mac.')).toBeTruthy()
        expect(native.intents.filter((i) => i.kind === 'removeDownload')).toEqual([
            { kind: 'removeDownload', learner: ACCOUNT, courseId: 'red-teaming' },
        ])
    })

    it('while Academy is unreachable, says so on every page, offers no new downloads and keeps checking', async () => {
        const native = scriptedNative({ 'courses.list': COURSES })
        const view = render(
            <AcademyView
                snapshot={ready([kept()], { reachable: false, localDrafts: [{ attemptId: 'att_1', baseVersion: 2, savedAt: '2026-10-18T12:00:00.000Z' }] })}
                client={native.client}
                onNavigate={vi.fn()}
                initialPath='/downloads'
            />
        )
        expect(await screen.findByText(/Academy can't be reached/)).toBeTruthy()
        expect(screen.getByText('Connect to Academy to download more courses.')).toBeTruthy()
        expect(screen.getByText('One lab draft is saved on this Mac only. Open its lab to send it.')).toBeTruthy()
        // The course list is still asked for: its reply is how Academy is seen to be back.
        await waitFor(() => expect(native.calls.some((c) => c.procedure === 'courses.list')).toBe(true))
        expect(screen.queryByRole('button', { name: 'Download Red Teaming' })).toBeNull()
        // Back online, the notice goes and the catalog is offered.
        view.rerender(<AcademyView snapshot={ready([kept()])} client={native.client} onNavigate={vi.fn()} initialPath='/downloads' />)
        await waitFor(() => expect(screen.queryByText(/Academy can't be reached/)).toBeNull())
        expect(await screen.findByRole('button', { name: 'Download Red Teaming' })).toBeTruthy()
    })

    it('is not offered to guests, and offline storage that could not open says so', async () => {
        const guest = scriptedNative({})
        const view = render(<AcademyView snapshot={academySnapshot(false)} client={guest.client} onNavigate={vi.fn()} initialPath='/downloads' />)
        expect(await screen.findByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeTruthy()
        expect(screen.queryByRole('link', { name: 'Downloads' })).toBeNull()
        view.unmount()
        const native = scriptedNative({ 'courses.list': COURSES })
        render(
            <AcademyView
                snapshot={academySnapshot(true, ACCOUNT, { state: 'unavailable' })}
                client={native.client}
                onNavigate={vi.fn()}
                initialPath='/downloads'
            />
        )
        expect(await screen.findByRole('link', { name: 'Downloads' })).toBeTruthy()
        expect(await screen.findByRole('alert')).toBeTruthy()
        const button = await screen.findByRole('button', { name: 'Download Runtime Security' })
        expect((button as HTMLButtonElement).disabled).toBe(true)
    })
})
