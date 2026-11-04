import { type FormEvent, useId, useRef, useState } from 'react'
import { Button, Callout, Input } from '@ai-security-academy/presentation'
import { AlertTriangle, CheckCircle2, Info } from 'lucide-react'
import type { ContextRef, StorageMode, TenantView, VaultError } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from './dispatch'

const FAILURE_TEXT: Record<VaultError, string> = {
    locked: 'Secure storage is locked. Unlock it and enter the key again, or keep it for this session only.',
    denied: 'The system did not allow access to secure storage. Enter the key again, or keep it for this session only.',
    unavailable: 'The credential store is not available. Enter the key again, or keep it for this session only.',
    notFound: 'The stored key is missing. Enter it again.',
    corrupt: 'The stored key could not be read. Enter it again.',
}

function scanStatus(tenant: TenantView): { tone: 'default' | 'success' | 'danger'; text: string } {
    const { state, mode, failure } = tenant.scan
    switch (state) {
        case 'missing':
            return { tone: 'default', text: 'No scan key. Enter this tenant’s AIRS API key to enable scans.' }
        case 'storing':
            return { tone: 'default', text: 'Saving the scan key…' }
        case 'ready':
            return {
                tone: 'success',
                text: mode === 'session' ? 'Scan key held for this session only.' : 'Scan key saved in secure storage.',
            }
        case 'failed':
            return { tone: 'danger', text: failure ? FAILURE_TEXT[failure] : 'The scan key could not be saved.' }
    }
}

interface Props {
    tenant: TenantView
    context: ContextRef
    dispatch: Dispatch
}

/**
 * Scan-key entry for the active tenant. The key is read from the field at
 * the moment of submission and the field is cleared before the request is
 * awaited; it is never held in React state, storage or any snapshot. (A
 * JavaScript string cannot be reliably zeroized; the native side keeps it.)
 */
export function ScanKeyForm({ tenant, context, dispatch }: Props) {
    const [mode, setMode] = useState<StorageMode>('keychain')
    const [message, setMessage] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const keyRef = useRef<HTMLInputElement>(null)
    const keyId = useId()
    const hintId = useId()
    const modeLegend = useId()
    const status = scanStatus(tenant)
    const hasKey = tenant.scan.state === 'ready' || tenant.scan.state === 'failed'

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const field = keyRef.current
        if (!field) return
        const key = field.value
        field.value = ''
        if (key.length === 0) {
            setMessage('Enter the scan key.')
            field.focus()
            return
        }
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch({ kind: 'configureScanCredential', context, mode, key })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    async function remove() {
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch({ kind: 'removeScanCredential', context })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    return (
        <section aria-labelledby={`${keyId}-heading`} data-slot='scan-key' className='settings-card'>
            <h3 id={`${keyId}-heading`} className='text-academy-ink text-[14px] font-semibold'>
                Scan key
            </h3>
            <Callout
                tone={status.tone}
                icon={status.tone === 'success' ? CheckCircle2 : status.tone === 'danger' ? AlertTriangle : Info}
                role='status'
                aria-live='polite'
                data-slot='scan-status'
            >
                {status.text}
            </Callout>
            <form onSubmit={(e) => void submit(e)} className='flex flex-col gap-3' aria-busy={busy}>
                <label htmlFor={keyId} className='text-academy-ink text-[13px] font-medium'>
                    AIRS API key
                </label>
                <Input
                    ref={keyRef}
                    id={keyId}
                    name='scan-key'
                    type='password'
                    autoComplete='off'
                    autoCapitalize='off'
                    autoCorrect='off'
                    spellCheck={false}
                    aria-describedby={hintId}
                    data-slot='scan-key-input'
                />
                <p id={hintId} className='text-academy-ink-faint text-[12px]'>
                    Sent only to {tenant.origin}. Entering a new key replaces the current one.
                </p>
                <fieldset aria-labelledby={modeLegend} className='flex flex-col gap-1'>
                    <legend id={modeLegend} className='text-academy-ink text-[13px] font-medium'>
                        Keep the key
                    </legend>
                    {(
                        [
                            ['keychain', 'In secure storage', 'Available after restarting the desktop'],
                            ['session', 'For this session only', 'Forgotten when the desktop quits'],
                        ] as const
                    ).map(([value, label, hint]) => (
                        <label key={value} className='flex min-h-8 cursor-pointer items-center gap-3'>
                            <input
                                type='radio'
                                name='scan-key-mode'
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
                    <Button type='submit' tier='primary' disabled={busy}>
                        {hasKey ? 'Replace key' : 'Save key'}
                    </Button>
                    {hasKey ? (
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void remove()}>
                            Remove scan key
                        </Button>
                    ) : null}
                </div>
                {message ? (
                    <p role='alert' data-slot='scan-key-message' className='text-academy-accent-warn text-[13px]'>
                        {message}
                    </p>
                ) : null}
            </form>
        </section>
    )
}
