/**
 * The production App and desktop shell composition over a scripted native
 * transport: navigation, keyboard/focus behavior, native menu commands,
 * Rust-owned appearance, truthful refusal reporting and Academy isolation.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
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
        // Administration is available (C08) and asks for a tenant first.
        await user.click(nav().getByRole('button', { name: /^Administration/ }))
        expect(nav().queryByRole('button', { name: /^Administration.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Tenant inventories' })).toBeInTheDocument()
        expect(screen.getByText('Add a tenant in Settings to read its inventories.')).toBeInTheDocument()
        // Sessions & Reports is available (C11) and asks for a tenant first.
        await user.click(nav().getByRole('button', { name: /^Sessions/ }))
        expect(nav().queryByRole('button', { name: /^Sessions.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Investigate sessions' })).toBeInTheDocument()
        expect(screen.getByText('Add a tenant in Settings to investigate its sessions.')).toBeInTheDocument()
        // The Case Library is available (C12) and asks for a tenant first.
        await user.click(nav().getByRole('button', { name: /^Case Library/ }))
        expect(nav().queryByRole('button', { name: /^Case Library.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Test case library' })).toBeInTheDocument()
        // Red Team & Models is available (C13) and asks for a tenant first.
        await user.click(nav().getByRole('button', { name: /^Red Team/ }))
        expect(nav().queryByRole('button', { name: /^Red Team.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Red Teaming and model observation' })).toBeInTheDocument()
        expect(screen.getByText('Add a tenant in Settings to observe its Red Teaming and model scans.')).toBeInTheDocument()
        // The API Client is available (C14), independent of any tenant.
        await user.click(nav().getByRole('button', { name: /^API Client/ }))
        expect(nav().queryByRole('button', { name: /^API Client.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'API Client' })).toBeInTheDocument()
        expect(screen.getByText('Opening the API Client…')).toBeInTheDocument()
        // AI Gateway is available (C16) and asks for a tenant first.
        await user.click(nav().getByRole('button', { name: /^AI Gateway/ }))
        expect(nav().queryByRole('button', { name: /^AI Gateway.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Gateway workspaces and configuration' })).toBeInTheDocument()
        expect(screen.getByText('Add a tenant in Settings to read its AI Gateway workspaces.')).toBeInTheDocument()
        // The native Harness is available (C19); this build's fixture shell
        // bundles no engine, and the view says so.
        await user.click(nav().getByRole('button', { name: /^Harness/ }))
        expect(nav().queryByRole('button', { name: /^Harness.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Harness' })).toBeInTheDocument()
        expect(screen.getByText('The Harness engine is not bundled with this build.')).toBeInTheDocument()
        // Import from v1 is available (C23): nothing is read until asked.
        await user.click(nav().getByRole('button', { name: /^Import from v1/ }))
        expect(nav().queryByRole('button', { name: /^Import from v1.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('heading', { level: 1, name: 'Import from v1' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Look for v1 data' })).toBeInTheDocument()
        expect(screen.getByText('Fixture build')).toBeInTheDocument()
        // Academy learning is available (C05) and opens its learner region.
        await user.click(nav().getByRole('button', { name: /^Academy/ }))
        expect(nav().queryByRole('button', { name: /^Academy.*Not available in this build/ })).toBeNull()
        expect(screen.getByRole('region', { name: 'Academy' })).toBeInTheDocument()
        // The rail must scroll rather than clip destinations at high zoom or
        // the minimum window size.
        expect(document.querySelector('[data-slot="rail"]')).toHaveClass('overflow-y-auto')
        // The page ends with room for its last control to clear the 60px fade.
        expect(document.querySelector('[data-slot="shell-status"]')).toHaveClass('pb-14')
    })

    it('supports keyboard-only navigation and moves focus to the new heading', async () => {
        const user = userEvent.setup()
        await launch()
        const sessions = nav().getByRole('button', { name: /^Sessions/ })
        while (document.activeElement !== sessions) await user.tab()
        await user.keyboard('{Enter}')
        const heading = screen.getByRole('heading', { level: 1, name: 'Investigate sessions' })
        expect(heading).toHaveFocus()
        expect(sessions).toHaveAttribute('aria-current', 'page')
        expect(nav().getByRole('button', { name: /Runtime Security/ })).not.toHaveAttribute('aria-current')
    })

    it('hands focus to the Academy page heading once the page renders', async () => {
        const user = userEvent.setup()
        await launch()
        // Academy's lab section reads matchMedia, which jsdom lacks.
        const matchMedia = vi
            .spyOn(window, 'matchMedia')
            .mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() } as never)
        try {
            const academy = nav().getByRole('button', { name: /^Academy/ })
            while (document.activeElement !== academy) await user.tab()
            await user.keyboard('{Enter}')
            const heading = await screen.findByRole('heading', { level: 1, name: 'What will you take up next?' }, { timeout: 5000 })
            await waitFor(() => expect(heading).toHaveFocus())
            expect(academy).toHaveAttribute('aria-current', 'page')
        } finally {
            matchMedia.mockRestore()
        }
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

    it('keeps a focused page control clear of the bottom fade and the topbar, but not rail controls', async () => {
        const user = userEvent.setup()
        await launch()
        await user.click(nav().getByRole('button', { name: /Settings/ }))
        const scrollBy = vi.fn()
        const original = window.scrollBy
        window.scrollBy = scrollBy as unknown as typeof window.scrollBy
        document.documentElement.style.scrollPaddingTop = '72px'
        document.documentElement.style.scrollPaddingBottom = '76px'
        // The correction runs on the frame after WebKit's own focus scroll.
        const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
            callback(0)
            return 0
        })
        const at = (element: Element, top: number, height = 44) =>
            vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({ top, bottom: top + height, height } as DOMRect)
        try {
            // Under the fade: scrolled up by exactly the overlap.
            const light = screen.getByRole('radio', { name: /Light/ })
            at(light, window.innerHeight - 70)
            light.focus()
            expect(scrollBy).toHaveBeenLastCalledWith(0, 50)
            // Under the sticky topbar: scrolled down to the padding.
            const dark = screen.getByRole('radio', { name: /Dark/ })
            at(dark, 20)
            dark.focus()
            expect(scrollBy).toHaveBeenLastCalledWith(0, -52)
            // Inside the padded area: left alone; rail controls never scroll the page.
            scrollBy.mockClear()
            const system = screen.getByRole('radio', { name: /Match System/ })
            at(system, 300)
            system.focus()
            const rail = nav().getByRole('button', { name: /Academy/ })
            at(rail, window.innerHeight - 20)
            rail.focus()
            expect(scrollBy).not.toHaveBeenCalled()
        } finally {
            frame.mockRestore()
            window.scrollBy = original
            document.documentElement.style.scrollPaddingTop = ''
            document.documentElement.style.scrollPaddingBottom = ''
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
        await user.keyboard('harn')
        expect(within(dialog).getAllByRole('option').map((o) => o.textContent)).toEqual(['Harness'])
        await user.keyboard('{Enter}')
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'Harness' })).toHaveFocus()

        // Native navigation while the modal palette is open closes it and
        // focuses the destination heading (never focus behind a modal).
        s.push({ kind: 'menu', command: { kind: 'commandPalette' } })
        expect(screen.getByRole('dialog')).toBeInTheDocument()
        s.push({ kind: 'menu', command: { kind: 'navigate', feature: 'library' } })
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(screen.getByRole('heading', { level: 1, name: 'Test case library' })).toHaveFocus()

        const trigger = screen.getByRole('button', { name: /Go to/ })
        await user.click(trigger)
        expect(screen.getByRole('combobox')).toHaveFocus()
        await user.keyboard('{Tab}')
        expect(screen.getByRole('combobox')).toHaveFocus()
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
        expect(trigger).toHaveFocus()
    })

    it('forgets Academy presentation storage when a reset completes, and keeps it when one stops', async () => {
        const s = await launch()
        const seed = () => {
            localStorage.setItem('academy-completion-moments:user_2fixturelearner', '{"v":1}')
            localStorage.setItem('unrelated', 'kept')
        }
        seed()
        s.push({ kind: 'shell', snapshot: { ...base, revision: '2', storage: 'resetting' } })
        s.push({ kind: 'shell', snapshot: { ...base, revision: '3', storage: 'ready', resetFailure: { stage: 'credentials', reason: 'unavailable' } } })
        expect(localStorage.getItem('academy-completion-moments:user_2fixturelearner')).not.toBeNull()
        s.push({ kind: 'shell', snapshot: { ...base, revision: '4', storage: 'resetting' } })
        s.push({ kind: 'shell', snapshot: { ...base, revision: '5', storage: 'ready', resetFailure: null } })
        expect(localStorage.getItem('academy-completion-moments:user_2fixturelearner')).toBeNull()
        expect(localStorage.getItem('unrelated')).toBe('kept')
        localStorage.clear()
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
