import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../app'
import { scriptedNative } from '../test/native'
import { fxHome, fxSwitcher } from './fixtures'
import { resetShellStore } from './store'

async function launch(snapshot: ShellSnapshot = fxHome()) {
    const native = scriptedNative(snapshot)
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('navigation', { name: 'Destinations' })
    return native
}

beforeEach(() => resetShellStore())

describe('refined shell page and Explain ownership', () => {
    it('puts the sole page title and navigation focus in the header while retaining the sidebar Harness action', async () => {
        const native = await launch(fxSwitcher())
        const header = document.querySelector('[data-slot="app-header"]') as HTMLElement
        const home = within(header).getByRole('heading', { level: 1, name: 'Home' })
        expect(screen.getAllByRole('heading', { level: 1 })).toEqual([home])
        expect(screen.getByRole('main')).toHaveAttribute('aria-labelledby', home.id)
        expect(within(header).queryByRole('button', { name: 'Ask Harness' })).toBeNull()
        expect(within(screen.getByRole('navigation', { name: 'Destinations' })).getByRole('button', { name: /Harness/ })).toBeInTheDocument()
        expect(screen.getByRole('main').querySelector('[data-slot="page-intro"] [data-slot="page-actions"]')).not.toBeNull()
        await native.push({ ...native.current, activeFeature: 'inbox' })
        const inbox = within(header).getByRole('heading', { level: 1, name: 'Inbox' })
        await waitFor(() => expect(inbox).toHaveFocus())
        expect(screen.getAllByRole('heading', { level: 1 })).toEqual([inbox])
        expect(within(screen.getByRole('main')).getByRole('button', { name: 'Refresh' })).toBeInTheDocument()
    })

    it('keeps Explain outside page layout and lets navigation continue while it is open', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxHome(), showCalls: true, calls: [] })
        const panel = screen.getByRole('complementary', { name: 'The calls' })
        const drawer = panel.closest('[data-slot="explain-drawer"]')
        expect(drawer).not.toBeNull()
        expect(drawer).toHaveStyle({ position: 'absolute', overflowY: 'auto' })
        expect(drawer).not.toHaveAttribute('aria-modal', 'true')
        expect(screen.getByRole('main')).not.toHaveAttribute('inert')
        await user.click(within(screen.getByRole('navigation', { name: 'Destinations' })).getByRole('button', { name: /Inbox/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'inbox' })
        await native.push({ ...native.current, activeFeature: 'inbox' })
        expect(screen.getByRole('complementary', { name: 'The calls' })).toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'Inbox' })).toHaveFocus()
    })

    it('gives Explain its own Close/Escape and yields to a higher dialog without trapping the page', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxHome(), showCalls: false, calls: undefined })
        const explain = screen.getByRole('switch', { name: 'Show the calls' })
        await user.click(explain)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setShowCalls', on: true })
        await native.push({ ...native.current, showCalls: true, calls: [] })
        const close = screen.getByRole('button', { name: 'Close Explain' })
        await waitFor(() => expect(close).toHaveFocus())
        await user.click(screen.getByRole('button', { name: /Search or go to/ }))
        expect(screen.getByRole('dialog', { name: 'Command palette' })).toBeInTheDocument()
        const before = native.dispatched.filter((i) => i.kind === 'setShowCalls').length
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog', { name: 'Command palette' })).toBeNull()
        expect(native.dispatched.filter((i) => i.kind === 'setShowCalls')).toHaveLength(before)
        expect(screen.getByRole('complementary', { name: 'The calls' })).toBeInTheDocument()
        screen.getByRole('heading', { level: 1, name: 'Home' }).focus()
        await user.keyboard('{Escape}')
        expect(native.dispatched.filter((i) => i.kind === 'setShowCalls')).toHaveLength(before)
        close.focus()
        await user.keyboard('{Escape}')
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setShowCalls', on: false })
        await native.push({ ...native.current, showCalls: false, calls: undefined })
        await waitFor(() => expect(explain).toHaveFocus())
    })

    it('hides a previous tenant’s drawer immediately and waits for its preference close before reopening', async () => {
        const native = await launch({ ...fxSwitcher(), showCalls: true, calls: [] })
        expect(screen.getByRole('complementary', { name: 'The calls' })).toBeInTheDocument()
        const other = native.current.tenants.find((t) => t.id !== native.current.active?.tenant)!
        await native.push({ ...native.current, active: { tenant: other.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' } })
        expect(screen.queryByRole('complementary', { name: 'The calls' })).toBeNull()
        await waitFor(() => expect(native.dispatched.at(-1)).toEqual({ kind: 'setShowCalls', on: false }))
        // An unrelated snapshot before the native preference reply cannot revive it.
        await native.push({ ...native.current, appearance: native.current.appearance })
        expect(screen.queryByRole('complementary', { name: 'The calls' })).toBeNull()
    })
})
