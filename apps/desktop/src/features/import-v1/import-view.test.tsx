/**
 * The reviewed v1 import (C23) through the production App and DesktopClient
 * against a scripted native host: nothing is read until asked, categories
 * and tenant targets are chosen explicitly, the review states every item's
 * status as text, only ready items can be imported, focus moves to the
 * result, an unsettled import offers Roll back, and an undone one Import again.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type ReceiptView, type ShellSnapshot, type V1ImportView } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant } from '../../test/native'

const REVIEW = 'rv-0123456789abcdef'
const RECEIPT = 'imp-0123456789abcdef'
const OURS = 'tn-0123456789abcdef'

function snapshot(view: Partial<V1ImportView> = {}): ShellSnapshot {
    return {
        ...baseSnapshot,
        tenants: [tenant({ id: OURS, label: 'Ours', tsgId: '1234567890' })],
        v1Import: { ...baseSnapshot.v1Import, ...view },
    }
}

const inspected: Partial<V1ImportView> = {
    source: 'ready',
    tenants: [
        { index: 0, label: 'Prod', tsgId: '1234567890', sessionOnly: false, matches: [OURS] },
        { index: 1, label: 'Scratch', tsgId: null, sessionOnly: true, matches: [] },
    ],
    categories: [
        { category: 'tenants', found: 2, problem: null },
        { category: 'cases', found: null, problem: null },
        { category: 'reports', found: 1, problem: null },
        { category: 'academyDrafts', found: null, problem: 'missing' },
    ],
    notMapped: ['navigation'],
}

const receipt = (overrides: Partial<ReceiptView> = {}): ReceiptView => ({
    id: RECEIPT,
    created: 1_790_000_000,
    state: 'applied',
    interrupted: false,
    items: [{ category: 'cases', label: 'Prod', state: 'applied', written: 2, kept: 0 }],
    canResume: false,
    canRollBack: true,
    ...overrides,
})

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Import from v1/ }))
    await screen.findByRole('heading', { level: 1, name: 'Import from v1' })
    return user
}

describe('Import from v1 (C23)', () => {
    it('reads nothing until asked, then reviews the chosen categories and tenants', async () => {
        const native = scriptedNative(snapshot())
        const user = await open(native)
        expect(screen.getByText(/v1 and its data are never changed/)).toBeInTheDocument()
        expect(native.dispatched).toEqual([])
        await user.click(screen.getByRole('button', { name: 'Look for v1 data' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'inspectV1Import' }))
        await native.push(snapshot(inspected))
        expect(await screen.findByText('v1’s data was found. Nothing has been read from its Keychain.')).toBeInTheDocument()
        expect(screen.getByText('Not imported (no equivalent here): navigation.')).toBeInTheDocument()
        // A category v1 does not have is said, not hidden.
        expect(screen.getByRole('checkbox', { name: /Academy lab drafts/ })).toHaveAccessibleName(/v1 has none on this Mac/)
        // The Keychain note appears only once a protected category is chosen.
        await user.click(screen.getByRole('checkbox', { name: /^Tenants/ }))
        expect(screen.queryByText(/Choose Allow, not Always Allow/)).toBeNull()
        await user.click(screen.getByRole('checkbox', { name: /^Saved cases/ }))
        expect(screen.getByText(/Choose Allow, not Always Allow/)).toBeInTheDocument()
        // The tenant with the same TSG is offered as the default target.
        const prod = screen.getByRole('combobox', { name: /^Prod/ })
        expect(prod).toHaveValue(OURS)
        await user.selectOptions(screen.getByRole('combobox', { name: /^Scratch/ }), 'skip')
        await user.click(screen.getByRole('button', { name: 'Review what would be imported' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'planV1Import',
                categories: ['tenants', 'cases'],
                targets: [
                    { source: 0, target: { kind: 'existing', tenant: OURS } },
                    { source: 1, target: { kind: 'skip' } },
                ],
            })
        )
    })

    it('imports only ready items, states every status as text and focuses the result', async () => {
        const plan: V1ImportView['plan'] = {
            review: REVIEW,
            items: [
                {
                    index: 0,
                    category: 'cases',
                    label: 'Prod',
                    target: 'Ours',
                    status: { kind: 'ready' },
                    counts: { new: 1, present: 0, conflict: 1, skipped: 0 },
                    partial: null,
                },
                {
                    index: 1,
                    category: 'reports',
                    label: 'Prod',
                    target: 'Ours',
                    status: { kind: 'alreadyPresent' },
                    counts: { new: 0, present: 1, conflict: 0, skipped: 0 },
                    partial: null,
                },
                {
                    index: 2,
                    category: 'conversations',
                    label: 'Prod',
                    target: 'Ours',
                    status: { kind: 'blocked', problem: 'keychainLocked' },
                    counts: { new: 0, present: 0, conflict: 0, skipped: 0 },
                    partial: null,
                },
            ],
        }
        const native = scriptedNative(snapshot({ ...inspected, plan }))
        const user = await open(native)
        const ready = screen.getByRole('checkbox', { name: /^Saved cases: Prod → Ours/ })
        expect(ready).toBeChecked()
        expect(ready).toHaveAccessibleName(/1 new · 1 in conflict \(kept as they are\)/)
        expect(screen.getByRole('checkbox', { name: /FP\/FN reports: Prod/ })).toBeDisabled()
        expect(screen.getByRole('checkbox', { name: /Harness conversations: Prod/ })).toHaveAccessibleName(
            /Cannot import: the Keychain is locked/
        )
        await user.click(screen.getByRole('button', { name: 'Import 1 item' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'confirmV1Import', review: REVIEW, items: [0] }))
        await native.push(snapshot({ ...inspected, plan: null, activity: 'importing' }))
        expect(await screen.findByText('Importing…')).toBeInTheDocument()
        await native.push(snapshot({ ...inspected, plan: null, receipts: [receipt()] }))
        await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Imports' })).toHaveFocus())
        expect(screen.getByText(/Saved cases: Prod · imported · 2 entries/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Roll back…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'rollBackV1Import', receipt: RECEIPT }))
        // The native sheet is cancelled: focus returns to Roll back.
        await native.push(snapshot({ ...inspected, plan: null, receipts: [receipt()], activity: 'confirming' }))
        await native.push(snapshot({ ...inspected, plan: null, receipts: [receipt()] }))
        await waitFor(() => expect(screen.getByRole('button', { name: 'Roll back…' })).toHaveFocus())
    })

    it('offers Roll back for an unsettled import and blocks a new review meanwhile', async () => {
        const unsettled = receipt({
            state: 'rollingBack',
            interrupted: true,
            canResume: false,
            items: [{ category: 'cases', label: 'Prod', state: 'reverting', written: 2, kept: 0 }],
        })
        const native = scriptedNative(snapshot({ ...inspected, receipts: [unsettled], problem: 'notRecorded' }))
        const user = await open(native)
        expect(screen.getByText(/An import or its rollback did not finish, and nothing of it is shown/)).toBeInTheDocument()
        expect(screen.getByText(/its record could not be saved, so nothing more was written/)).toBeInTheDocument()
        expect(screen.getByText(/Rollback not finished/)).toBeInTheDocument()
        await user.click(screen.getByRole('checkbox', { name: /^Tenants/ }))
        expect(screen.getByRole('button', { name: 'Review what would be imported' })).toBeDisabled()
        expect(screen.queryByRole('button', { name: 'Import again' })).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Roll back…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'rollBackV1Import', receipt: RECEIPT }))
        expect(screen.queryByRole('button', { name: 'Forget this record' })).toBeNull()
    })

    it('reads an import that did not finish as undone and offers Import again', async () => {
        const undone = receipt({
            state: 'rolledBack',
            interrupted: true,
            canResume: true,
            canRollBack: false,
            items: [{ category: 'cases', label: 'Prod', state: 'reverted', written: 0, kept: 0 }],
        })
        const native = scriptedNative(snapshot({ ...inspected, receipts: [undone] }))
        const user = await open(native)
        expect(screen.getByText(/Did not finish, so it was undone/)).toBeInTheDocument()
        expect(screen.queryByText(/An import or its rollback did not finish/)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Import again' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'resumeV1Import', receipt: RECEIPT }))
        expect(screen.getByRole('button', { name: 'Forget this record' })).toBeInTheDocument()
    })

    it('says a record lost in both copies holds what imports write, and releases it only when asked', async () => {
        const native = scriptedNative(snapshot({ damagedReceipts: [RECEIPT] }))
        const user = await open(native)
        const alert = screen.getByText(/An import record is damaged in both of its copies/).closest('[role="alert"]')
        expect(alert).not.toBeNull()
        expect(alert).toHaveTextContent(/is held and can't be changed/)
        expect(alert).toHaveTextContent(/can no longer be rolled back/)
        expect(native.dispatched).toEqual([])
        await user.click(screen.getByRole('button', { name: `Remove damaged record ${RECEIPT}` }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'forgetV1ImportReceipt', receipt: RECEIPT }))
        // Released: the alert and its button go, and focus moves to the imports.
        await native.push(snapshot({ damagedReceipts: [] }))
        await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Imports' })).toHaveFocus())
        expect(screen.queryByText(/damaged in both of its copies/)).toBeNull()
    })

    it('says when the import records cannot be read at all', async () => {
        const native = scriptedNative(snapshot({ receiptsUnreadable: true }))
        await open(native)
        const alert = screen.getByText(/The import records can't be read/).closest('[role="alert"]')
        expect(alert).toHaveTextContent(/Restore access to the app's data folder, then reopen the app/)
        expect(screen.queryByRole('button', { name: /Remove damaged record/ })).toBeNull()
    })
})
