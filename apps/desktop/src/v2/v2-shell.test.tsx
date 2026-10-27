/**
 * The production App over a scripted native transport, rendering the v2
 * shell (C28): the Rust catalogue drives the sidebar, every navigation is a
 * `navigate` intent, the palette and tenant switcher dispatch, preferences
 * dispatch, refusals reach the status surface as warn toasts, and the
 * accessibility invariants I8–I10 hold.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient, type RuntimeOverview } from '@prisma-airs/desktop-contracts'
import { App } from '../app'
import { scriptedNative, tenant, type Native } from '../test/native'
import { fxHome, fxHomeDark, fxSwitcher } from './fixtures'
import { DeliveryOutcomeUnknown } from '@prisma-airs/desktop-contracts'
import { resetShellStore, useShellStore } from './store'

async function launch(snapshot = fxHome()): Promise<Native> {
    const native = scriptedNative(snapshot)
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('navigation', { name: 'Destinations' })
    return native
}

const rail = () => within(screen.getByRole('navigation', { name: 'Destinations' }))
const intents = (native: Native) => native.dispatched.map((i) => i.kind)

beforeEach(() => {
    resetShellStore()
    document.documentElement.removeAttribute('data-theme')
})

describe('v2 shell', () => {
    it('renders the sidebar from the Rust catalogue with the menu shortcuts and marks the unavailable Inbox', async () => {
        await launch()
        const items = rail().getAllByRole('button').filter((b) => b.hasAttribute('data-inset-focus'))
        expect(items.map((b) => b.textContent)).toEqual([
            'Home⌘1',
            'Inbox⌘2',
            'Harness⌘3',
            'Academy⌘4',
            'Test Cases⌘5',
            'API Client⌘6',
            'Administration⌘7',
        ])
        expect(items[0]).toHaveAttribute('aria-current', 'page')
        expect(items[1]).toHaveAttribute('aria-disabled', 'true')
        expect(items[1]).toHaveAccessibleDescription('Not available: The Inbox arrives in C30')
        expect(screen.getByRole('heading', { level: 1, name: 'Home' })).toBeInTheDocument()
        expect(document.querySelector('[data-v2-shell]')).not.toBeNull()
    })

    it('dispatches navigate for the sidebar, the native menu and Home cards, never local state', async () => {
        const user = userEvent.setup()
        const native = await launch()
        await user.click(rail().getByRole('button', { name: /Harness/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'harness' })
        // The page changes only when the core reports it.
        expect(screen.getByRole('heading', { level: 1, name: 'Home' })).toBeInTheDocument()
        await native.push({ ...native.current, activeFeature: 'harness' })
        expect(rail().getByRole('button', { name: /Harness/ })).toHaveAttribute('aria-current', 'page')
        // An unavailable destination dispatches nothing.
        await user.click(rail().getByRole('button', { name: /Inbox/ }))
        expect(intents(native).filter((k) => k === 'navigate')).toHaveLength(1)
        // A Home card is the same intent.
        await native.push({ ...native.current, activeFeature: 'home' })
        await user.click(within(screen.getByRole('main')).getByRole('button', { name: /Test Cases/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'library' })
    })

    it('reports a refused navigation on the status surface as a warn toast', async () => {
        const user = userEvent.setup()
        const native = await launch()
        native.respond = () => ({ outcome: 'refusal', refusal: 'invalidInput' })
        await user.click(rail().getByRole('button', { name: /Academy/ }))
        const toast = await screen.findByText('Navigation was refused (invalidInput).')
        const surface = toast.closest('[data-slot="toast"]')
        expect(surface).toHaveAttribute('role', 'status')
        expect(surface).toHaveAttribute('data-kind', 'warn')
        expect(document.querySelectorAll('[data-slot="toast"]')).toHaveLength(1)
    })

    it('opens the palette from the sidebar, lists destinations, tenants and actions, and dispatches on Enter', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        const dialog = screen.getByRole('dialog', { name: 'Command palette' })
        expect(dialog).toHaveAttribute('aria-modal', 'true')
        const options = within(dialog).getAllByRole('option').map((o) => o.textContent)
        expect(options.slice(0, 2)).toEqual(['Home⌘1', 'Inbox⌘2C30'])
        expect(options).toContain('Switch to Meridian Retail · unitedStates · TSG 1001464285')
        expect(options).toContain('Read now')
        // End and Home jump to the last and first entries.
        await user.keyboard('{End}')
        expect(within(dialog).getByRole('combobox')).toHaveAttribute('aria-activedescendant', expect.stringContaining('action:read'))
        await user.keyboard('{Home}')
        expect(within(dialog).getByRole('combobox')).toHaveAttribute('aria-activedescendant', expect.stringContaining('go:home'))
        await user.type(within(dialog).getByRole('combobox'), 'meridian{Enter}')
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: 'tn-fedcba9876543210' })
        expect(screen.queryByRole('dialog')).toBeNull()
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        await user.type(screen.getByRole('combobox'), 'read{Enter}')
        expect(native.dispatched.at(-1)).toMatchObject({ kind: 'management', request: { kind: 'overview' } })
    })

    it('switches tenants from the rail switcher with one selectTenant intent and lists Add a tenant', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        const switcher = screen.getByRole('button', { name: /^Switch tenant/ })
        expect(switcher).toHaveAccessibleName('Switch tenant: Northwind Health · europe · TSG 1762204418')
        expect(switcher).toHaveTextContent('TSG 1762204418')
        await user.click(switcher)
        const list = screen.getByRole('group', { name: 'Switch tenant' })
        expect(within(list).getAllByRole('button').map((o) => o.textContent)).toEqual([
            'Northwind Healtheurope · TSG 1762204418',
            'Meridian RetailunitedStates · TSG 1001464285',
            'Add a tenant…',
        ])
        await user.click(within(list).getByRole('button', { name: /Meridian Retail/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: 'tn-fedcba9876543210' })
        expect(screen.queryByRole('group', { name: 'Switch tenant' })).toBeNull()
    })

    it('dispatches the appearance, show-the-calls and return-to-classic preferences', async () => {
        const user = userEvent.setup()
        const native = await launch()
        await user.click(screen.getByRole('button', { name: 'Dark' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setAppearance', theme: 'dark' })
        await user.click(screen.getByRole('checkbox', { name: 'Show the calls' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setShowCalls', on: false })
        await user.click(screen.getByRole('button', { name: 'Return to the classic interface' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setUiVariant', variant: 'legacy' })
        // The legacy shell renders only when the core says so.
        await native.push({ ...native.current, uiVariant: 'legacy', features: (await import('../test/native')).baseSnapshot.features, activeFeature: 'runtime' })
        expect(screen.getByRole('navigation', { name: 'Features' })).toBeInTheDocument()
    })

    it('applies the dark theme attribute the tokens key on', async () => {
        await launch(fxHomeDark())
        expect(document.documentElement.dataset.theme).toBe('gruvbox')
    })

    it('shows the read status from the observed overview, never a timer of its own', async () => {
        const native = await launch(fxSwitcher())
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toContain('First read pending')
        const read: RuntimeOverview = {
            readAt: 1_761_490_000,
            chart: { bucketSeconds: 3600, buckets: [{ time: '2026-10-26T07:00:00Z', total: 1, violating: 0, detections: [] }] },
            chartFailure: null,
            summary: { sessions: 0, violating: 0, benign: 0, detectionEvents: 0 },
            keysComplete: true,
            keysFailure: null,
            items: [],
            gaps: [],
            gateway: null,
        }
        await native.push({ ...native.current, overview: { ...native.current.overview, face: 'allClear', read, fresh: true } })
        expect(document.querySelector('[data-slot="read-status"]')?.textContent).toMatch(/Read finished at \d\d:\d\d/)
    })

    it('keeps one topmost modal: the palette over the switcher, Escape closes only the top, focus returns', async () => {
        const user = userEvent.setup()
        await launch(fxSwitcher())
        const trigger = rail().getByRole('button', { name: /Go to…/ })
        trigger.focus()
        await user.keyboard('{Enter}')
        const palette = screen.getByRole('dialog', { name: 'Command palette' })
        expect(palette).toHaveAttribute('data-topmost', 'true')
        expect(within(palette).getByRole('combobox')).toHaveFocus()
        expect(useShellStore.getState().modals).toHaveLength(1)
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog')).toBeNull()
        expect(trigger).toHaveFocus()
    })

    it('expires toasts after their lifetime, or keeps them under Reduce Motion', async () => {
        await launch()
        act(() => useShellStore.setState({ toastMs: 120 }))
        const toast = () => document.querySelector('[data-slot="toast"]')
        act(() => {
            useShellStore.getState().toast('Read finished at 07:45 · all sources complete.', 'ok')
        })
        await waitFor(() => expect(toast()).toHaveAttribute('data-kind', 'ok'))
        await waitFor(() => expect(toast()).toBeNull(), { timeout: 2000 })
        // Reduce Motion: persists until dismissed.
        const matchMedia = window.matchMedia
        window.matchMedia = ((q: string) => ({ matches: q.includes('reduced-motion'), media: q, addEventListener: () => undefined, removeEventListener: () => undefined })) as unknown as typeof window.matchMedia
        try {
            act(() => {
                useShellStore.getState().toast('Snoozed until tomorrow 07:42 · support-copilot-key', 'info')
            })
            await waitFor(() => expect(toast()).toHaveAttribute('data-kind', 'info'))
            await new Promise((resolve) => setTimeout(resolve, 400))
            expect(toast()).toHaveAttribute('data-kind', 'info')
            expect(toast()).toHaveTextContent('Warning:'.replace('Warning:', 'Note:'))
        } finally {
            window.matchMedia = matchMedia
        }
    })

    it('keeps an unconfirmed delivery inline with Retry, never auto-resending, and retries only on the click', async () => {
        const user = userEvent.setup()
        const native = await launch()
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(rail().getByRole('button', { name: /Harness/ }))
        const strip = await screen.findByRole('list', { name: 'Not confirmed' })
        expect(strip).toHaveTextContent('Navigation was sent but the desktop did not confirm it.')
        expect(document.querySelector('[data-slot="toast"]')).toBeNull()
        const sent = native.dispatched.length
        // Snapshot updates never redispatch.
        await native.push({ ...native.current })
        expect(native.dispatched).toHaveLength(sent)
        expect(screen.getByRole('list', { name: 'Not confirmed' })).toBeInTheDocument()
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-2', operation: null } })
        await user.click(within(strip).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'harness' })
        expect(screen.queryByRole('list', { name: 'Not confirmed' })).toBeNull()
    })

    it('opens the tenant switcher from the native ⌘T command without switching, and returns focus to the trigger on Escape', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await native.pushMenu({ kind: 'tenantSwitcher' })
        const list = await screen.findByRole('group', { name: 'Switch tenant' })
        await waitFor(() => expect(within(list).getAllByRole('button')[0]).toHaveFocus())
        expect(native.dispatched.filter((i) => i.kind === 'selectTenant')).toHaveLength(0)
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('group', { name: 'Switch tenant' })).toBeNull()
        expect(screen.getByRole('button', { name: /^Switch tenant/ })).toHaveFocus()
        // ⌘1 from the native Go menu dispatches navigate like the sidebar.
        await native.pushMenu({ kind: 'navigate', feature: 'home' })
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'home' })
    })

    it('keeps destination names when the rail is collapsed and does not restart a toast deadline on re-render', async () => {
        const native = await launch()
        // Apply the collapsed rule's visual hiding to every label, as the <1280px media query does.
        document.querySelectorAll<HTMLElement>('[data-v2-label]').forEach((el) => {
            el.style.position = 'absolute'
            el.style.width = '1px'
            el.style.height = '1px'
            el.style.overflow = 'hidden'
            el.style.clip = 'rect(0 0 0 0)'
        })
        expect(rail().getByRole('button', { name: /^Harness/ })).toHaveAccessibleName('Harness Command 3')
        expect(rail().getByRole('button', { name: 'Return to the classic interface' })).toBeInTheDocument()
        expect(rail().getByRole('button', { name: /^Harness/ })).toHaveAttribute('title', 'Harness (⌘3)')
        // A snapshot update re-renders the shell mid-life; the deadline is the toast's.
        act(() => useShellStore.setState({ toastMs: 300 }))
        const started = Date.now()
        act(() => {
            useShellStore.getState().toast('Read finished at 07:45 · all sources complete.', 'ok')
        })
        await new Promise((resolve) => setTimeout(resolve, 150))
        await native.push({ ...native.current })
        expect(document.querySelector('[data-slot="toast"]')).not.toBeNull()
        await waitFor(() => expect(document.querySelector('[data-slot="toast"]')).toBeNull(), { timeout: 2000 })
        expect(Date.now() - started).toBeLessThan(600)
    })

    it('hands focus to the tenant list when ⌘T replaces an open palette, and Escape returns it to the trigger', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        expect(screen.getByRole('dialog', { name: 'Command palette' })).toBeInTheDocument()
        await native.pushMenu({ kind: 'tenantSwitcher' })
        expect(screen.queryByRole('dialog')).toBeNull()
        const list = await screen.findByRole('group', { name: 'Switch tenant' })
        await waitFor(() => expect(within(list).getAllByRole('button')[0]).toHaveFocus())
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('group', { name: 'Switch tenant' })).toBeNull()
        expect(screen.getByRole('button', { name: /^Switch tenant/ })).toHaveFocus()
        expect(native.dispatched.filter((i) => i.kind === 'selectTenant')).toHaveLength(0)
    })

    it('routes appearance through the same recovery path from the rail and the native menu', async () => {
        const user = userEvent.setup()
        const native = await launch()
        native.respond = () => ({ outcome: 'refusal', refusal: 'deniedStore' })
        await user.click(screen.getByRole('button', { name: 'Dark' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setAppearance', theme: 'dark' })
        const toast = await screen.findByText('The appearance change was refused (deniedStore).')
        expect(toast.closest('[data-slot="toast"]')).toHaveAttribute('data-kind', 'warn')
        // The observed selection stays authoritative: still 'system'.
        expect(screen.getByRole('button', { name: 'Match system' })).toHaveAttribute('aria-pressed', 'true')
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await native.pushMenu({ kind: 'setAppearance', theme: 'light' })
        const strip = await screen.findByRole('list', { name: 'Not confirmed' })
        expect(strip).toHaveTextContent('The appearance change was sent but the desktop did not confirm it.')
        const sent = native.dispatched.length
        await native.push({ ...native.current })
        expect(native.dispatched).toHaveLength(sent)
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-3', operation: null } })
        await user.click(within(strip).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setAppearance', theme: 'light' })
        expect(screen.queryByRole('list', { name: 'Not confirmed' })).toBeNull()
    })

    it('focuses the tenant list after ⌘T even when focus inside an inert rail is refused, as WebKit does', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        // WebKit refuses focus inside an inert subtree; jsdom does not, so refuse it here.
        const descriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'focus') as PropertyDescriptor
        const realFocus = HTMLElement.prototype.focus
        Object.defineProperty(HTMLElement.prototype, 'focus', {
            configurable: true,
            writable: true,
            value: function (this: HTMLElement, ...args: Parameters<typeof realFocus>) {
                if (this.closest('[inert]')) return
                return realFocus.apply(this, args)
            },
        })
        try {
            await native.pushMenu({ kind: 'tenantSwitcher' })
            const list = await screen.findByRole('group', { name: 'Switch tenant' })
            await waitFor(() => expect(within(list).getAllByRole('button')[0]).toHaveFocus())
            expect(document.querySelector('[data-v2-rail]')).not.toHaveAttribute('inert')
            await user.keyboard('{Escape}')
            expect(screen.getByRole('button', { name: /^Switch tenant/ })).toHaveFocus()
            expect(native.dispatched.filter((i) => i.kind === 'selectTenant')).toHaveLength(0)
        } finally {
            Object.defineProperty(HTMLElement.prototype, 'focus', descriptor)
        }
    })

    it('renders the tenant popup outside the rail with a usable width and reports Settings interface-toggle outcomes', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        const list = screen.getByRole('group', { name: 'Switch tenant' })
        expect(list.closest('[data-v2-rail]')).toBeNull()
        expect(list.style.minWidth).toBe('280px')
        await user.keyboard('{Escape}')
        // Settings (inside the v2 shell) reports a refused interface change and keeps an unknown one retryable.
        await native.push({ ...native.current, activeFeature: 'settings' })
        const toggle = await screen.findByRole('checkbox', { name: 'Use the new interface' })
        native.respond = () => ({ outcome: 'refusal', refusal: 'deniedStore' })
        await user.click(toggle)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setUiVariant', variant: 'legacy' })
        const status = document.querySelector('[data-slot="interface-status"]') as HTMLElement
        await waitFor(() => expect(status.textContent).toMatch(/refused|Keychain|store|denied/i))
        expect(toggle).toBeChecked()
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(toggle)
        await waitFor(() => expect(status).toHaveTextContent('did not confirm it'))
        const sent = native.dispatched.length
        await native.push({ ...native.current })
        expect(native.dispatched).toHaveLength(sent)
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-4', operation: null } })
        await user.click(within(status).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setUiVariant', variant: 'legacy' })
        await waitFor(() => expect(status.textContent).toBe(''))
    })

    it('makes toast controls inert and hidden while a modal is topmost, usable again after', async () => {
        const user = userEvent.setup()
        await launch(fxSwitcher())
        act(() => useShellStore.setState({ toastMs: 60000 }))
        act(() => {
            useShellStore.getState().toast('Navigation was refused (invalidInput).', 'warn')
        })
        const dismiss = () => screen.queryByRole('button', { name: 'Dismiss', hidden: true }) as HTMLElement
        expect(dismiss().closest('[inert]')).toBeNull()
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        expect(dismiss().closest('[inert]')).not.toBeNull()
        expect(dismiss().closest('[aria-hidden="true"]')).not.toBeNull()
        await user.keyboard('{Escape}')
        expect(dismiss().closest('[inert]')).toBeNull()
        await user.click(dismiss())
        expect(document.querySelector('[data-slot="toast"]')).toBeNull()
    })

    it('bounds the tenant popup to the viewport and lets the keyboard reach the last of twenty tenants', async () => {
        const user = userEvent.setup()
        const base = fxSwitcher()
        const many = Array.from({ length: 20 }, (_, i) => tenant({ id: `tn-${String(i).padStart(16, '0')}`, label: `Tenant ${String(i + 1).padStart(2, '0')}` }))
        const native = await launch({ ...base, tenants: many, active: { tenant: many[0]!.id, context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' } })
        await native.pushMenu({ kind: 'tenantSwitcher' })
        const list = await screen.findByRole('group', { name: 'Switch tenant' })
        expect(list.style.maxHeight).toMatch(/calc\(100vh - \d+px\)/)
        expect(list.className).toContain('overflow-y-auto')
        const buttons = within(list).getAllByRole('button')
        expect(buttons).toHaveLength(21)
        await waitFor(() => expect(buttons[0]).toHaveFocus())
        for (let i = 0; i < 19; i += 1) await user.tab()
        expect(buttons[19]).toHaveFocus()
        await user.keyboard('{Enter}')
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: many[19]!.id })
    })

    it('names the intended tenant in a refused switch and the read tenant in a refused read, even after the context changes', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        native.respond = () => ({ outcome: 'refusal', refusal: 'invalidInput' })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: /Meridian Retail/ }))
        expect(await screen.findByText('The switch to Meridian Retail was refused (invalidInput).')).toBeInTheDocument()
        // Palette: Read now with a late refusal after the active context moved on.
        let answer: (value: unknown) => void = () => undefined
        native.respond = () => new Promise((resolve) => (answer = resolve))
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        await user.type(screen.getByRole('combobox'), 'read{Enter}')
        await native.push({ ...native.current, active: { tenant: 'tn-fedcba9876543210', context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' } })
        await act(async () => {
            answer({ outcome: 'refusal', refusal: 'deniedStore' })
            await new Promise((resolve) => setTimeout(resolve, 0))
        })
        expect(await screen.findByText('The read of Northwind Health was refused (deniedStore).')).toBeInTheDocument()
    })

    it('tells two tenants with the same label and region apart by TSG, and names the current tenant on the trigger away from Home', async () => {
        const user = userEvent.setup()
        const a = tenant({ id: 'tn-0123456789abcdef', label: 'Acme', tsgId: '1111111111' })
        const b = tenant({ id: 'tn-fedcba9876543210', label: 'Acme', tsgId: '2222222222' })
        const native = await launch({ ...fxHome(), tenants: [a, b], active: { tenant: a.id, context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' }, activeFeature: 'harness' })
        expect(screen.getByRole('button', { name: 'Switch tenant: Acme · europe · TSG 1111111111' })).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        const rows = within(screen.getByRole('group', { name: 'Switch tenant' })).getAllByRole('button')
        expect(rows.map((r) => r.textContent)).toEqual(['Acmeeurope · TSG 1111111111', 'Acmeeurope · TSG 2222222222', 'Add a tenant…'])
        await user.click(rows[1]!)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: b.id })
        await native.push({ ...native.current, active: { tenant: b.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' } })
        expect(screen.getByRole('button', { name: 'Switch tenant: Acme · europe · TSG 2222222222' })).toBeInTheDocument()
    })

    it('identifies identically named tenants in the palette by environment and TSG, and ArrowDown moves the current row', async () => {
        const user = userEvent.setup()
        const active = tenant({ id: 'tn-0000000000000001', label: 'Northwind Health', tsgId: '1762204418' })
        const a = tenant({ id: 'tn-0123456789abcdef', label: 'Acme', tsgId: '1111111111' })
        const b = tenant({ id: 'tn-fedcba9876543210', label: 'Acme', tsgId: '2222222222', destination: { kind: 'region', region: 'unitedStates' } })
        const native = await launch({ ...fxHome(), tenants: [active, a, b], active: { tenant: active.id, context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' } })
        await user.click(rail().getByRole('button', { name: /Go to…/ }))
        const dialog = screen.getByRole('dialog', { name: 'Command palette' })
        const labels = within(dialog).getAllByRole('option').map((o) => o.textContent)
        expect(labels).toContain('Switch to Acme · europe · TSG 1111111111')
        expect(labels).toContain('Switch to Acme · unitedStates · TSG 2222222222')
        // ArrowDown moves the current row one step at a time (aria-current follows).
        const current = () => within(dialog).getAllByRole('option').find((o) => o.getAttribute('aria-current') === 'true')?.textContent
        expect(current()).toBe('Home⌘1')
        await user.keyboard('{ArrowDown}{ArrowDown}')
        expect(current()).toBe('Harness⌘3')
        await user.type(within(dialog).getByRole('combobox'), '2222{Enter}')
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectTenant', tenant: b.id })
    })
})
