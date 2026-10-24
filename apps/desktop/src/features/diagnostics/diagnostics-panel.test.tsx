/**
 * Local redacted diagnostics in Settings (C24): what is kept and for how
 * long is stated, nothing is exported until asked, and the export's outcome
 * is announced.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type DiagnosticsView, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, scriptedNative } from '../../test/native'

const snapshot = (view: Partial<DiagnosticsView> = {}): ShellSnapshot => ({
    ...baseSnapshot,
    diagnostics: { ...baseSnapshot.diagnostics, events: 12, ...view },
})

describe('diagnostics (C24)', () => {
    it('states what is kept, exports and clears only when asked, and announces the export', async () => {
        const native = scriptedNative(snapshot())
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Settings/ }))
        const group = await screen.findByRole('group', { name: 'Diagnostics' })
        expect(within(group).getByText(/Kept only on this Mac for 14 days and never sent anywhere/)).toBeInTheDocument()
        expect(within(group).getByText(/Prompts, responses, keys, tokens and names are never kept/)).toBeInTheDocument()
        expect(within(group).getByText('12 events kept.')).toBeInTheDocument()
        expect(native.dispatched.some((i) => i.kind === 'exportDiagnostics')).toBe(false)
        const exportButton = within(group).getByRole('button', { name: 'Export diagnostics…' })
        exportButton.focus()
        await user.keyboard(' ')
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'exportDiagnostics' }))
        await native.push(snapshot({ export: 'saving' }))
        // Held while the save dialog is open, without losing keyboard focus,
        // so focus is on Export when the dialog closes.
        expect(exportButton).toHaveAttribute('aria-disabled', 'true')
        expect(exportButton).toHaveFocus()
        await user.keyboard(' ')
        expect(native.dispatched.filter((i) => i.kind === 'exportDiagnostics')).toHaveLength(1)
        await native.push(snapshot({ export: 'saved' }))
        expect(await within(group).findByRole('status')).toHaveTextContent('Diagnostics saved.')
        expect(exportButton).toHaveFocus()
        expect(exportButton).toHaveAttribute('aria-disabled', 'false')
        await user.click(within(group).getByRole('button', { name: 'Clear diagnostics' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearDiagnostics' }))
    })

    it('announces a clearing, and says when one could not remove everything', async () => {
        const native = scriptedNative(snapshot({ events: 12, clear: 'failed' }))
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Settings/ }))
        const group = await screen.findByRole('group', { name: 'Diagnostics' })
        // The events still kept are counted, and the failure is an alert.
        expect(within(group).getByText('12 events kept.')).toBeInTheDocument()
        expect(within(group).getByRole('alert')).toHaveTextContent(/could not all be removed, so the events counted above are still kept/)
        // Retried by keyboard: Clear is reachable and admitted again.
        within(group).getByRole('button', { name: 'Clear diagnostics' }).focus()
        await user.keyboard('{Enter}')
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearDiagnostics' }))
        await native.push(snapshot({ events: 2, clear: 'cleared' }))
        expect(await within(group).findByRole('status')).toHaveTextContent('Diagnostics cleared.')
        expect(within(group).queryByRole('alert')).toBeNull()
    })

    it('says when a build keeps none', async () => {
        const native = scriptedNative(snapshot({ available: false }))
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Settings/ }))
        const group = await screen.findByRole('group', { name: 'Diagnostics' })
        expect(within(group).getByText('This build keeps no diagnostics.')).toBeInTheDocument()
        expect(within(group).queryByRole('button')).toBeNull()
    })
})
