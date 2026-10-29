/**
 * The Inbox and Home attention (C30) over the scripted native transport:
 * the entries come from the core's Inbox scope, holds are core intents whose
 * return time the core states (I11), completion toasts follow the core's
 * ReadStatus (I4), a fresh tenant shows pending and nothing else (I12), the
 * switch guard renders from the refusal payload (I1), and the Inbox
 * selection survives a tenant switch round trip (I2).
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DeliveryOutcomeUnknown, DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { scriptedNative, type Native } from '../../test/native'
import { fxFresh, fxInbox } from '../fixtures'
import { resetShellStore } from '../store'

async function launch(snapshot: ShellSnapshot): Promise<Native> {
    const native = scriptedNative(snapshot)
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1 })
    return native
}
const toasts = () => Array.from(document.querySelectorAll('[data-slot="toast"]')).map((t) => t.textContent ?? '')

beforeEach(() => resetShellStore())

describe('Inbox', () => {
    it('lists the core entries in tabs with counts, opens the detail pane, and acts through core intents', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        expect(screen.getByRole('heading', { level: 1, name: 'Inbox' })).toBeInTheDocument()
        const tabs = screen.getAllByRole('tab').map((t) => t.textContent)
        expect(tabs).toEqual(['All 4', 'Needs action 2', 'Updates 1', 'Snoozed 1'])
        expect(screen.getByText(/3 of 4 sources were read at \d\d:\d\d\./)).toBeInTheDocument()
        await user.click(screen.getByRole('tab', { name: /Needs action/ }))
        const rows = within(screen.getByRole('list', { name: 'Inbox items' })).getAllByRole('button')
        expect(rows.map((r) => r.textContent)).toEqual([expect.stringContaining('Runtime expires in 5 days'), expect.stringContaining('AI Gateway: setup warning')])
        await user.click(rows[0] as HTMLElement)
        const detail = screen.getByRole('complementary', { name: 'Details' })
        expect(within(detail).getByRole('heading', { level: 2, name: 'Runtime expires in 5 days' })).toBeInTheDocument()
        expect(within(detail).getByRole('row', { name: /Expires/ })).toBeInTheDocument()
        expect(within(detail).getByText(/Why this matters/)).toBeInTheDocument()
        await user.click(within(detail).getByRole('button', { name: 'Open API keys' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'administration' })
        // The action carries its target: Administration opens on the API keys inventory, filtered to the key.
        const ready = native.current.tenants.map((t, i) => (i === 0 ? { ...t, management: { state: 'ready' as const, mode: 'keychain' as const, failure: null, clientId: 'client@1762204418.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } } : t))
        await native.push({ ...native.current, activeFeature: 'administration', tenants: ready })
        await screen.findByRole('heading', { level: 1, name: 'Administration' })
        await waitFor(() => expect(screen.getByRole('tab', { name: 'API keys' })).toHaveAttribute('aria-selected', 'true'))
        await waitFor(() => expect(document.querySelector('[data-slot="administration-filter"]')).toHaveValue('Runtime'))
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Runtime expires in 5 days/ }))
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Acknowledge' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'acknowledgeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'runtime-key:k1' })
    })

    it('snoozes through the core and states the return time the core computed, the same on Home (I11)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        await user.click(screen.getByRole('button', { name: /Runtime expires in 5 days/ }))
        const detail = screen.getByRole('complementary', { name: 'Details' })
        await user.click(within(detail).getByRole('button', { name: 'Snooze 24 hours: Runtime expires in 5 days' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'snoozeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'runtime-key:k1' })
        // The core answers with the return time and a receipt; the row, the pane and the toast all state it.
        const snoozed = native.current.inbox.items.map((e) => (e.id === 'runtime-key:k1' ? { ...e, hold: { kind: 'snooze' as const, returnsAt: 1_761_576_400 } } : e))
        await native.push({ ...native.current, inbox: { ...native.current.inbox, items: snoozed, lastSnooze: { id: 'runtime-key:k1', title: 'Runtime expires in 5 days', returnsAt: 1_761_576_400, sequence: 1 } } })
        const returns = (await within(screen.getByRole('complementary', { name: 'Details' })).findByText(/^Returns /)).textContent
        expect(returns).toMatch(/^Returns (today|tomorrow|\w+ \d+) \d\d:\d\d$/)
        await user.click(screen.getByRole('tab', { name: /Snoozed/ }))
        const row = screen.getByRole('button', { name: /Runtime expires in 5 days/ })
        expect(row.textContent).toContain(returns)
        await waitFor(() => expect(toasts().join(' ')).toContain(`Runtime expires in 5 days snoozed · returns ${returns?.replace('Returns ', '')}`))
        // Home offers the same snooze label for the same item.
        await native.push({ ...native.current, activeFeature: 'home' })
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        expect(screen.getByRole('button', { name: 'Snooze 24 hours: AI Gateway: setup warning' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="attention"]')?.textContent ?? '').not.toContain('Runtime expires in 5 days')
    })

    it('derives completion toasts from the core read status: warn when partial, ok when complete (I4)', async () => {
        const native = await launch(fxInbox())
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: { ...native.current.inbox.read, generation: 2 } } })
        await waitFor(() => expect(toasts().join(' ')).toMatch(/Read finished at \d\d:\d\d · 1 source partial/))
        expect(document.querySelector('[data-slot="toast"]')?.getAttribute('data-kind')).toBe('warn')
        const complete = { ...native.current.inbox.read, generation: 3, sourcesMissing: [], products: native.current.inbox.read.products.map((p) => ({ ...p, state: 'ok' as const, sourcesRead: 2 })) }
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: complete } })
        await waitFor(() => expect(toasts().join(' ')).toMatch(/Read finished at \d\d:\d\d · all sources complete/))
        expect(document.querySelector('[data-slot="toast"]')?.getAttribute('data-kind')).toBe('ok')
    })

    it('a fresh tenant shows first read pending and an empty Inbox, never sample data (I12)', async () => {
        const native = await launch(fxFresh())
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toContain('First read pending')
        expect(document.querySelector('[data-slot="attention"]')).toBeNull()
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toContain('First read pending')
        expect(screen.queryByRole('list', { name: 'Inbox items' })).toBeNull()
    })

    it('renders the switch guard from the refusal payload and re-dispatches with the chosen resolution (I1)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        native.respond = () => ({ outcome: 'refusal', refusal: 'decisionPending' })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: /Meridian Retail/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: 'tn-fedcba9876543210', resolution: null })
        expect(toasts()).toEqual([])
        await native.push({ ...native.current, switchBlocked: { to: 'tn-fedcba9876543210', pending: { conversation: 'op-7', decision: 'ap-1' }, completion: false } })
        const guard = await screen.findByRole('dialog', { name: 'A Harness decision is waiting' })
        expect(within(guard).getByText(/Switching to Meridian Retail ends it/)).toBeInTheDocument()
        // Initial focus is Stay, the safe default: Enter on arrival ends nothing.
        await waitFor(() => expect(within(guard).getByRole('button', { name: 'Stay' })).toHaveFocus())
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(guard).getByRole('button', { name: 'Stay' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'dismissSwitchGuard' })
        await native.push({ ...native.current, switchBlocked: { to: 'tn-fedcba9876543210', pending: { conversation: 'op-7', decision: 'ap-1' }, completion: false } })
        await user.click(within(await screen.findByRole('dialog', { name: 'A Harness decision is waiting' })).getByRole('button', { name: 'Archive and switch' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: 'tn-fedcba9876543210', resolution: 'archive' })
    })

    it('announces a snooze once, from the core receipt: never an old hold after a switch or a reread (I11)', async () => {
        const native = await launch(fxInbox())
        // Meridian already holds a snoozed item: switching there is not a new snooze.
        const meridian = native.current.tenants[1]!
        const held = { ...native.current.inbox.items[2]!, id: 'gateway-ws-9', title: 'B already snoozed' }
        await native.push({ ...native.current, active: { tenant: meridian.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, inbox: { ...native.current.inbox, items: [held], lastSnooze: { id: 'gateway-ws-9', title: 'B already snoozed', returnsAt: 1_761_576_400, sequence: 3 } } })
        await screen.findByText(/B already snoozed/)
        // A reread repeats the same receipt: still nothing new.
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: { ...native.current.inbox.read, generation: 9 } } })
        await waitFor(() => expect(toasts().join(' ')).toMatch(/Read finished/))
        expect(toasts().join(' ')).not.toContain('snoozed')
        // A new snooze in the active tenant is announced exactly once, with the core's time.
        await native.push({ ...native.current, inbox: { ...native.current.inbox, lastSnooze: { id: 'gateway-ws-9', title: 'B already snoozed', returnsAt: 1_761_580_000, sequence: 4 } } })
        await waitFor(() => expect(toasts().join(' ')).toContain('B already snoozed snoozed · returns'))
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: { ...native.current.inbox.read, generation: 10 } } })
        await waitFor(() => expect(toasts().join(' ')).toMatch(/Read finished/))
        expect(toasts().join(' ')).not.toContain('snoozed')
    })

    it('reports each action outcome for the tenant or item it names: refusal and failure as warn toasts, an unconfirmed delivery with Retry, an admitted recheck as info (DESIGN-V2 §7)', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxInbox(), activeFeature: 'home' })
        native.respond = () => ({ outcome: 'refusal', refusal: 'capabilityMissing' })
        await user.click(screen.getByRole('button', { name: 'Read now' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'readTenant', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' })
        expect(await screen.findByText('The read of Northwind Health was refused (capabilityMissing). That is not available for the current tenant.')).toBeInTheDocument()
        expect(document.querySelector('[data-slot="toast"]')?.getAttribute('data-kind')).toBe('warn')
        // A hold whose delivery was not confirmed is kept inline with Retry; it is never replayed on its own.
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(screen.getByRole('button', { name: 'Snooze 24 hours: Runtime expires in 5 days' }))
        const unconfirmed = await screen.findByText('The snooze of “Runtime expires in 5 days” was sent but the desktop did not confirm it. The current state is shown.')
        const sent = native.dispatched.length
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(unconfirmed.closest('li') as HTMLElement).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.length).toBe(sent + 1)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'snoozeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'runtime-key:k1' })
        // An admitted recheck announces the product it rereads.
        await user.click(screen.getByRole('button', { name: 'Recheck' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'recheckProduct', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', product: 'gateway' })
        await waitFor(() => expect(toasts().join(' ')).toContain('Rechecking AI Gateway…'))
        // The Inbox's Read now and hold actions report the same way.
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        native.respond = () => ({ outcome: 'refusal', refusal: 'staleContext' })
        await user.click(screen.getByRole('button', { name: /Runtime expires in 5 days/ }))
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Acknowledge' }))
        expect(await screen.findByText(/The acknowledgment of “Runtime expires in 5 days” was refused \(staleContext\)/)).toBeInTheDocument()
    })

    it('a failed first read is failed, never pending or an all-clear; a failed refresh labels the kept read as older (I4)', async () => {
        const native = await launch({ ...fxFresh() })
        // Admitted, then failed at the token: one completion, nothing read.
        const failed = { state: 'failed' as const, lastReadAt: null, failedAt: 1_761_490_300, settledAt: 1_761_490_300, products: [{ product: 'runtimeSecurity' as const, state: 'failed' as const, sourcesRead: 0, sourcesTotal: 2, detail: 'Unauthorized' }], sourcesMissing: ['sessions chart', 'API keys'], generation: 1, rechecking: null }
        await native.push({ ...native.current, inbox: { items: [], read: failed, lastSnooze: null }, overview: { ...native.current.overview, failure: 'unauthorized', failedAt: 1_761_490_300 } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read failed at \d\d:\d\d · nothing read yet/))
        await waitFor(() => expect(toasts().join(' ')).toMatch(/Read failed at \d\d:\d\d · nothing read yet/))
        expect(document.querySelector('[data-slot="toast"]')?.getAttribute('data-kind')).toBe('warn')
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).not.toContain('pending')
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toMatch(/The read at \d\d:\d\d failed\. Nothing has been read/)
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).not.toContain('Nothing needs action')
        // A later failed refresh keeps the earlier successful read, labelled older.
        const kept = fxInbox()
        await native.push({ ...kept, activeFeature: 'inbox', inbox: { ...kept.inbox, read: { ...kept.inbox.read, state: 'failed', failedAt: 1_761_493_600, settledAt: 1_761_493_600, generation: 2 } } })
        await waitFor(() => expect(screen.getByText(/The read at \d\d:\d\d failed; the read from \d\d:\d\d is shown\./)).toBeInTheDocument())
        expect(screen.getByRole('list', { name: 'Inbox items' })).toBeInTheDocument()
        await native.push({ ...native.current, activeFeature: 'home' })
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read failed at \d\d:\d\d · showing the read from \d\d:\d\d/)
    })

    it('an arrival and an unconfirmed item action belong to their tenant: the next tenant inherits neither, and a retired context cannot be revived (I2)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        await user.click(screen.getByRole('button', { name: /Runtime expires in 5 days/ }))
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Open API keys' }))
        // An unconfirmed snooze in Northwind, kept inline with Retry.
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Runtime expires in 5 days/ }))
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Snooze 24 hours: Runtime expires in 5 days' }))
        await screen.findByText(/The snooze of “Runtime expires in 5 days” was sent but the desktop did not confirm it/)
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        // Meridian, with a ready management client, opens Administration: no inherited filter, no inherited action strip.
        const meridian = native.current.tenants[1]!
        const ready = { ...meridian, management: { state: 'ready' as const, mode: 'keychain' as const, failure: null, clientId: 'client@1001464285.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } }
        await native.push({ ...native.current, tenants: [native.current.tenants[0]!, ready], active: { tenant: meridian.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, activeFeature: 'administration', inbox: { items: [], read: { state: 'pending', lastReadAt: null, failedAt: null, settledAt: null, products: [], sourcesMissing: [], generation: 0, rechecking: null }, lastSnooze: null } })
        await screen.findByRole('heading', { level: 1, name: 'Administration' })
        expect(screen.getByRole('tab', { name: 'Custom topics' })).toHaveAttribute('aria-selected', 'true')
        expect(screen.getByRole('tab', { name: 'API keys' })).toHaveAttribute('aria-selected', 'false')
        expect(document.querySelector('[data-slot="administration-filter"]')).toHaveValue('')
        expect(document.querySelector('[data-slot="unconfirmed"]')).toBeNull()
        // Back in Northwind (a new context): the retired delivery is gone for good, never re-sent.
        const sent = native.dispatched.length
        await native.push({ ...fxInbox(), active: { tenant: 'tn-0123456789abcdef', context: 'cx-cccccccccccccccccccccccc' } })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="unconfirmed"]')).toBeNull()
        expect(native.dispatched.length).toBe(sent)
    })

    it('Home never claims an all-clear from unread data: in-progress, failed and partial reads are named (I4)', async () => {
        const native = await launch(fxFresh())
        const base = native.current.inbox.read
        await native.push({ ...native.current, inbox: { items: [], read: { ...base, state: 'reading' }, lastSnooze: null }, overview: { ...native.current.overview, refreshing: true } })
        const attentionText = () => document.querySelector('[data-slot="home"]')?.textContent ?? ''
        await waitFor(() => expect(attentionText()).toContain('Reading…'))
        expect(attentionText()).not.toContain('Nothing needs action')
        // Partial: complete only in what was read.
        const partial = { ...base, state: 'done' as const, lastReadAt: 1_761_490_000, settledAt: 1_761_490_000, products: [{ product: 'runtimeSecurity' as const, state: 'incomplete' as const, sourcesRead: 1, sourcesTotal: 2, detail: 'unreported' }], sourcesMissing: ['API keys'], generation: 1 }
        const partialRead = { readAt: 1_761_490_000, chart: { bucketSeconds: 3600, buckets: [] }, chartFailure: null, summary: { sessions: 0, violating: 0, benign: 0, detectionEvents: 0 }, keysComplete: true, keysFailure: null, items: [], gaps: ['Runtime Security API keys: 1 key expiration unreported'], gateway: null }
        await native.push({ ...native.current, inbox: { items: [], read: partial, lastSnooze: null }, overview: { ...native.current.overview, refreshing: false, fresh: true, face: 'gap', read: partialRead } })
        await waitFor(() => expect(attentionText()).toContain('Nothing needs action in what was read. 1 source was not read.'))
        // Complete: the all-clear.
        await native.push({ ...native.current, inbox: { items: [], read: { ...partial, products: [{ ...partial.products[0]!, state: 'ok', sourcesRead: 2 }], sourcesMissing: [], generation: 2 }, lastSnooze: null } })
        await waitFor(() => expect(attentionText()).toMatch(/Nothing needs action\.(?! in)/))
    })

    it('toasts that speak for a tenant are retired on leaving it, even under Reduce Motion (I2)', async () => {
        const native = await launch(fxInbox())
        const matchMedia = window.matchMedia
        window.matchMedia = ((query: string) => ({ matches: query.includes('reduce'), media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false })) as typeof window.matchMedia
        try {
            await native.push({ ...native.current, inbox: { ...native.current.inbox, lastSnooze: { id: 'runtime-key:k1', title: 'Runtime expires in 5 days', returnsAt: 1_761_576_400, sequence: 1 } } })
            await waitFor(() => expect(toasts().join(' ')).toContain('Runtime expires in 5 days snoozed'))
            const meridian = native.current.tenants[1]!
            await native.push({ ...native.current, active: { tenant: meridian.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, inbox: { items: [], read: { state: 'pending', lastReadAt: null, failedAt: null, settledAt: null, products: [], sourcesMissing: [], generation: 0, rechecking: null }, lastSnooze: null } })
            await waitFor(() => expect(toasts()).toEqual([]))
            // Back in Northwind: the old receipt is a baseline, not news.
            await native.push({ ...fxInbox(), inbox: { ...fxInbox().inbox, lastSnooze: { id: 'runtime-key:k1', title: 'Runtime expires in 5 days', returnsAt: 1_761_576_400, sequence: 1 } } })
            await screen.findByRole('heading', { level: 1, name: 'Inbox' })
            expect(toasts()).toEqual([])
        } finally {
            window.matchMedia = matchMedia
        }
    })

    it('a notification for a scope that retired before it arrived is never surfaced: late refusal, lost delivery, late recheck (I2)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        await user.click(screen.getByRole('button', { name: /Runtime expires in 5 days/ }))
        // Northwind's snooze is answered only after the shell has moved to Meridian.
        let answer: (value: unknown) => void = () => undefined
        native.respond = (intent) => (intent.kind === 'snoozeItem' ? new Promise((resolve) => (answer = resolve)) : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Snooze 24 hours: Runtime expires in 5 days' }))
        const meridian = native.current.tenants[1]!
        const inMeridian = { ...native.current, active: { tenant: meridian.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, activeFeature: 'home' as const, inbox: { items: [], read: { state: 'pending' as const, lastReadAt: null, failedAt: null, settledAt: null, products: [], sourcesMissing: [], generation: 0, rechecking: null }, lastSnooze: null } }
        await native.push(inMeridian)
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        answer({ outcome: 'refusal', refusal: 'staleContext' })
        await new Promise((r) => setTimeout(r, 50))
        expect(toasts()).toEqual([])
        expect(document.body.textContent).not.toContain('Runtime expires in 5 days')
        // The same for a lost delivery (the client gives up after its resend): no Retry strip in Meridian.
        await native.push({ ...fxInbox() })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Runtime expires in 5 days/ }))
        let fail: (reason: unknown) => void = () => undefined
        native.respond = (intent) => (intent.kind === 'snoozeItem' ? new Promise((_, reject) => (fail = reject)) : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Snooze 24 hours: Runtime expires in 5 days' }))
        await native.push(inMeridian)
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        fail(new DeliveryOutcomeUnknown('lost'))
        await new Promise((r) => setTimeout(r, 50))
        fail(new DeliveryOutcomeUnknown('lost'))
        await new Promise((r) => setTimeout(r, 50))
        expect(document.querySelector('[data-slot="unconfirmed"]')).toBeNull()
        expect(toasts()).toEqual([])
        // Back in Northwind (a new context): still nothing of the retired action.
        await native.push({ ...fxInbox(), active: { tenant: 'tn-0123456789abcdef', context: 'cx-cccccccccccccccccccccccc' } })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="unconfirmed"]')).toBeNull()
        expect(toasts()).toEqual([])
        // A refusal answered only after A → B → A (a new context of A) speaks for the retired context: never surfaced.
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Runtime expires in 5 days/ }))
        let late: (value: unknown) => void = () => undefined
        native.respond = (intent) => (intent.kind === 'acknowledgeItem' ? new Promise((resolve) => (late = resolve)) : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Acknowledge' }))
        await native.push(inMeridian)
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        await native.push({ ...fxInbox(), active: { tenant: 'tn-0123456789abcdef', context: 'cx-dddddddddddddddddddddddd' } })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        late({ outcome: 'refusal', refusal: 'staleContext' })
        await new Promise((r) => setTimeout(r, 50))
        expect(toasts()).toEqual([])
    })

    it('a first product-only recheck refused for a role is a settled attempt, announced for when it settled, never first read pending (I4)', async () => {
        const native = await launch(fxFresh())
        const settled = { state: 'done' as const, lastReadAt: null, failedAt: null, settledAt: 1_761_490_000, products: [{ product: 'runtimeSecurity' as const, state: 'pending' as const, sourcesRead: 0, sourcesTotal: 2, detail: null }, { product: 'redTeam' as const, state: 'needsRole' as const, sourcesRead: 0, sourcesTotal: 2, detail: 'Denied' }], sourcesMissing: ['Red Team statistics', 'Red Team scan list', 'sessions chart', 'API keys'], generation: 1, rechecking: null }
        await native.push({ ...native.current, inbox: { items: [], read: settled, lastSnooze: null } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read finished at \d\d:\d\d · 1 product still needs a role/))
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).not.toContain('pending')
        await waitFor(() => expect(toasts().join(' ')).toMatch(/1 product still needs a role/))
        expect(document.querySelector('[data-slot="toast"]')?.getAttribute('data-kind')).toBe('warn')
        expect(document.querySelector('[data-slot="home"]')?.textContent).toContain('Nothing needs action in what was read. 4 sources were not read.')
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toMatch(/established nothing: 4 sources still unread/)
    })

    it('lists the entries the core supplies before any read (a setup warning) with every action, while a truly empty fresh tenant shows pending (I12)', async () => {
        const user = userEvent.setup()
        const fresh = fxFresh()
        const warning = { id: 'vw-1', kind: 'setupWarning' as const, needsAction: true, severity: 'warn' as const, title: 'AI Gateway: setup warning', detail: 'Needs the management client; not checked.', source: 'Setup · verify', product: 'gateway' as const, target: { feature: 'gateway' as const, product: 'gateway' as const, resource: null, item: null }, actionLabel: 'Open AI Gateway', facts: [{ label: 'Product', value: 'AI Gateway' }], when: 1_761_480_000, hold: null }
        const native = await launch({ ...fresh, activeFeature: 'inbox', inbox: { ...fresh.inbox, items: [warning] } })
        expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual(['All 1', 'Needs action 1', 'Updates 0', 'Snoozed 0'])
        expect(document.querySelector('[data-slot="inbox-empty"]')).toBeNull()
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /AI Gateway: setup warning/ }))
        const detail = screen.getByRole('complementary', { name: 'Details' })
        await user.click(within(detail).getByRole('button', { name: 'Open AI Gateway' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'gateway' })
        await user.click(within(detail).getByRole('button', { name: 'Acknowledge' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'acknowledgeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'vw-1' })
        await user.click(within(detail).getByRole('button', { name: 'Snooze 24 hours: AI Gateway: setup warning' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'snoozeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'vw-1' })
        await native.push({ ...native.current, inbox: { ...native.current.inbox, items: [{ ...warning, hold: { kind: 'snooze', returnsAt: 1_761_576_400 } }] } })
        await user.click(screen.getByRole('tab', { name: /Snoozed/ }))
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /AI Gateway: setup warning/ }))
        await user.click(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('button', { name: 'Unsnooze' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'unsnoozeItem', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa', id: 'vw-1' })
        // The read is still pending: Home says so, the entry stays on Home's attention list too.
        expect(native.current.inbox.read.state).toBe('pending')
        await native.push({ ...native.current, activeFeature: 'home', inbox: { ...native.current.inbox, items: [warning] } })
        await screen.findByRole('heading', { level: 1, name: 'Home' })
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toContain('First read pending')
        expect(document.querySelector('[data-slot="attention"]')?.textContent).toContain('AI Gateway: setup warning')
        // Without any entry, a fresh tenant's Inbox is the pending state.
        await native.push({ ...fresh, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toContain('First read pending')
    })

    it('a healthy empty read older than ten minutes is a gap, not an all-clear, on Home and in the Inbox (C12, I4)', async () => {
        const native = await launch({ ...fxInbox(), activeFeature: 'home' })
        const healthy = { ...native.current.inbox.read, products: [{ product: 'runtimeSecurity' as const, state: 'ok' as const, sourcesRead: 2, sourcesTotal: 2, detail: null }], sourcesMissing: [] }
        const read = { ...native.current.overview, read: { readAt: 1_761_490_000, chart: { bucketSeconds: 3600, buckets: [] }, chartFailure: null, summary: { sessions: 0, violating: 0, benign: 0, detectionEvents: 0 }, keysComplete: true, keysFailure: null, items: [], gaps: [], gateway: null }, attention: [], held: [] }
        // Fresh: no activity yet, an allowed line.
        await native.push({ ...native.current, inbox: { items: [], read: healthy, lastSnooze: null }, overview: { ...read, fresh: true, face: 'allClear' } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/no activity yet in Northwind Health/))
        expect(document.querySelector('[data-slot="home"]')?.textContent).toMatch(/Nothing needs action\.(?! in)/)
        // Expired: the cached read is kept, its age named; never an all-clear.
        await native.push({ ...native.current, overview: { ...read, fresh: false, face: 'gap' } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read at \d\d:\d\d · more than 10 minutes old; read again to confirm/))
        expect(document.querySelector('[data-slot="home"]')?.textContent).toMatch(/Nothing needs action in what was read\. The read from \d\d:\d\d is more than 10 minutes old\./)
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toMatch(/Nothing needed action at \d\d:\d\d; that read is more than 10 minutes old\./)
        expect(screen.getByText(/The read from \d\d:\d\d is more than 10 minutes old\./)).toBeInTheDocument()
        // Nothing was re-read on its own.
        expect(native.dispatched.filter((i) => i.kind === 'readTenant' || i.kind === 'management')).toHaveLength(0)
        // Read now: admission alone restores nothing; the expired read stays named until the new one lands.
        await native.push({ ...native.current, activeFeature: 'home', inbox: { ...native.current.inbox, read: { ...healthy, state: 'reading' } }, overview: { ...read, fresh: false, face: 'gap', refreshing: true } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Reading… · the read from \d\d:\d\d shown is more than 10 minutes old/))
        expect(document.querySelector('[data-slot="home"]')?.textContent).toMatch(/Nothing needs action in what was read\. A read is in progress; the read from \d\d:\d\d shown is more than 10 minutes old\./)
        expect(document.querySelector('[data-slot="home"]')?.textContent).not.toContain('no activity yet')
        await native.push({ ...native.current, activeFeature: 'inbox' })
        await screen.findByRole('heading', { level: 1, name: 'Inbox' })
        expect(screen.getByText(/A read is in progress; the read from \d\d:\d\d shown is more than 10 minutes old\./)).toBeInTheDocument()
        // A successful settlement is fresh again; a failed one keeps the older read, named by the failure.
        await native.push({ ...native.current, activeFeature: 'home', inbox: { ...native.current.inbox, read: { ...healthy, state: 'done', lastReadAt: 1_761_493_600, settledAt: 1_761_493_600, generation: 2 } }, overview: { ...read, read: { ...read.read, readAt: 1_761_493_600 }, fresh: true, face: 'allClear', refreshing: false } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read finished at \d\d:\d\d · no activity yet in Northwind Health/))
        await native.push({ ...native.current, inbox: { ...native.current.inbox, read: { ...healthy, state: 'failed', lastReadAt: 1_761_493_600, failedAt: 1_761_497_200, settledAt: 1_761_497_200, generation: 3 } }, overview: { ...read, read: { ...read.read, readAt: 1_761_493_600 }, fresh: false, face: 'gap', failure: 'unavailable', failedAt: 1_761_497_200 } })
        await waitFor(() => expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read failed at \d\d:\d\d · showing the read from \d\d:\d\d/))
        expect(document.querySelector('[data-slot="home"]')?.textContent).not.toMatch(/Nothing needs action\.(?! in)/)
    })

    it('keeps the Inbox tab and item per tenant across a switch round trip (I2)', async () => {
        const user = userEvent.setup()
        const native = await launch(fxInbox())
        await user.click(screen.getByRole('tab', { name: /Snoozed/ }))
        await user.click(screen.getByRole('button', { name: /Gateway workspace Team A/ }))
        // Meridian: its own (empty) Inbox and its own selection.
        const meridian = native.current.tenants[1]!
        await native.push({ ...native.current, active: { tenant: meridian.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, inbox: { items: [], read: { state: 'pending', lastReadAt: null, failedAt: null, settledAt: null, products: [], sourcesMissing: [], generation: 0, rechecking: null }, lastSnooze: null } })
        await waitFor(() => expect(screen.getByRole('tab', { name: /^All/ })).toHaveAttribute('aria-selected', 'true'))
        expect(document.querySelector('[data-slot="inbox-empty"]')?.textContent).toContain('First read pending')
        // Back to Northwind: the Snoozed tab and the selected item are as they were.
        await native.push(fxInbox())
        await waitFor(() => expect(screen.getByRole('tab', { name: /Snoozed/ })).toHaveAttribute('aria-selected', 'true'))
        expect(within(screen.getByRole('complementary', { name: 'Details' })).getByRole('heading', { level: 2, name: 'Gateway workspace Team A over quota' })).toBeInTheDocument()
    })
})
