/**
 * The production App and desktop shell composition over a scripted native
 * transport: navigation, keyboard/focus behavior, native menu commands,
 * Rust-owned appearance, truthful refusal reporting and Academy isolation.
 */
import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient, type DesktopTransport, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import shellResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-shell-response.json'
import { App } from '../app'

const base = shellResponse.snapshot as ShellSnapshot

interface Scripted {
    transport: DesktopTransport
    push(update: unknown): void
    /** Delivers an update and lets the async work it starts settle in act. */
    pushAsync(update: unknown): Promise<void>
    dispatched: unknown[]
    respond: (body: Record<string, unknown>) => unknown
}

function scripted(): Scripted {
    const listeners: ((update: unknown) => void)[] = []
    const decode = (bytes: Uint8Array) => JSON.parse(new TextDecoder().decode(bytes)) as Record<string, unknown>
    const state: Scripted = {
        dispatched: [],
        respond: () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }),
        push: (update) => act(() => listeners.at(-1)?.(update)),
        pushAsync: (update) =>
            act(async () => {
                listeners.at(-1)?.(update)
                await new Promise((resolve) => setTimeout(resolve, 0))
            }),
        transport: {
            async observe(bytes) {
                const body = decode(bytes)
                const scope = body.scope as { kind: string }
                if (scope.kind === 'deliverySession') {
                    return { kind: 'deliverySession', session: `ds-${'a'.repeat(32)}`, nextSequence: '1' }
                }
                return { kind: 'shell', snapshot: base }
            },
            async dispatch(bytes) {
                const body = decode(bytes)
                state.dispatched.push(body.intent)
                return state.respond(body)
            },
            openChannel(onMessage) {
                listeners.push(onMessage)
                return { token: `__CHANNEL__:${listeners.length}`, close: () => undefined }
            },
        },
    }
    return state
}

async function launch(s = scripted()) {
    render(<App client={new DesktopClient(s.transport)} />)
    await screen.findByRole('navigation', { name: 'Features' })
    return s
}

const nav = () => within(screen.getByRole('navigation', { name: 'Features' }))

let setItem: ReturnType<typeof vi.spyOn>
beforeEach(() => {
    setItem = vi.spyOn(Storage.prototype, 'setItem')
    document.documentElement.removeAttribute('data-theme')
})
afterEach(() => setItem.mockRestore())

describe('desktop shell', () => {
    it('lists every native feature and shows unavailable destinations explicitly', async () => {
        const user = userEvent.setup()
        await launch()
        const buttons = nav().getAllByRole('button')
        expect(buttons.map((b) => b.textContent?.replace('Soon', '').trim())).toEqual(base.features.map((f) => f.label))
        // Runtime Security is the available landing destination (C03).
        expect(nav().getByRole('button', { name: /Runtime Security/ })).toHaveAttribute('aria-current', 'page')
        expect(nav().queryByRole('button', { name: /Runtime Security.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Scan a prompt or response' })).toBeInTheDocument()
        expect(screen.getByText('Add a tenant and its scan key in Settings to run scans.')).toBeInTheDocument()
        await user.click(nav().getByRole('button', { name: /^Academy/ }))
        expect(nav().getByRole('button', { name: /^Academy.*Not available in this build/ })).toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'Academy' })).toBeInTheDocument()
        expect(screen.getByText(/is not available in this build/)).toBeInTheDocument()
        expect(screen.getByText('Planned in C04')).toBeInTheDocument()
        expect(screen.getByText('Fixture build')).toBeInTheDocument()
        // The rail must scroll rather than clip destinations at high zoom or
        // the minimum window size.
        expect(document.querySelector('[data-slot="rail"]')).toHaveClass('overflow-y-auto')
    })

    it('supports keyboard-only navigation and moves focus to the new heading', async () => {
        const user = userEvent.setup()
        await launch()
        const academy = nav().getByRole('button', { name: /^Academy/ })
        while (document.activeElement !== academy) await user.tab()
        await user.keyboard('{Enter}')
        const heading = screen.getByRole('heading', { level: 1, name: 'Academy' })
        expect(heading).toHaveFocus()
        expect(academy).toHaveAttribute('aria-current', 'page')
        expect(nav().getByRole('button', { name: /Runtime Security/ })).not.toHaveAttribute('aria-current')
    })

    it('scrolls a control into view when arrow keys move focus within a radio group', async () => {
        const user = userEvent.setup()
        await launch()
        await user.click(nav().getByRole('button', { name: /Settings/ }))
        const scrolled = vi.spyOn(Element.prototype, 'scrollIntoView')
        try {
            screen.getByRole('radio', { name: /Match System/ }).focus()
            await user.keyboard('{ArrowDown}')
            const light = screen.getByRole('radio', { name: /Light/ })
            expect(light).toHaveFocus()
            expect(scrolled.mock.contexts.at(-1)).toBe(light)
            expect(scrolled).toHaveBeenLastCalledWith({ block: 'nearest' })
        } finally {
            scrolled.mockRestore()
        }
    })

    it('handles native menu commands and restores focus when the palette closes', async () => {
        const user = userEvent.setup()
        const s = await launch()
        s.push({ kind: 'menu', command: { kind: 'navigate', feature: 'settings' } })
        expect(screen.getByRole('heading', { level: 1, name: 'Settings' })).toHaveFocus()

        s.push({ kind: 'menu', command: { kind: 'commandPalette' } })
        const dialog = screen.getByRole('dialog', { name: 'Command palette' })
        const field = within(dialog).getByRole('combobox', { name: 'Go to destination' })
        expect(field).toHaveFocus()
        await user.keyboard('gate')
        expect(within(dialog).getAllByRole('option').map((o) => o.textContent)).toEqual(['AI GatewayNot available'])
        await user.keyboard('{Enter}')
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'AI Gateway' })).toHaveFocus()

        // Native navigation while the modal palette is open closes it and
        // focuses the destination heading (never focus behind a modal).
        s.push({ kind: 'menu', command: { kind: 'commandPalette' } })
        expect(screen.getByRole('dialog')).toBeInTheDocument()
        s.push({ kind: 'menu', command: { kind: 'navigate', feature: 'academy' } })
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'Academy' })).toHaveFocus()

        const trigger = screen.getByRole('button', { name: /Go to/ })
        await user.click(trigger)
        expect(screen.getByRole('combobox')).toHaveFocus()
        await user.keyboard('{Tab}')
        expect(screen.getByRole('combobox')).toHaveFocus()
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(trigger).toHaveFocus()
    })

    it('applies the native theme in the same commit that first shows the shell', async () => {
        // A MutationObserver callback runs right after the commit inserts the
        // shell, before any deferred effect, so it sees what is painted.
        let themeWhenShown: string | undefined | null = null
        const observer = new MutationObserver(() => {
            if (themeWhenShown === null && document.querySelector('nav[aria-label="Features"]')) {
                themeWhenShown = document.documentElement.dataset.theme
            }
        })
        observer.observe(document.body, { childList: true, subtree: true })
        try {
            await launch()
        } finally {
            observer.disconnect()
        }
        expect(themeWhenShown).toBe('gruvbox')
    })

    it('treats appearance as Rust-owned: dispatches, then applies the native snapshot', async () => {
        const user = userEvent.setup()
        const s = await launch()
        expect(document.documentElement.dataset.theme).toBe('gruvbox')
        await user.click(nav().getByRole('button', { name: /Settings/ }))
        const dark = screen.getByRole('radio', { name: /Dark/ })
        const system = screen.getByRole('radio', { name: /Match System/ })
        expect(system).toBeChecked()

        await user.click(screen.getByRole('radio', { name: /Light/ }))
        await act(async () => {})
        expect(s.dispatched).toEqual([{ kind: 'setAppearance', theme: 'light' }])
        expect(system).toBeChecked()
        expect(document.documentElement.dataset.theme).toBe('gruvbox')

        s.push({ kind: 'shell', snapshot: { ...base, revision: '2', appearance: 'light' } })
        expect(screen.getByRole('radio', { name: /Light/ })).toBeChecked()
        expect(document.documentElement.dataset.theme).toBe('paper')

        await s.pushAsync({ kind: 'menu', command: { kind: 'setAppearance', theme: 'dark' } })
        expect(s.dispatched.at(-1)).toEqual({ kind: 'setAppearance', theme: 'dark' })
        s.push({ kind: 'shell', snapshot: { ...base, revision: '3', appearance: 'dark' } })
        expect(dark).toBeChecked()
        expect(document.documentElement.dataset.theme).toBe('gruvbox')
    })

    it('reports refusals and unconfirmed outcomes without claiming the change applied', async () => {
        const user = userEvent.setup()
        const s = scripted()
        s.respond = () => ({ outcome: 'refusal', refusal: 'busy' })
        await launch(s)
        await user.click(nav().getByRole('button', { name: /Settings/ }))
        await user.click(screen.getByRole('radio', { name: /Dark/ }))
        expect(await screen.findByText(/refused the appearance change \(busy\)/)).toBeInTheDocument()
        expect(screen.getByRole('radio', { name: /Match System/ })).toBeChecked()

        s.respond = () => {
            throw new Error('lost')
        }
        await user.click(screen.getByRole('radio', { name: /Light/ }))
        expect(await screen.findByText(/did not confirm the appearance change/)).toBeInTheDocument()
        expect(screen.getByRole('radio', { name: /Match System/ })).toBeChecked()
    })

    it('mounts no Academy learner, gamification or persistence effects and makes no network calls', async () => {
        const fetchSpy = vi.spyOn(globalThis, 'fetch')
        await launch()
        for (const slot of ['notifications-bell', 'xp-bar', 'credit-pill', 'streak-flame', 'mobile-tab-bar', 'theme-toggle']) {
            expect(document.querySelector(`[data-slot="${slot}"]`)).toBeNull()
        }
        expect(setItem).not.toHaveBeenCalled()
        expect(fetchSpy).not.toHaveBeenCalled()
        expect(document.querySelector('[data-slot="app-shell"]')).not.toBeNull()
        fetchSpy.mockRestore()
    })
})
