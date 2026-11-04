/**
 * Reset and storage-durability presentation through the real App and shell:
 * the two-step reset, stage-specific failure text that never claims retired
 * keys are still saved, and the unconfirmed-durability notice.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant, framed } from '../../test/native'

async function openSettings(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    // C44: one presentation; the core names the destination in the snapshot.
    await native.push({ ...native.current, onboarding: 'done', activeFeature: 'settings' })
    return user
}

const resetSection = () => screen.getByRole('region', { name: 'Reset local data' })

describe('reset local data', () => {
    it('asks for confirmation before dispatching a reset', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        const user = await openSettings(native)
        await user.click(within(resetSection()).getByRole('button', { name: 'Reset local data…' }))
        expect(native.dispatched).toEqual([])
        await user.click(within(resetSection()).getByRole('button', { name: 'Confirm reset' }))
        expect(native.dispatched).toEqual([{ kind: 'resetLocalData' }])
    })

    it('explains a stop while deleting keys without claiming the keys were kept', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        await openSettings(native)
        await native.push({
            ...withTenant(baseSnapshot, tenant()),
            resetFailure: { stage: 'credentials', reason: 'locked' },
        })
        const alert = within(resetSection()).getByRole('alert')
        expect(alert).toHaveTextContent(/Reset stopped while deleting scan keys because secure storage is locked/)
        expect(alert).toHaveTextContent(/removed from use/)
        expect(alert).not.toHaveTextContent(/data was kept/i)
        expect(screen.getByText(/No scan key\./)).toBeInTheDocument()
    })

    it('explains a stop while removing the document after the keys were deleted', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        await openSettings(native)
        await native.push({
            ...withTenant(baseSnapshot, tenant()),
            resetFailure: { stage: 'document', reason: 'unavailable' },
        })
        expect(within(resetSection()).getByRole('alert')).toHaveTextContent(
            /deleted the scan keys but could not remove the local desktop data/
        )
    })

    it('says while resetting that keys are out of use', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        await openSettings(native)
        await native.push({ ...baseSnapshot, storage: 'resetting', tenants: [tenant()], active: null })
        expect(screen.getByText(/Scan keys are out of use until it finishes/)).toBeInTheDocument()
        expect(screen.queryByLabelText('AIRS API key')).toBeNull()
    })

    it('reports a change whose durability was not confirmed', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        await openSettings(native)
        expect(document.querySelector('[data-slot="durability-notice"]')).toBeNull()
        await native.push({ ...withTenant(baseSnapshot, tenant()), durabilityUnconfirmed: true })
        expect(document.querySelector('[data-slot="durability-notice"]')).toHaveTextContent(
            /saved, but the system did not confirm it was written to disk/
        )
    })
})
