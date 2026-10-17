/**
 * Production Academy account controls through the real App, DesktopClient
 * and shell: device sign-in states, the issuer-named page, sign-out and
 * honest failures. No URL or credential crosses from the presentation.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { type AcademyView, DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative } from '../../test/native'

const withAcademy = (academy: AcademyView): ShellSnapshot => ({ ...baseSnapshot, academy })

async function openSettings(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    const nav = await screen.findByRole('navigation', { name: 'Features' })
    await user.click(within(nav).getByRole('button', { name: /Settings/ }))
    return user
}

const account = () => within(screen.getByRole('group', { name: 'Academy account' }))

describe('Academy account', () => {
    it('starts device sign-in, shows the code, opens only the native page and signs out', async () => {
        const native = scriptedNative(withAcademy({ state: 'signedOut' }))
        const user = await openSettings(native)
        expect(account().getByText(/separate from AIRS tenants/)).toBeInTheDocument()
        await user.click(account().getByRole('button', { name: 'Sign in to Academy' }))
        expect(native.dispatched).toEqual([{ kind: 'startAcademySignIn' }])
        await native.push(withAcademy({ state: 'starting' }))
        expect(account().getByText('Contacting Academy…')).toBeInTheDocument()
        await native.push(
            withAcademy({
                state: 'awaitingApproval',
                userCode: 'WDJB-MJHT',
                verificationUri: 'https://clerk.academy.example/activate',
                expiresIn: 599,
            })
        )
        expect(account().getByLabelText('Sign-in code W D J B - M J H T')).toHaveTextContent('WDJB-MJHT')
        expect(account().getByText(/https:\/\/clerk.academy.example\/activate · expires in 10 min/)).toBeInTheDocument()
        await user.click(account().getByRole('button', { name: 'Open sign-in page' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'openAcademyVerification' })
        await native.push(withAcademy({ state: 'signedIn', displayName: 'Ada Lovelace', learner: 'lx-aaaaaaaaaaaaaaaaaaaaaaaa' }))
        expect(account().getByText('Ada Lovelace')).toBeInTheDocument()
        await user.click(account().getByRole('button', { name: 'Sign out of Academy' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'signOutAcademy' })
        // Nothing the presentation sent names a URL or carries a token.
        expect(native.sent.some((body) => body.includes('https://'))).toBe(false)
    })

    it('cancels a sign-in in progress', async () => {
        const native = scriptedNative(
            withAcademy({ state: 'awaitingApproval', userCode: 'ABCD-EFGH', verificationUri: 'https://a.example/x', expiresIn: 30 })
        )
        const user = await openSettings(native)
        await user.click(account().getByRole('button', { name: 'Cancel sign-in' }))
        expect(native.dispatched).toEqual([{ kind: 'cancelAcademySignIn' }])
    })

    it.each([
        [{ state: 'failed', failure: 'denied', vault: null, displayName: null }, 'Sign-in was declined in the browser.'],
        [{ state: 'failed', failure: 'expired', vault: null, displayName: null }, 'The sign-in code expired before it was approved.'],
        [{ state: 'failed', failure: 'authFailed', vault: 'locked', displayName: null }, 'The Keychain is locked'],
        [{ state: 'failed', failure: 'sessionExpired', vault: null, displayName: 'Ada Lovelace' }, 'Your Academy session expired.'],
        [{ state: 'failed', failure: 'notConfigured', vault: null, displayName: null }, 'not available on this server'],
    ] as [AcademyView, string][])('explains %o with an explicit way forward', async (view, text) => {
        const native = scriptedNative(withAcademy(view))
        const user = await openSettings(native)
        expect(account().getByText(new RegExp(text))).toBeInTheDocument()
        await user.click(account().getByRole('button', { name: 'Sign in again' }))
        expect(native.dispatched).toEqual([{ kind: 'startAcademySignIn' }])
    })

    it('reports a refusal without claiming a state change', async () => {
        const native = scriptedNative(withAcademy({ state: 'signedOut' }))
        native.respond = () => ({ outcome: 'refusal', refusal: 'deniedStore' })
        const user = await openSettings(native)
        await user.click(account().getByRole('button', { name: 'Sign in to Academy' }))
        expect(await account().findByRole('alert')).toHaveTextContent('Local desktop data or the credential store is not available.')
        expect(account().getByText('Not signed in to Academy.')).toBeInTheDocument()
    })
})
