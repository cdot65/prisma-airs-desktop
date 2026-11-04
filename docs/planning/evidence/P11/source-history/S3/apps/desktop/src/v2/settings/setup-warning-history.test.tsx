import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { scriptedNative } from '../../test/native'
import { fxInbox } from '../fixtures'
import { resetShellStore } from '../store'

beforeEach(() => resetShellStore())

describe('dated setup warning history', () => {
    it('keeps the original outcome/date beside a later resolution and distinguishes unknown-scope checks', async () => {
        const base = fxInbox()
        const warnings = [
            { id: 'vw-21', product: 'modelSkill' as const, reason: 'Original model source was forbidden.', recordedSecs: 1_790_000_000, recheck: { checkedSecs: 1_790_000_100, state: 'pass' as const, reason: 'Matching source reads passed.', equivalent: true, basis: 'recordedScope' as const }, resolvedSecs: 1_790_000_100 },
            { id: 'vw-22', product: 'gateway' as const, reason: 'Original Gateway workspace IAM check failed.', recordedSecs: 1_790_000_001, recheck: { checkedSecs: 1_790_000_101, state: 'pass' as const, reason: 'Current workspace read passed.', equivalent: false, basis: 'unknown' as const } },
        ]
        const native = scriptedNative({ ...base, activeFeature: 'settings', tenants: base.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: warnings } : t) })
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        const diagnostics = await screen.findByRole('group', { name: 'Diagnostics' })
        const history = within(diagnostics).getByRole('region', { name: 'Setup check history' })
        expect(within(history).getByText('Original model source was forbidden.')).toBeInTheDocument()
        expect(within(history).getByText('Matching source reads passed.')).toBeInTheDocument()
        expect(Array.from(history.querySelectorAll('[data-warning="vw-21"] time')).map((time) => time.getAttribute('datetime'))).toEqual([new Date(1_790_000_000_000).toISOString(), new Date(1_790_000_100_000).toISOString(), new Date(1_790_000_100_000).toISOString()])
        expect(history.querySelector('[data-warning="vw-21"] [data-state="resolved"]')).not.toBeNull()
        expect(within(history).getByText('Original Gateway workspace IAM check failed.')).toBeInTheDocument()
        expect(history.querySelector('[data-warning="vw-22"] [data-state="active"]')).not.toBeNull()
        expect(within(history).getByText(/scope could not be matched/i)).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'exportDiagnostics' || i.kind === 'clearDiagnostics')).toHaveLength(0)
        await user.click(within(diagnostics).getByRole('button', { name: 'Export diagnostics…' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'exportDiagnostics' })
        await user.click(within(diagnostics).getByRole('button', { name: 'Clear diagnostics' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'clearDiagnostics' })
        await native.push({ ...native.current, diagnostics: { ...native.current.diagnostics, events: 0, clear: 'cleared' } })
        expect(within(history).getByText('Original model source was forbidden.')).toBeInTheDocument()
        expect(within(history).getByText('Original Gateway workspace IAM check failed.')).toBeInTheDocument()
    })
})
