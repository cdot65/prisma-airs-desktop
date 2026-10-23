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
        await user.click(within(group).getByRole('button', { name: 'Export diagnostics…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'exportDiagnostics' }))
        await native.push(snapshot({ export: 'saving' }))
        expect(within(group).getByRole('button', { name: 'Export diagnostics…' })).toBeDisabled()
        await native.push(snapshot({ export: 'saved' }))
        expect(await within(group).findByRole('status')).toHaveTextContent('Diagnostics saved.')
        await user.click(within(group).getByRole('button', { name: 'Clear diagnostics' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearDiagnostics' }))
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
