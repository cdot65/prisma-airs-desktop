import { type FormEvent, useId, useRef, useState } from 'react'
import { Button, Callout, Input } from '@ai-security-academy/presentation'
import { AlertTriangle, CheckCircle2, Info } from 'lucide-react'
import type { ContextRef, StorageMode, TenantView, VaultError } from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, readManagement } from '../administration/management-read'
import { type Dispatch, outcomeMessage } from './dispatch'

const FAILURE_TEXT: Record<VaultError, string> = {
    locked: 'The Keychain is locked. Unlock it and enter the secret again, or keep it for this session only.',
    denied: 'macOS did not allow access to the Keychain. Enter the secret again, or keep it for this session only.',
    unavailable: 'The credential store is not available. Enter the secret again, or keep it for this session only.',
    notFound: 'The stored client secret is missing. Enter it again.',
    corrupt: 'The stored client secret could not be read. Enter it again.',
}

function status(tenant: TenantView): { tone: 'default' | 'success' | 'danger'; text: string } {
    const { state, mode, failure } = tenant.management
    switch (state) {
        case 'missing':
            return {
                tone: 'default',
                text: 'No management client. Scans do not need one; add one to read this tenant’s inventories in Administration.',
            }
        case 'storing':
            return { tone: 'default', text: 'Saving the management client…' }
        case 'ready':
            return {
                tone: 'success',
                text:
                    mode === 'session'
                        ? 'Management client secret held for this session only.'
                        : 'Management client secret saved in the Keychain.',
            }
        case 'failed':
            return { tone: 'danger', text: failure ? FAILURE_TEXT[failure] : 'The management client could not be saved.' }
    }
}

interface Props {
    tenant: TenantView
    context: ContextRef
    dispatch: Dispatch
    observe: ObserveManagement
}

/**
 * The tenant's separate management OAuth client (C08). It never replaces or
 * reads the scan key. The secret is read from its field at submission and
 * the field is cleared before the request is awaited; it is never held in
 * React state, storage or any snapshot. Verify performs one bounded
 * management read with the stored client.
 */
export function ManagementCredentialForm({ tenant, context, dispatch, observe }: Props) {
    const [mode, setMode] = useState<StorageMode>('keychain')
    const [message, setMessage] = useState<string | null>(null)
    const [verified, setVerified] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const secretRef = useRef<HTMLInputElement>(null)
    const clientRef = useRef<HTMLInputElement>(null)
    const ids = useId()
    const shown = status(tenant)
    const view = tenant.management
    const hasClient = view.state === 'ready' || view.state === 'failed'
    const noTsg = tenant.tsgId === null

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const secretField = secretRef.current
        const clientField = clientRef.current
        if (!secretField || !clientField) return
        const secret = secretField.value
        secretField.value = ''
        const clientId = clientField.value.trim()
        setVerified(null)
        if (clientId.length === 0) {
            setMessage('Enter the client ID.')
            clientField.focus()
            return
        }
        if (secret.length === 0) {
            setMessage('Enter the client secret.')
            secretField.focus()
            return
        }
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch({ kind: 'configureManagementCredential', context, mode, clientId, secret })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    async function verify() {
        setBusy(true)
        setMessage(null)
        setVerified(null)
        const outcome = await readManagement(dispatch, observe, context, { kind: 'verify' })
        setBusy(false)
        if (outcome.kind === 'notAdmitted') setMessage(outcome.message)
        else if (outcome.result.kind === 'verified') setVerified('The management client can read this tenant.')
        else if (outcome.result.kind === 'failed') setMessage(MANAGEMENT_FAILURE_TEXT[outcome.result.failure])
        else setMessage('The service returned a reply the desktop could not use.')
    }

    async function remove() {
        setBusy(true)
        setMessage(null)
        setVerified(null)
        const outcome = await dispatch({ kind: 'removeManagementCredential', context })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    return (
        <section aria-labelledby={`${ids}-heading`} data-slot='management-credential' className='flex flex-col gap-3'>
            <h3 id={`${ids}-heading`} className='text-academy-ink text-[14px] font-semibold'>
                Management client
            </h3>
            <Callout
                tone={shown.tone}
                icon={shown.tone === 'success' ? CheckCircle2 : shown.tone === 'danger' ? AlertTriangle : Info}
                role='status'
                aria-live='polite'
                data-slot='management-status'
            >
                {shown.text}
                {view.clientId ? (
                    <span className='text-academy-ink-soft mt-1 block text-[12px]' data-slot='management-client-id'>
                        Client ID: {view.clientId}
                    </span>
                ) : null}
            </Callout>
            {noTsg ? (
                <Callout tone='warning' icon={AlertTriangle} data-slot='management-no-tsg'>
                    This tenant has no TSG ID. Add one to its profile before configuring a management client.
                </Callout>
            ) : null}
            <form onSubmit={(e) => void submit(e)} className='flex flex-col gap-3' aria-busy={busy}>
                <label htmlFor={`${ids}-client`} className='text-academy-ink text-[13px] font-medium'>
                    Client ID
                </label>
                <Input
                    ref={clientRef}
                    id={`${ids}-client`}
                    name='management-client-id'
                    autoComplete='off'
                    autoCapitalize='off'
                    autoCorrect='off'
                    spellCheck={false}
                    defaultValue={view.clientId ?? ''}
                    maxLength={1024}
                    data-slot='management-client-input'
                />
                <label htmlFor={`${ids}-secret`} className='text-academy-ink text-[13px] font-medium'>
                    Client secret
                </label>
                <Input
                    ref={secretRef}
                    id={`${ids}-secret`}
                    name='management-secret'
                    type='password'
                    autoComplete='off'
                    autoCapitalize='off'
                    autoCorrect='off'
                    spellCheck={false}
                    aria-describedby={`${ids}-hint`}
                    data-slot='management-secret-input'
                />
                <p id={`${ids}-hint`} className='text-academy-ink-faint text-[12px]'>
                    The secret is sent only to {view.tokenOrigin ?? 'the Palo Alto Networks token service'} to obtain
                    short-lived access tokens. It is separate from the scan key.
                </p>
                <fieldset aria-labelledby={`${ids}-mode`} className='flex flex-col gap-1'>
                    <legend id={`${ids}-mode`} className='text-academy-ink text-[13px] font-medium'>
                        Keep the secret
                    </legend>
                    {(
                        [
                            ['keychain', 'In the Keychain', 'Available after restarting the desktop'],
                            ['session', 'For this session only', 'Forgotten when the desktop quits'],
                        ] as const
                    ).map(([value, label, hint]) => (
                        <label key={value} className='flex min-h-11 cursor-pointer items-center gap-3'>
                            <input
                                type='radio'
                                name='management-mode'
                                value={value}
                                checked={mode === value}
                                onChange={() => setMode(value)}
                                className='size-4 accent-[var(--academy-accent)]'
                            />
                            <span className='flex flex-col'>
                                <span className='text-academy-ink text-[13px]'>{label}</span>
                                <span className='text-academy-ink-faint text-[12px]'>{hint}</span>
                            </span>
                        </label>
                    ))}
                </fieldset>
                <div className='flex flex-wrap gap-2'>
                    <Button type='submit' tier='primary' disabled={busy || noTsg}>
                        {hasClient ? 'Replace client' : 'Save client'}
                    </Button>
                    {view.state === 'ready' ? (
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void verify()}>
                            Verify
                        </Button>
                    ) : null}
                    {hasClient ? (
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void remove()}>
                            Remove management client
                        </Button>
                    ) : null}
                </div>
                {verified ? (
                    <p role='status' data-slot='management-verified' className='text-academy-ink text-[13px]'>
                        {verified}
                    </p>
                ) : null}
                {message ? (
                    <p role='alert' data-slot='management-message' className='text-academy-accent-warn text-[13px]'>
                        {message}
                    </p>
                ) : null}
            </form>
        </section>
    )
}
