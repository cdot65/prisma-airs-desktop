/**
 * I8 at the shared modal boundary: two stacked dialogs keep keyboard
 * interaction inside the topmost, the lower one is inert and hidden, Escape
 * closes only the top, and each opener gets its focus back.
 */
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { useState } from 'react'
import { Modal } from './modal'
import { resetShellStore, useShellStore } from './store'

function Stack() {
    const [outer, setOuter] = useState(false)
    const [inner, setInner] = useState(false)
    return (
        <div>
            <button type='button' onClick={() => setOuter(true)}>
                Open outer
            </button>
            <button type='button'>Page control</button>
            {outer && (
                <Modal label='Outer' onClose={() => setOuter(false)}>
                    <button type='button' onClick={() => setInner(true)}>
                        Open inner
                    </button>
                    <button type='button'>Outer second</button>
                </Modal>
            )}
            {inner && (
                <Modal label='Inner' onClose={() => setInner(false)}>
                    <input aria-label='Inner field' />
                    <button type='button'>Inner done</button>
                </Modal>
            )}
        </div>
    )
}

beforeEach(() => resetShellStore())

describe('modal stack (I8)', () => {
    it('keeps only the topmost dialog interactive, traps Tab, closes top-first on Escape and restores each opener', async () => {
        const user = userEvent.setup()
        render(<Stack />)
        const openOuter = screen.getByRole('button', { name: 'Open outer' })
        openOuter.focus()
        await user.keyboard('{Enter}')
        const outer = screen.getByRole('dialog', { name: 'Outer' })
        expect(outer).toHaveAttribute('aria-modal', 'true')
        expect(screen.getByRole('button', { name: 'Open inner' })).toHaveFocus()
        // Tab wraps inside the outer dialog.
        await user.tab()
        expect(screen.getByRole('button', { name: 'Outer second' })).toHaveFocus()
        await user.tab()
        expect(screen.getByRole('button', { name: 'Open inner' })).toHaveFocus()
        await user.tab({ shift: true })
        expect(screen.getByRole('button', { name: 'Outer second' })).toHaveFocus()
        // Open the inner dialog: the outer one becomes inert and hidden.
        await user.click(screen.getByRole('button', { name: 'Open inner' }))
        const inner = screen.getByRole('dialog', { name: 'Inner' })
        expect(inner).toHaveAttribute('aria-modal', 'true')
        expect(outer).not.toHaveAttribute('aria-modal')
        expect(outer).toHaveAttribute('data-topmost', 'false')
        expect(outer.parentElement).toHaveAttribute('aria-hidden', 'true')
        expect(outer.parentElement).toHaveAttribute('inert')
        expect(useShellStore.getState().modals).toHaveLength(2)
        expect(screen.getByRole('textbox', { name: 'Inner field' })).toHaveFocus()
        await user.tab()
        await user.tab()
        expect(screen.getByRole('textbox', { name: 'Inner field' })).toHaveFocus()
        // Escape closes the inner dialog only; focus returns to its opener.
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog', { name: 'Inner' })).toBeNull()
        expect(screen.getByRole('dialog', { name: 'Outer' })).toHaveAttribute('aria-modal', 'true')
        expect(screen.getByRole('button', { name: 'Open inner' })).toHaveFocus()
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog')).toBeNull()
        expect(openOuter).toHaveFocus()
        expect(useShellStore.getState().modals).toHaveLength(0)
    })

    it('restores the exact non-first opener of a stacked dialog', async () => {
        const user = userEvent.setup()
        render(<Stack />)
        await user.click(screen.getByRole('button', { name: 'Open outer' }))
        // Open the inner dialog from the outer dialog's second control.
        const second = screen.getByRole('button', { name: 'Outer second' })
        second.focus()
        await user.keyboard('{Enter}')
        expect(screen.queryByRole('dialog', { name: 'Inner' })).toBeNull()
        // 'Outer second' does nothing on Enter; open inner via its own control but with
        // the opener recorded as 'Outer second' by focusing it right before the click.
        second.focus()
        const openInner = screen.getByRole('button', { name: 'Open inner' })
        openInner.addEventListener('mousedown', (e) => e.preventDefault(), { once: true })
        await user.click(openInner)
        expect(screen.getByRole('dialog', { name: 'Inner' })).toHaveAttribute('aria-modal', 'true')
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('dialog', { name: 'Inner' })).toBeNull()
        await waitFor(() => expect(screen.getByRole('button', { name: 'Outer second' })).toHaveFocus())
    })

    it('Escape closes only the topmost dialog even when focus has left it (a native sheet returned focus to the page)', async () => {
        const user = userEvent.setup()
        render(<Stack />)
        await user.click(screen.getByRole('button', { name: 'Open outer' }))
        await user.click(screen.getByRole('button', { name: 'Open inner' }))
        ;(document.activeElement as HTMLElement | null)?.blur()
        expect(document.activeElement).toBe(document.body)
        await user.keyboard('{Escape}')
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Inner' })).toBeNull())
        expect(screen.getByRole('dialog', { name: 'Outer' })).toBeInTheDocument()
    })

    it('returns focus to the page heading when the opener no longer exists', async () => {
        const user = userEvent.setup()
        function Gone() {
            const [open, setOpen] = useState(false)
            const [opener, setOpener] = useState(true)
            return (
                <main>
                    <h1 tabIndex={-1}>Page</h1>
                    {opener && (
                        <button type='button' onClick={() => setOpen(true)}>
                            Open once
                        </button>
                    )}
                    {open && (
                        <Modal label='Once' onClose={() => setOpen(false)}>
                            <button type='button' onClick={() => setOpener(false)}>
                                Finish
                            </button>
                        </Modal>
                    )}
                </main>
            )
        }
        render(<Gone />)
        await user.click(screen.getByRole('button', { name: 'Open once' }))
        await user.click(screen.getByRole('button', { name: 'Finish' }))
        await user.keyboard('{Escape}')
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Once' })).toBeNull())
        await waitFor(() => expect(screen.getByRole('heading', { name: 'Page' })).toHaveFocus())
    })
})
