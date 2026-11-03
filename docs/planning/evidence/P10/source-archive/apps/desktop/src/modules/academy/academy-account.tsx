import { useId, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, CheckCircle2, Info } from 'lucide-react'
import type { AcademyFailure, AcademyView, Intent, VaultError } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

const FAILURE_TEXT: Record<AcademyFailure, string> = {
    notConfigured: 'Academy desktop sign-in is not available on this server.',
    invalidConfig: 'Academy returned a sign-in configuration the desktop cannot trust.',
    network: 'Academy could not be reached. Check your connection and try again.',
    invalidResponse: 'Academy returned an unexpected sign-in response. Try again.',
    denied: 'Sign-in was declined in the browser.',
    expired: 'The sign-in code expired before it was approved. Start again.',
    sessionExpired: 'Your Academy session expired. Sign in again.',
    scopeMissing: 'Academy did not grant learning access to this sign-in.',
    accountChanged: 'Academy changed its sign-in configuration. Sign in again.',
    authFailed: 'Academy sign-in could not complete. Try again.',
}

const VAULT_TEXT: Record<VaultError, string> = {
    locked: 'The Keychain is locked, so the session could not be saved. Unlock it and sign in again.',
    denied: 'macOS did not allow the Academy session to be saved in the Keychain.',
    unavailable: 'The Keychain is not available, so the session could not be saved.',
    notFound: 'The saved Academy session is missing from the Keychain.',
    corrupt: 'The saved Academy session could not be read.',
}

interface Props {
    view: AcademyView
    dispatch: Dispatch
}

/**
 * The Academy learner account, independent of AIRS tenants. Sign-in is a
 * browser device flow: the desktop shows a code, opens the Academy page the
 * issuer named, and never sees a password. Tokens stay in the native core.
 */
export function AcademyAccount({ view, dispatch }: Props) {
    const legend = useId()
    const [busy, setBusy] = useState(false)
    const [message, setMessage] = useState<string | null>(null)

    async function send(intent: Intent) {
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch(intent)
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    return (
        <fieldset
            aria-labelledby={legend}
            aria-busy={busy}
            data-slot='academy-account'
            className='border-academy-rule flex flex-col gap-3 rounded-[12px] border p-5'
        >
            <legend id={legend} className='text-academy-ink px-1 text-[15px] font-semibold'>
                Academy account
            </legend>
            <p className='text-academy-ink-soft text-[13px]'>
                Your Academy learner account is separate from AIRS tenants; switching tenants does not change it.
            </p>
            <div role='status' aria-live='polite' className='flex flex-col gap-3'>
                <State view={view} busy={busy} send={send} />
            </div>
            {message ? (
                <p role='alert' data-slot='academy-message' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </fieldset>
    )
}

function State({ view, busy, send }: { view: AcademyView; busy: boolean; send: (intent: Intent) => Promise<void> }) {
    switch (view.state) {
        case 'signedOut':
            return (
                <div className='flex flex-col items-start gap-3'>
                    <p className='text-academy-ink text-[13px]'>Not signed in to Academy.</p>
                    <Button type='button' tier='primary' disabled={busy} onClick={() => void send({ kind: 'startAcademySignIn' })}>
                        Sign in to Academy
                    </Button>
                </div>
            )
        case 'starting':
        case 'completing':
            return (
                <div className='flex flex-col items-start gap-3'>
                    <p className='text-academy-ink text-[13px]'>
                        {view.state === 'starting' ? 'Contacting Academy…' : 'Approved. Finishing sign-in…'}
                    </p>
                    <Button type='button' tier='secondary' onClick={() => void send({ kind: 'cancelAcademySignIn' })}>
                        Cancel sign-in
                    </Button>
                </div>
            )
        case 'awaitingApproval':
            return (
                <div className='flex flex-col items-start gap-3' data-slot='academy-code'>
                    <p className='text-academy-ink text-[13px]'>Enter this code on the Academy sign-in page:</p>
                    <p
                        className='font-mono text-academy-ink text-[24px] font-semibold tracking-[0.12em] select-all'
                        aria-label={`Sign-in code ${view.userCode.split('').join(' ')}`}
                    >
                        {view.userCode}
                    </p>
                    <p className='text-academy-ink-faint text-[12px] break-all'>
                        {view.verificationUri} · expires in {Math.max(1, Math.ceil(view.expiresIn / 60))} min
                    </p>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='button' tier='primary' onClick={() => void send({ kind: 'openAcademyVerification' })}>
                            Open sign-in page
                        </Button>
                        <Button type='button' tier='secondary' onClick={() => void send({ kind: 'cancelAcademySignIn' })}>
                            Cancel sign-in
                        </Button>
                    </div>
                </div>
            )
        case 'signedIn':
            return (
                <div className='flex flex-col items-start gap-3'>
                    <Callout tone='success' icon={CheckCircle2}>
                        <p>
                            Signed in to Academy as <span className='font-semibold'>{view.displayName}</span>.
                        </p>
                    </Callout>
                    <Button type='button' tier='secondary' disabled={busy} onClick={() => void send({ kind: 'signOutAcademy' })}>
                        Sign out of Academy
                    </Button>
                </div>
            )
        case 'failed':
            return (
                <div className='flex flex-col items-start gap-3'>
                    <Callout tone='warning' icon={view.failure === 'network' ? Info : AlertTriangle} data-slot='academy-failure'>
                        {view.displayName ? <p className='font-semibold'>{view.displayName}</p> : null}
                        <p>{view.vault ? VAULT_TEXT[view.vault] : FAILURE_TEXT[view.failure]}</p>
                    </Callout>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='button' tier='primary' disabled={busy} onClick={() => void send({ kind: 'startAcademySignIn' })}>
                            Sign in again
                        </Button>
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void send({ kind: 'signOutAcademy' })}>
                            Sign out of Academy
                        </Button>
                    </div>
                </div>
            )
    }
}
