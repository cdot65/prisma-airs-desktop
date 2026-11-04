/**
 * Home as the prototype draws it (R2App start=home, C36), over the
 * production App and a scripted native transport: one card per
 * Administration product from the core's read, the attention list with its
 * core intents, the quick actions, the Explain banner and Refresh.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../app'
import { scriptedNative, type Native } from '../test/native'
import { fxFresh, fxInbox } from './fixtures'
import { resetShellStore, useShellStore } from './store'

async function launch(snapshot: ShellSnapshot): Promise<Native> {
    const native = scriptedNative(snapshot)
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('navigation', { name: 'Destinations' })
    return native
}

const WS = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }

/** The Inbox fixture's tenant on Home, with a Runtime chart and the Gateway's day. */
function fxHomeRead(): ShellSnapshot {
    const base = fxInbox()
    const read = base.overview.read!
    return {
        ...base,
        activeFeature: 'home',
        overview: {
            ...base.overview,
            read: {
                ...read,
                chart: { bucketSeconds: 3600, buckets: [0, 1, 0, 2, 0, 0, 1, 3, 0, 1, 2, 4].map((v, i) => ({ time: `t${i}`, total: 10 + v, violating: v, detections: null })) },
                summary: { sessions: 120, violating: 14, benign: 106, detectionEvents: 14 },
                gateway: { workspace: WS.id, workspaceName: WS.name, requests: 1284, errors: 211, cost: 312, partial: false, usageFailure: null, limitsFailure: null, policies: 2 },
            },
        },
    }
}

const card = (id: string) => document.querySelector(`[data-slot="card"][data-product="${id}"]`) as HTMLElement

beforeEach(() => resetShellStore())

describe('v2 Home', () => {
    it('draws one card per Administration product from the read: Runtime’s detection events and chart, the Gateway’s day, read states, and the two gates', async () => {
        const native = await launch(fxHomeRead())
        expect(screen.getByRole('heading', { level: 1, name: 'Home' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/^6 products · Read finished at \d\d:\d\d · 1 source partial · 2 need action$/)
        const cards = screen.getByRole('list', { name: 'Product cards' })
        expect(within(cards).getAllByRole('listitem')).toHaveLength(6)
        // Runtime Security: the summary's detection events, the chart as bars, the worst attention item as the line below.
        const rt = card('runtimeSecurity')
        expect(rt).toHaveTextContent('AI Runtime Security')
        expect(rt).toHaveTextContent('Connected')
        expect(rt).toHaveTextContent('14')
        expect(rt).toHaveTextContent('detection events')
        expect(rt.querySelectorAll('[aria-hidden="true"] span').length).toBe(12)
        expect(rt).toHaveTextContent('Runtime expires in 5 days')
        // The Gateway: requests and cost from the workspace's day; the error rate flagged above 5%.
        const gw = card('gateway')
        expect(gw).toHaveTextContent('Partial')
        expect(gw).toHaveTextContent('1,284')
        expect(gw).toHaveTextContent('requests · today · $3.12')
        // The worst attention item for the product is its last line; without one, the day's error rate.
        expect(gw).toHaveTextContent('AI Gateway: setup warning')
        await native.push({ ...native.current, inbox: { ...native.current.inbox, items: native.current.inbox.items.filter((e) => e.product !== 'gateway') } })
        expect(card('gateway')).toHaveTextContent('Error rate 16.4% today')
        // A partial read may be rechecked (C30); the admitted recheck is announced for the product.
        expect(screen.getAllByRole('button', { name: 'Recheck' })).toHaveLength(1)
        // Products the core reads without figures carry their read state.
        expect(card('redTeam')).toHaveTextContent('Connecting')
        expect(card('modelSkill')).toHaveTextContent('Connecting')
        // The gated products carry their gate and the New tag, never a figure.
        const ais = card('agenticIdentity')
        expect(ais).toHaveTextContent('Not available')
        expect(ais).toHaveTextContent('New')
        expect(ais).toHaveTextContent('—')
        expect(card('agenticEndpoint')).toHaveTextContent('Not available in this release')
    })

    it('opens a card’s product in Administration through the arrival, never local state', async () => {
        const user = userEvent.setup()
        const native = await launch(fxHomeRead())
        await user.click(card('gateway'))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'administration' })
        expect(useShellStore.getState().arrival).toMatchObject({ feature: 'administration', product: 'gateway', tenant: 'tn-0123456789abcdef' })
        await native.push({ ...native.current, activeFeature: 'administration' })
        const gateway = document.querySelector('[data-slot="admin-product"][data-product="gateway"]') as HTMLElement
        expect(within(gateway).getByRole('button', { name: /AI Gateway/ })).toHaveAttribute('aria-expanded', 'true')
    })

    it('lists what needs attention with the item’s action, Acknowledge and Snooze as core intents, beside the quick actions', async () => {
        const user = userEvent.setup()
        const native = await launch(fxHomeRead())
        const attention = screen.getByRole('region', { name: 'Needs attention' })
        expect(within(attention).getByRole('heading', { level: 2 })).toHaveTextContent('Needs attention 2')
        const rows = within(attention).getAllByRole('listitem')
        expect(rows).toHaveLength(2)
        expect(rows[0]).toHaveTextContent('Runtime expires in 5 days')
        expect(rows[0]).toHaveTextContent('AI Runtime Security')
        await user.click(within(rows[0]!).getByRole('button', { name: 'Acknowledge: Runtime expires in 5 days' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'acknowledgeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'runtime-key:k1' })
        await user.click(within(rows[0]!).getByRole('button', { name: /^Snooze.*Runtime expires in 5 days/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'snoozeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'runtime-key:k1' })
        // The item's own action follows its target.
        await user.click(within(rows[0]!).getByRole('button', { name: 'Open API keys' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'administration' })
        expect(useShellStore.getState().arrival).toMatchObject({ feature: 'administration', resource: 'apiKeys' })
        await user.click(within(attention).getByRole('button', { name: 'Open the Inbox' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'inbox' })
        // Quick actions.
        const quick = screen.getByRole('region', { name: 'Quick actions' })
        expect(within(quick).getAllByRole('button').map((b) => b.textContent)).toEqual([
            expect.stringContaining('Test a prompt'),
            expect.stringContaining('Ask the Harness'),
            expect.stringContaining('Send a request'),
            expect.stringContaining('Keep learning'),
        ])
        await user.click(within(quick).getByRole('button', { name: /Send a request/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'apiClient' })
        await user.click(within(quick).getByRole('button', { name: /Test a prompt/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'administration' })
        expect(useShellStore.getState().arrival).toMatchObject({ feature: 'administration', product: 'runtimeSecurity' })
    })

    it('shows the Explain banner only while the calls are shown, and Refresh reads the tenant and waits while reading', async () => {
        const user = userEvent.setup()
        const native = await launch(fxHomeRead())
        expect(document.querySelector('[data-slot="home-explain"]')).not.toBeNull()
        await native.push({ ...native.current, showCalls: false })
        expect(document.querySelector('[data-slot="home-explain"]')).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Refresh' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'readTenant', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' })
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: { ...native.current.inbox.read, state: 'reading' } } })
        expect(screen.getByRole('button', { name: 'Reading…' })).toBeDisabled()
    })

    it('never turns unknown coverage into an all-clear: a failed chart, a failed usage read, an unknown error count and no workspace are named (R1, R2)', async () => {
        const base = fxHomeRead()
        const read = base.overview.read!
        // A failed chart with a usable key inventory: the Runtime card names the gap, not "no detections".
        const native = await launch({ ...base, inbox: { ...base.inbox, items: [] }, overview: { ...base.overview, read: { ...read, chart: null, chartFailure: 'unavailable', summary: { sessions: null, violating: null, benign: null, detectionEvents: null } } } })
        expect(card('runtimeSecurity')).toHaveTextContent('Sessions chart not read')
        expect(card('runtimeSecurity')).toHaveTextContent('—')
        expect(card('runtimeSecurity')).not.toHaveTextContent('No detections')
        // The Gateway's usage read failed: no "No errors".
        await native.push({ ...native.current, overview: { ...native.current.overview, read: { ...read, gateway: { ...read.gateway!, requests: null, errors: null, cost: null, usageFailure: 'unavailable' } } }, inbox: { ...native.current.inbox, items: [] } })
        expect(card('gateway')).toHaveTextContent('Usage not read · Team A')
        expect(card('gateway')).not.toHaveTextContent('No errors')
        // Requests known, errors unknown: said so.
        await native.push({ ...native.current, overview: { ...native.current.overview, read: { ...read, gateway: { ...read.gateway!, errors: null, usageFailure: null } } } })
        expect(card('gateway')).toHaveTextContent('1,284')
        expect(card('gateway')).toHaveTextContent('Error count unknown · Team A')
        // No workspace chosen: an unknown figure under the request metric, and the gate.
        await native.push({ ...native.current, overview: { ...native.current.overview, read: { ...read, gateway: null } } })
        expect(card('gateway')).toHaveTextContent('—')
        expect(card('gateway')).toHaveTextContent('requests · today')
        expect(card('gateway')).toHaveTextContent('No Gateway workspace chosen')
        expect(card('gateway')).not.toHaveTextContent('0/2')
    })

    it('names the severity of each attention row in words as well as colour and icon (I9, R4)', async () => {
        await launch(fxHomeRead())
        const rows = within(screen.getByRole('region', { name: 'Needs attention' })).getAllByRole('listitem')
        expect(rows[0]!.querySelector('[data-slot="severity"]')).toHaveTextContent('Warning')
        expect(rows.map((r) => r.querySelector('[data-slot="severity"]')?.textContent)).toEqual(['Warning', 'Warning'])
    })

    it('"Test a prompt" opens the Runtime Playground tab itself, even after another Runtime tab was chosen (R3)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxHomeRead())
        const runtime = () => document.querySelector('[data-slot="admin-product"][data-product="runtimeSecurity"]') as HTMLElement
        // Visit Administration first and choose another Runtime tab.
        await user.click(card('runtimeSecurity'))
        await native.push({ ...native.current, activeFeature: 'administration' })
        await user.click(within(runtime()).getByRole('tab', { name: 'API keys' }))
        expect(within(runtime()).getByRole('tab', { name: 'API keys' })).toHaveAttribute('aria-selected', 'true')
        await native.push({ ...native.current, activeFeature: 'home' })
        await user.click(within(screen.getByRole('region', { name: 'Quick actions' })).getByRole('button', { name: /Test a prompt/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'administration' })
        expect(useShellStore.getState().arrival).toMatchObject({ product: 'runtimeSecurity', tab: 'playground' })
        await native.push({ ...native.current, activeFeature: 'administration' })
        // The product's own tab (the Playground panel has tabs of its own).
        expect(within(runtime()).getAllByRole('tab', { name: 'Playground' })[0]).toHaveAttribute('aria-selected', 'true')
    })

    it('a fresh tenant shows first read pending on every card and an empty attention list, never sample data (I12)', async () => {
        await launch({ ...fxFresh(), activeFeature: 'home' })
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toContain('First read pending')
        for (const id of ['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill']) {
            expect(card(id)).toHaveTextContent('Connecting')
            expect(card(id)).toHaveTextContent('First read pending')
            expect(card(id)).toHaveTextContent('—')
        }
        const attention = screen.getByRole('region', { name: 'Needs attention' })
        expect(within(attention).queryAllByRole('listitem')).toHaveLength(0)
        expect(attention).toHaveTextContent('First read pending')
    })
})
