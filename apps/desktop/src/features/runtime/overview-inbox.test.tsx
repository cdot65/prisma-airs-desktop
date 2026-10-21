/**
 * Runtime Overview & Inbox through the real App and shell: a read on
 * opening only when nothing fresh is held (and not again right after a
 * failure), source states that never pass for a healthy zero, Needs
 * attention faces and holds, Inbox filters and navigation.
 */
import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type InboxItem, type Intent, type OverviewView, type RuntimeOverview, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'
import { chartSeries } from './overview-inbox'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const now = () => Math.floor(Date.now() / 1000)

const KEY: InboxItem = {
    id: 'runtime-key:k1',
    title: 'Runtime expires in 5 days',
    detail: 'Review this key and rotate or replace it before its clients lose access.',
    source: 'Runtime Security API keys',
    action: true,
    tone: 'warn',
    destination: 'apiKeys',
    actionLabel: 'Manage API keys',
    facts: [{ label: 'Key', value: 'Runtime' }],
    perMille: null,
}
const ACTIVITY: InboxItem = {
    id: 'runtime-activity',
    title: 'Runtime activity available',
    detail: 'Sessions and violations Runtime Security reported for the last 24 hours.',
    source: 'Runtime Security · last 24h',
    action: false,
    tone: 'ok',
    destination: 'sessions',
    actionLabel: 'Inspect sessions',
    facts: [{ label: 'Sessions', value: '15' }],
    perMille: 0,
}

const read = (overrides: Partial<RuntimeOverview> = {}): RuntimeOverview => ({
    readAt: now(),
    chart: {
        bucketSeconds: 3600,
        buckets: [
            { time: '2026-10-19T00:00:00Z', total: 10, violating: 0, detections: [] },
            { time: '2026-10-19T01:00:00Z', total: 5, violating: 0, detections: [] },
        ],
    },
    chartFailure: null,
    summary: { sessions: 15, violating: 0, benign: 15, detectionEvents: 0 },
    keysComplete: true,
    keysFailure: null,
    items: [KEY, ACTIVITY],
    gaps: [],
    gateway: null,
    ...overrides,
})

const overview = (overrides: Partial<OverviewView> = {}): OverviewView => ({
    read: read(),
    refreshing: false,
    failure: null,
    failedAt: null,
    fresh: true,
    attention: [KEY],
    held: [],
    face: 'items',
    ...overrides,
})

const ready = (view: OverviewView): ShellSnapshot => ({
    ...withTenant(
        baseSnapshot,
        tenant({
            management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1234567890.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
        }),
        CONTEXT
    ),
    overview: view,
})

async function openOverview(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'Scan a prompt or response' })
    await user.click(screen.getByRole('tab', { name: 'Overview & Inbox' }))
    return user
}

const reads = (native: Native) => native.dispatched.filter((i): i is Extract<Intent, { kind: 'management' }> => i.kind === 'management')

describe('Runtime Overview & Inbox', () => {
    it('reads on opening only when nothing fresh is held', async () => {
        const fresh = scriptedNative(ready(overview()))
        await openOverview(fresh)
        expect(reads(fresh)).toEqual([])
        cleanup()
        const stale = scriptedNative(ready(overview({ fresh: false })))
        const user = await openOverview(stale)
        expect(reads(stale).map((r) => r.request)).toEqual([{ kind: 'overview' }])
        expect(screen.getByText(/^Stale · as of/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: /Refresh Runtime insights/ }))
        expect(reads(stale)).toHaveLength(2)
    })

    it('does not read again right after a failure and keeps the last read visible', async () => {
        const native = scriptedNative(ready(overview({ fresh: false, failure: 'network', failedAt: now() - 60, face: 'items' })))
        await openOverview(native)
        expect(reads(native)).toEqual([])
        expect(screen.getByRole('alert')).toHaveTextContent('The last refresh failed')
        expect(screen.getByText(/Showing what was read at/)).toBeInTheDocument()
    })

    it('shows the chosen Gateway workspace’s day with its source state and opens AI Gateway from its item', async () => {
        const GATEWAY: InboxItem = {
            id: 'gateway-errors-16f7e90d-382a-4e78-b577-1b01eb5f8297',
            title: 'Gateway errors in Team A',
            detail: '4 of 40 Gateway requests in the last day failed.',
            source: 'AI Gateway',
            action: true,
            tone: 'bad',
            destination: 'gateway',
            actionLabel: 'Open Gateway logs',
            facts: [{ label: 'Requests', value: '40' }],
            perMille: 100,
        }
        const gateway = {
            workspace: '16f7e90d-382a-4e78-b577-1b01eb5f8297', workspaceName: 'Team A', requests: 40, errors: 4, cost: 120, partial: false,
            usageFailure: null, policies: null, limitsFailure: 'denied' as const,
        }
        const native = scriptedNative(ready(overview({ read: read({ items: [GATEWAY, KEY], gateway }), attention: [GATEWAY, KEY], face: 'items' })))
        const user = await openOverview(native)
        const day = screen.getByRole('region', { name: 'AI Gateway · Team A' })
        expect(within(day).getByText(/Last day: 40 requests · 4 errors · \$1\.20 cost/)).toBeInTheDocument()
        expect(within(day).getByText(/Limit policies unavailable: This management client is not allowed/)).toBeInTheDocument()
        await user.click(within(screen.getByRole('list', { name: 'Insights' })).getByRole('button', { name: /Gateway errors in Team A/ }))
        expect(screen.getByText('10.0% of Gateway requests failed')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Open Gateway logs' }))
        expect(await screen.findByRole('heading', { level: 1, name: 'Gateway workspaces and configuration' })).toBeInTheDocument()
        cleanup()
        // No workspace chosen when read: said so, never a zero.
        const none = scriptedNative(ready(overview({ read: read({ gateway: null }) })))
        await openOverview(none)
        expect(screen.getByText(/No Gateway workspace was chosen when this was read\./)).toBeInTheDocument()
    })

    it('states unknown and failed sources instead of zeros', async () => {
        const native = scriptedNative(
            ready(
                overview({
                    read: read({
                        chart: null,
                        chartFailure: 'unavailable',
                        summary: { sessions: null, violating: null, benign: null, detectionEvents: null },
                        items: [KEY],
                        gaps: ['Runtime activity unavailable'],
                    }),
                    attention: [],
                    held: [{ item: KEY, kind: 'snooze' }],
                    face: 'gap',
                })
            )
        )
        await openOverview(native)
        expect(screen.getByText(/Runtime activity unavailable: The management service is unavailable/)).toBeInTheDocument()
        expect(screen.getByText('Can’t confirm all clear')).toBeInTheDocument()
        expect(within(screen.getByRole('region', { name: 'Needs attention' })).getAllByText('Runtime activity unavailable')).not.toHaveLength(0)
        expect(screen.getByText('Source coverage')).toBeInTheDocument()
        expect(screen.queryByText(/0 sessions/)).toBeNull()
    })

    it('holds and restores attention items and navigates to their evidence', async () => {
        const native = scriptedNative(ready(overview()))
        const user = await openOverview(native)
        const panel = screen.getByRole('region', { name: 'Needs attention' })
        expect(within(panel).getByText('0 critical · 1 warn')).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Acknowledge' }))
        await user.click(within(panel).getByRole('button', { name: 'Snooze 24h' }))
        expect(native.dispatched.filter((i) => i.kind === 'holdInsight')).toEqual([
            { kind: 'holdInsight', context: CONTEXT, id: KEY.id, hold: 'acknowledge' },
            { kind: 'holdInsight', context: CONTEXT, id: KEY.id, hold: 'snooze' },
        ])
        await native.push(ready(overview({ attention: [], held: [{ item: KEY, kind: 'acknowledge' }], face: 'allClear' })))
        expect(screen.getByText('Nothing open: what still needs attention is acknowledged or snoozed')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: `Restore ${KEY.title}` }))
        expect(native.dispatched.filter((i) => i.kind === 'restoreInsight')).toEqual([{ kind: 'restoreInsight', context: CONTEXT, id: KEY.id }])
        // Inbox filters, then the activity item opens Sessions.
        await user.click(screen.getByRole('button', { name: 'Updates 1' }))
        await user.click(screen.getByRole('button', { name: 'Inspect sessions' }))
        await screen.findByRole('heading', { level: 1, name: 'Investigate sessions' })
    })

    it('asks for a management client before reading', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        await openOverview(native)
        expect(screen.getByText(/Configure one in Settings to see them/)).toBeInTheDocument()
        expect(reads(native)).toEqual([])
    })

    it('charts detectors only when every bucket reported them', () => {
        const complete = chartSeries({
            bucketSeconds: 3600,
            buckets: [
                { time: 'a', total: 3, violating: 2, detections: [{ detector: 'pi', count: 2 }, { detector: 'dlp', count: 1 }] },
                { time: 'b', total: 1, violating: 1, detections: [{ detector: 'dlp', count: 3 }] },
            ],
        })
        expect(complete.map((s) => s.name)).toEqual(['dlp', 'pi'])
        const partial = chartSeries({
            bucketSeconds: 3600,
            buckets: [
                { time: 'a', total: 3, violating: 2, detections: null },
                { time: 'b', total: 1, violating: 1, detections: [] },
            ],
        })
        expect(partial).toEqual([{ name: 'Violating sessions', values: [2, 1] }])
    })
})
