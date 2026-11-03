import { type FormEvent, useId, useState } from 'react'
import type { ContextRef, CredentialSource, HarnessSignInView, SelectedWorkspace } from '@prisma-airs/desktop-contracts'
import type { Dispatch, DispatchOutcome } from '../../../features/tenants/dispatch'
import { SIGN_IN_FAILURE_TEXT, SIGN_IN_NEEDS, SIGN_IN_STEP_TEXT, actionOutcomeText, notConfirmedText, saveFailureText } from '../text'
import { Btn, Field } from './controls'

interface Props {
    context: ContextRef
    /** The chosen Gateway workspace; company sign-in is set up for one. */
    selected: SelectedWorkspace | null
    signIn: HarnessSignInView | null
    /** The workspace key is stored for the chosen workspace. */
    keyHere: boolean
    /** A conversation is running (changing the credential ends it). */
    running: boolean
    dispatch: Dispatch
    onMessage: (message: string | null) => void
}

/**
 * Which credential the Harness answers with (C20), as a v2 panel (C38): the
 * workspace inference key, or a company sign-in through the browser whose
 * Gateway token is renewed before it is sent.
 */
export function SignInPanel({ selected, ...rest }: Props) {
    return selected ? <WorkspaceSignIn selected={selected} {...rest} /> : <SignInUnavailable />
}

const faint = { color: 'var(--v2-faint)' }

/** With no workspace chosen, company sign-in is shown but cannot be set up yet. */
function SignInUnavailable() {
    const ids = useId()
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-sign-in-setup'>
            <h2 id={`${ids}-heading`} className='text-[13px] font-bold'>
                Company sign-in (SSO)
            </h2>
            <p className='text-[12px]' style={faint}>{`Answer with your company’s identity provider instead of a workspace inference key. ${SIGN_IN_NEEDS}`}</p>
            <Btn className='self-start' disabled aria-describedby={`${ids}-reason`}>
                Set up company sign-in
            </Btn>
            <p id={`${ids}-reason`} className='text-[12px]' style={faint} data-slot='harness-sign-in-reason'>
                Choose a Gateway workspace first
            </p>
        </section>
    )
}

const CHECK = 'Check the sign-in state shown here before trying again.'

function WorkspaceSignIn({ context, selected, signIn, keyHere, running, dispatch, onMessage }: Props & { selected: SelectedWorkspace }) {
    const ids = useId()
    const here = signIn !== null && signIn.workspace.id === selected.workspace.id
    const [editing, setEditing] = useState(false)
    const [busy, setBusy] = useState(false)
    const source: CredentialSource = here && signIn.selected ? 'companySignIn' : 'workspaceKey'

    // An unknown delivery stays Not confirmed naming the action (a sign-out or a credential choice may have
    // happened; the sign-in state shown here is the state to reconcile with); a refusal says its own reason.
    async function send(intent: Parameters<Dispatch>[0], action: string, failureText?: (outcome: DispatchOutcome) => string): Promise<boolean> {
        onMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind === 'unknown') onMessage(notConfirmedText(action, CHECK))
            else if (outcome.kind !== 'admitted') onMessage(failureText ? failureText(outcome) : actionOutcomeText(outcome, action, CHECK))
            return outcome.kind === 'admitted'
        } finally {
            setBusy(false)
        }
    }

    async function configure(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const first = !here
        const form = new FormData(event.currentTarget)
        const value = (name: string) => String(form.get(name) ?? '').trim()
        const saved = await send(
            {
                kind: 'configureHarnessSignIn',
                context,
                workspace: selected.reference,
                endpoint: value('endpoint'),
                issuer: value('issuer'),
                clientId: value('client'),
                audience: value('audience'),
            },
            'saving the company sign-in',
            saveFailureText
        )
        if (!saved) return
        setEditing(false)
        // Newly set up, company sign-in becomes the choice for new
        // conversations; signing in is still the operator's step. A running
        // conversation is left alone (changing the credential ends it).
        if (first && !running) await send({ kind: 'chooseHarnessCredential', context, source: 'companySignIn' }, 'choosing the company sign-in')
    }

    const signedIn = here && signIn.account !== null
    const step = here ? signIn.step : null
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-credential'>
            <h2 id={`${ids}-heading`} className='text-[13px] font-bold'>
                Gateway credential
            </h2>
            {here ? (
                <fieldset className='flex flex-col gap-1' disabled={busy}>
                    <legend className='text-[13px] font-medium'>Answer with</legend>
                    {(
                        [
                            ['workspaceKey', 'The workspace inference key', keyHere ? 'Stored in AI Gateway' : 'Not stored for this workspace'],
                            ['companySignIn', 'Company sign-in', signedIn ? `Signed in as ${signIn.account}` : 'Not signed in'],
                        ] as const
                    ).map(([value, label, hint]) => (
                        <label key={value} className='flex min-h-9 cursor-pointer items-center gap-3'>
                            <input
                                type='radio'
                                name={`${ids}-source`}
                                value={value}
                                checked={source === value}
                                onChange={() => void send({ kind: 'chooseHarnessCredential', context, source: value }, 'choosing the credential')}
                                className='size-4 min-h-0'
                            />
                            <span className='flex flex-col'>
                                <span className='text-[13px]'>{label}</span>
                                <span className='text-[12px]' style={faint}>
                                    {hint}
                                </span>
                            </span>
                        </label>
                    ))}
                    {running ? (
                        <p className='text-[12px]' style={faint}>
                            Changing the credential ends the running conversation.
                        </p>
                    ) : null}
                </fieldset>
            ) : null}
            <h3 className='text-[13px] font-semibold'>Company sign-in (SSO)</h3>
            {!here && !editing ? (
                <>
                    <p className='text-[12px]' style={faint}>
                        {SIGN_IN_NEEDS}
                    </p>
                    <Btn className='self-start' onClick={() => setEditing(true)}>
                        Set up company sign-in
                    </Btn>
                </>
            ) : here && !editing ? (
                <div className='flex flex-col gap-2' data-slot='harness-sign-in'>
                    <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]'>
                        <dt style={faint}>Identity provider</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.issuer}</dd>
                        <dt style={faint}>Client ID</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.clientId}</dd>
                        <dt style={faint}>Gateway audience</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.audience}</dd>
                        <dt style={faint}>Gateway endpoint</dt>
                        <dd className='font-mono text-[12px] break-all'>{signIn.endpoint}</dd>
                    </dl>
                    {step ? (
                        <p role='status' className='text-[13px]' data-slot='harness-sign-in-step'>
                            {SIGN_IN_STEP_TEXT[step]}
                        </p>
                    ) : null}
                    {signIn.failure ? (
                        <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }} data-slot='harness-sign-in-failure'>
                            {SIGN_IN_FAILURE_TEXT[signIn.failure]}
                        </p>
                    ) : null}
                    <div className='flex flex-wrap gap-2'>
                        {step ? (
                            <Btn onClick={() => void send({ kind: 'cancelHarnessSignIn', context }, 'cancelling the sign-in')}>Cancel sign-in</Btn>
                        ) : (
                            <Btn tier='primary' disabled={busy} onClick={() => void send({ kind: 'signInHarness', context }, 'starting the sign-in')}>
                                {signedIn ? 'Sign in again' : 'Sign in with your browser'}
                            </Btn>
                        )}
                        {signedIn ? (
                            <Btn disabled={busy} onClick={() => void send({ kind: 'signOutHarness', context }, 'signing out')}>
                                Sign out
                            </Btn>
                        ) : (
                            <Btn tier='tertiary' disabled={busy || step !== null} onClick={() => setEditing(true)}>
                                Change identity provider
                            </Btn>
                        )}
                        <Btn tier='tertiary' disabled={busy} onClick={() => void send({ kind: 'removeHarnessSignIn', context }, 'removing the company sign-in')}>
                            Remove company sign-in
                        </Btn>
                    </div>
                </div>
            ) : (
                <form className='flex flex-col gap-2' onSubmit={(event) => void configure(event)} data-slot='harness-sign-in-form'>
                    <p className='text-[12px]' style={faint}>
                        {`Use your company’s identity provider for ${selected.workspace.name} instead of the workspace key. The Gateway token is a signed JWT verified against the provider’s keys, kept in memory, and renewed before it expires; only the refresh token is kept, in the Keychain.`}
                    </p>
                    <p className='text-[12px]' style={faint}>
                        The identity provider must allow loopback redirects to http://127.0.0.1 on any port (path /callback).
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
                            <span className='font-medium'>{label}</span>
                            <Field name={name} defaultValue={value} placeholder={placeholder} autoCapitalize='off' autoCorrect='off' spellCheck={false} required maxLength={2048} />
                        </label>
                    ))}
                    <div className='flex flex-wrap gap-2'>
                        <Btn type='submit' disabled={busy}>
                            Save company sign-in
                        </Btn>
                        <Btn tier='tertiary' onClick={() => setEditing(false)}>
                            Cancel
                        </Btn>
                    </div>
                </form>
            )}
        </section>
    )
}
