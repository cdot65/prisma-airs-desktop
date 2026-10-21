import { type FormEvent, useId, useState } from 'react'
import { Button, Input } from '@ai-security-academy/presentation'
import type {
    ContextRef,
    CredentialSource,
    HarnessSignInView,
    SelectedWorkspace,
    SignInFailure,
    SignInStep,
} from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../tenants/dispatch'

/** Why company sign-in or its renewal failed, as the operator reads it. */
export const SIGN_IN_FAILURE_TEXT: Record<SignInFailure, string> = {
    invalidConfig:
        'The identity provider’s configuration could not be used: it must name this issuer, support PKCE with S256 and keep its endpoints on the issuer’s own address.',
    network: 'The identity provider could not be reached. Try again.',
    invalidResponse: 'The identity provider returned something this app does not accept.',
    denied: 'Sign-in was declined.',
    timedOut: 'The browser did not come back within five minutes.',
    cancelled: 'Sign-in was cancelled.',
    browserUnavailable: 'The browser could not be opened.',
    callbackUnavailable: 'The sign-in callback could not be opened on this Mac.',
    invalidToken: 'A token from the identity provider did not verify (signature, issuer, audience, client or time).',
    opaqueToken: 'The identity provider’s Gateway token is not a signed JWT, which this app requires.',
    wrongWorkspace: 'The token is for another Gateway workspace.',
    differentAccount: 'A different account signed in. Sign out first to use another account.',
    sessionExpired: 'Your sign-in has expired. Sign in again to send.',
    storeFailed: 'The sign-in could not be kept in the Keychain.',
}

const STEP_TEXT: Record<SignInStep, string> = {
    contacting: 'Contacting the identity provider…',
    browser: 'Continue in your browser. This app is waiting for it to come back.',
    completing: 'Verifying the sign-in…',
}

interface Props {
    context: ContextRef
    selected: SelectedWorkspace
    signIn: HarnessSignInView | null
    /** The workspace key is stored for the chosen workspace. */
    keyHere: boolean
    /** A conversation is running (changing the credential ends it). */
    running: boolean
    dispatch: Dispatch
    onMessage: (message: string | null) => void
}

/**
 * Which credential the Harness answers with (C20): the workspace inference
 * key, or a company sign-in through the browser whose Gateway token is
 * renewed before it is sent.
 */
export function SignInPanel({ context, selected, signIn, keyHere, running, dispatch, onMessage }: Props) {
    const ids = useId()
    const here = signIn !== null && signIn.workspace.id === selected.workspace.id
    const [editing, setEditing] = useState(false)
    const [busy, setBusy] = useState(false)
    const source: CredentialSource = here && signIn.selected ? 'companySignIn' : 'workspaceKey'

    async function send(intent: Parameters<Dispatch>[0]): Promise<boolean> {
        onMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') onMessage('That was not done. Check the values and try again.')
            return outcome.kind === 'admitted'
        } finally {
            setBusy(false)
        }
    }

    async function configure(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const form = new FormData(event.currentTarget)
        const value = (name: string) => String(form.get(name) ?? '').trim()
        const saved = await send({
            kind: 'configureHarnessSignIn',
            context,
            workspace: selected.reference,
            endpoint: value('endpoint'),
            issuer: value('issuer'),
            clientId: value('client'),
            audience: value('audience'),
        })
        if (saved) setEditing(false)
    }

    const signedIn = here && signIn.account !== null
    const step = here ? signIn.step : null
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-credential'>
            <h2 id={`${ids}-heading`} className='text-[15px] font-semibold'>
                Gateway credential
            </h2>
            {here ? (
                <fieldset className='flex flex-col gap-1' disabled={busy}>
                    <legend className='text-academy-ink text-[13px] font-medium'>Answer with</legend>
                    {(
                        [
                            ['workspaceKey', 'The workspace inference key', keyHere ? 'Stored in AI Gateway' : 'Not stored for this workspace'],
                            ['companySignIn', 'Company sign-in', signedIn ? `Signed in as ${signIn.account}` : 'Not signed in'],
                        ] as const
                    ).map(([value, label, hint]) => (
                        <label key={value} className='flex min-h-11 cursor-pointer items-center gap-3'>
                            <input
                                type='radio'
                                name={`${ids}-source`}
                                value={value}
                                checked={source === value}
                                onChange={() => void send({ kind: 'chooseHarnessCredential', context, source: value })}
                                className='size-4 accent-[var(--academy-accent)]'
                            />
                            <span className='flex flex-col'>
                                <span className='text-academy-ink text-[13px]'>{label}</span>
                                <span className='text-academy-ink-faint text-[12px]'>{hint}</span>
                            </span>
                        </label>
                    ))}
                    {running ? <p className='text-academy-ink-faint text-[12px]'>Changing the credential ends the running conversation.</p> : null}
                </fieldset>
            ) : null}
            {!here && !editing ? (
                <Button type='button' tier='tertiary' size='sm' className='self-start' onClick={() => setEditing(true)}>
                    Set up company sign-in
                </Button>
            ) : here && !editing ? (
                <div className='flex flex-col gap-2' data-slot='harness-sign-in'>
                    <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]'>
                        <dt className='text-academy-ink-faint'>Identity provider</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.issuer}</dd>
                        <dt className='text-academy-ink-faint'>Client ID</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.clientId}</dd>
                        <dt className='text-academy-ink-faint'>Gateway audience</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.audience}</dd>
                        <dt className='text-academy-ink-faint'>Gateway endpoint</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.endpoint}</dd>
                    </dl>
                    {step ? (
                        <p role='status' className='text-[13px]' data-slot='harness-sign-in-step'>
                            {STEP_TEXT[step]}
                        </p>
                    ) : null}
                    {signIn.failure ? (
                        <p role='alert' className='text-academy-accent-warn text-[13px]' data-slot='harness-sign-in-failure'>
                            {SIGN_IN_FAILURE_TEXT[signIn.failure]}
                        </p>
                    ) : null}
                    <div className='flex flex-wrap gap-2'>
                        {step ? (
                            <Button type='button' tier='secondary' size='sm' onClick={() => void send({ kind: 'cancelHarnessSignIn', context })}>
                                Cancel sign-in
                            </Button>
                        ) : (
                            <Button type='button' tier='primary' size='sm' disabled={busy} onClick={() => void send({ kind: 'signInHarness', context })}>
                                {signedIn ? 'Sign in again' : 'Sign in with your browser'}
                            </Button>
                        )}
                        {signedIn ? (
                            <Button type='button' tier='secondary' size='sm' disabled={busy} onClick={() => void send({ kind: 'signOutHarness', context })}>
                                Sign out
                            </Button>
                        ) : (
                            <Button type='button' tier='tertiary' size='sm' disabled={busy || step !== null} onClick={() => setEditing(true)}>
                                Change identity provider
                            </Button>
                        )}
                        <Button
                            type='button'
                            tier='tertiary'
                            size='sm'
                            disabled={busy}
                            onClick={() => void send({ kind: 'removeHarnessSignIn', context })}
                        >
                            Remove company sign-in
                        </Button>
                    </div>
                </div>
            ) : (
                <form className='flex flex-col gap-2' onSubmit={(event) => void configure(event)} data-slot='harness-sign-in-form'>
                    <p className='text-academy-ink-faint text-[12px]'>
                        {`Use your company’s identity provider for ${selected.workspace.name} instead of the workspace key. The Gateway token is a signed JWT verified against the provider’s keys, kept in memory, and renewed before it expires; only the refresh token is kept, in the Keychain.`}
                    </p>
                    {(
                        [
                            ['endpoint', 'Gateway endpoint', here ? signIn.endpoint : '', 'https://'],
                            ['issuer', 'Identity provider (issuer URL)', here ? signIn.issuer : '', 'https://'],
                            ['client', 'Client ID', here ? signIn.clientId : '', ''],
                            ['audience', 'Gateway audience', here ? signIn.audience : '', ''],
                        ] as const
                    ).map(([name, label, value, placeholder]) => (
                        <label key={name} className='flex flex-col gap-1 text-[13px]'>
                            <span className='text-academy-ink font-medium'>{label}</span>
                            <Input
                                name={name}
                                defaultValue={value}
                                placeholder={placeholder}
                                autoCapitalize='off'
                                autoCorrect='off'
                                spellCheck={false}
                                required
                                maxLength={2048}
                            />
                        </label>
                    ))}
                    <div className='flex flex-wrap gap-2'>
                        <Button type='submit' tier='secondary' size='sm' disabled={busy}>
                            Save company sign-in
                        </Button>
                        <Button type='button' tier='tertiary' size='sm' onClick={() => setEditing(false)}>
                            Cancel
                        </Button>
                    </div>
                </form>
            )}
        </section>
    )
}
