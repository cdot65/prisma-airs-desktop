/**
 * Production management-client onboarding through the real App,
 * DesktopClient and shell: a separate credential from the scan key, the
 * secret captured at submission and cleared before awaiting, verification
 * as one management read, TSG requirement and removal.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient, type TenantView } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const CLIENT_ID = 'svc@1234567890.iam.panserviceaccount.com'
const SECRET = 'synthetic-management-secret-0042'
const readyManagement: TenantView['management'] = {
    state: 'ready',
    mode: 'keychain',
    failure: null,
    clientId: CLIENT_ID,
    tokenOrigin: 'https://auth.apps.paloaltonetworks.com',
}

async function openSettings(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    const nav = await screen.findByRole('navigation', { name: 'Features' })
    await user.click(within(nav).getByRole('button', { name: /Settings/ }))
    return user
}

const section = () => document.querySelector<HTMLElement>('[data-slot="management-credential"]') as HTMLElement
const secretField = () => within(section()).getByLabelText('Client secret') as HTMLInputElement
const clientField = () => within(section()).getByLabelText('Client ID') as HTMLInputElement

let setItem: ReturnType<typeof vi.spyOn>
beforeEach(() => {
    setItem = vi.spyOn(Storage.prototype, 'setItem')
})
afterEach(() => setItem.mockRestore())

describe('management client onboarding', () => {
    it('saves a separate client, clearing the secret before awaiting and keeping it out of storage', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'keychain', failure: null } }), CONTEXT))
        let admit: (value: unknown) => void = () => undefined
        native.respond = () => new Promise((resolve) => (admit = resolve))
        const user = await openSettings(native)
        expect(within(section()).getByRole('status')).toHaveTextContent('Scans do not need one')
        await user.type(clientField(), CLIENT_ID)
        await user.type(secretField(), SECRET)
        await user.click(within(section()).getByRole('radio', { name: /For this session only/ }))
        await user.click(within(section()).getByRole('button', { name: 'Save client' }))
        expect(secretField().value).toBe('')
        expect(native.dispatched).toEqual([
            { kind: 'configureManagementCredential', context: CONTEXT, mode: 'session', clientId: CLIENT_ID, secret: SECRET },
        ])
        admit({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        // The scan key is never part of a management intent.
        expect(native.dispatched.some((i) => i.kind === 'configureScanCredential')).toBe(false)
        for (const call of setItem.mock.calls) expect(JSON.stringify(call)).not.toContain(SECRET)
        expect(native.delivered.join('\n')).not.toContain(SECRET)
    })

    it('requires a client ID and a secret before dispatching', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        const user = await openSettings(native)
        await user.click(within(section()).getByRole('button', { name: 'Save client' }))
        expect(within(section()).getByRole('alert')).toHaveTextContent('Enter the client ID.')
        await user.type(clientField(), CLIENT_ID)
        await user.click(within(section()).getByRole('button', { name: 'Save client' }))
        expect(within(section()).getByRole('alert')).toHaveTextContent('Enter the client secret.')
        expect(native.dispatched).toEqual([])
    })

    it('needs a TSG ID on the tenant profile', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant({ tsgId: null }), CONTEXT))
        await openSettings(native)
        expect(screen.getByText(/has no TSG ID/)).toBeInTheDocument()
        expect(within(section()).getByRole('button', { name: 'Save client' })).toBeDisabled()
    })

    it('points to the Harness for company sign-in (SSO)', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        await openSettings(native)
        expect(within(section()).getByText('Company sign-in (SSO) for the Harness is set up per Gateway workspace, in Harness.')).toBeInTheDocument()
    })

    it('verifies with one management read and reports failures faithfully', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant({ management: readyManagement }), CONTEXT))
        let answer: unknown = { kind: 'management', result: { kind: 'verified' } }
        native.respond = () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: 'op-7' } })
        const observed: string[] = []
        native.observeManagement = (operation) => {
            observed.push(operation)
            return answer
        }
        const user = await openSettings(native)
        expect(within(section()).getByRole('status')).toHaveTextContent('saved in the Keychain')
        expect(within(section()).getByText(`Client ID: ${CLIENT_ID}`)).toBeInTheDocument()
        await user.click(within(section()).getByRole('button', { name: 'Verify' }))
        expect(await within(section()).findByText('The management client can read this tenant.')).toBeInTheDocument()
        expect(native.dispatched).toEqual([{ kind: 'management', context: CONTEXT, request: { kind: 'verify' } }])
        expect(observed).toEqual(['op-7'])
        answer = { kind: 'management', result: { kind: 'failed', failure: 'unauthorized', outcome: 'rejected' } }
        await user.click(within(section()).getByRole('button', { name: 'Verify' }))
        expect(await within(section()).findByRole('alert')).toHaveTextContent('was not accepted')
        expect(within(section()).queryByText('The management client can read this tenant.')).toBeNull()
    })

    it('removes only the management client', async () => {
        const native = scriptedNative(
            withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'keychain', failure: null }, management: readyManagement }), CONTEXT)
        )
        const user = await openSettings(native)
        await user.click(within(section()).getByRole('button', { name: 'Remove management client' }))
        expect(native.dispatched).toEqual([{ kind: 'removeManagementCredential', context: CONTEXT }])
    })
})
