import { useEffect, useId, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info } from 'lucide-react'
import type { CandidateProblem, ImportError, ImportView } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from './dispatch'

const ERROR_TEXT: Record<ImportError, string> = {
    tooLarge: 'That file is too large to import.',
    unsupported: 'That file is not a supported tenant-profile format.',
    empty: 'That file contains no tenant profiles.',
}

const PROBLEM_TEXT: Record<CandidateProblem, string> = {
    invalidLabel: 'missing or invalid label',
    invalidTsgId: 'invalid TSG ID',
    invalidDestination: 'destination is not an HTTPS origin',
}

interface Props {
    view: ImportView
    dispatch: Dispatch
}

/**
 * Reviewed import of nonsecret tenant metadata. The file is chosen in a
 * native sheet and read by the desktop; this view only shows the desktop's
 * whitelisted preview and sends the explicit selection. Imported tenants
 * have no credentials.
 */
export function ProfileImport({ view, dispatch }: Props) {
    const [selected, setSelected] = useState<number[]>([])
    const [message, setMessage] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const headingId = useId()
    const review = view.state === 'review' ? view.review : null

    useEffect(() => {
        // A new preview starts with every importable entry selected.
        if (view.state === 'review') {
            setSelected(view.candidates.filter((c) => c.problem === null).map((c) => c.index))
        }
        // Only a new review (a new reference) resets the selection.
    }, [review])

    async function run(intent: Parameters<Dispatch>[0]) {
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch(intent)
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    return (
        <section aria-labelledby={headingId} data-slot='profile-import' className='flex flex-col gap-3'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Import tenant profiles
            </h3>
            <p className='text-academy-ink-soft text-[13px]'>
                Imports labels, TSG IDs and scan destinations only. Keys, secrets and referenced credential files are
                never read; each imported tenant needs its own scan key.
            </p>
            {view.state !== 'review' ? (
                <div>
                    <Button
                        type='button'
                        tier='secondary'
                        disabled={busy || view.state === 'choosing'}
                        onClick={() => void run({ kind: 'chooseProfileImport' })}
                    >
                        Choose profile file…
                    </Button>
                </div>
            ) : null}
            {view.state === 'choosing' ? (
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    Choose a file in the open dialog.
                </p>
            ) : null}
            {view.state === 'failed' ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    {ERROR_TEXT[view.error]} Nothing was imported.
                </Callout>
            ) : null}
            {view.state === 'review' ? (
                <form
                    data-slot='import-review'
                    aria-busy={busy}
                    className='flex flex-col gap-3'
                    onSubmit={(event) => {
                        event.preventDefault()
                        void run({ kind: 'confirmProfileImport', review: view.review, selected })
                    }}
                >
                    <Callout tone='default' icon={Info}>
                        <p>
                            Preview of <span className='font-mono'>{view.fileName}</span> (
                            {view.format === 'airsCliRegistry' ? 'airs-cli tenant registry' : 'v2 profile document'}).
                        </p>
                        <p data-slot='import-discarded'>
                            Not imported: {view.discarded.secretFields} secret field(s),{' '}
                            {view.discarded.fileReferences} file reference(s), {view.discarded.otherFields} other
                            field(s).
                        </p>
                    </Callout>
                    <fieldset className='flex flex-col gap-1'>
                        <legend className='text-academy-ink text-[13px] font-medium'>Profiles to import</legend>
                        {view.candidates.map((candidate) => {
                            const disabled = candidate.problem !== null
                            const destination = candidate.destination
                                ? candidate.destination.kind === 'custom'
                                    ? candidate.destination.origin
                                    : `${candidate.destination.region} region${candidate.defaultDestination ? ' (default; not in the file)' : ''}`
                                : 'no valid destination'
                            return (
                                <label
                                    key={candidate.index}
                                    className='flex min-h-11 items-start gap-3 py-1'
                                    data-slot='import-candidate'
                                >
                                    <input
                                        type='checkbox'
                                        className='mt-1 size-4 accent-[var(--academy-accent)]'
                                        disabled={disabled}
                                        checked={selected.includes(candidate.index)}
                                        onChange={(e) =>
                                            setSelected((current) =>
                                                e.target.checked
                                                    ? [...current, candidate.index].sort((a, b) => a - b)
                                                    : current.filter((i) => i !== candidate.index)
                                            )
                                        }
                                    />
                                    <span className='flex flex-col'>
                                        <span className='text-academy-ink text-[13px] font-medium'>
                                            {candidate.label || 'Unnamed profile'}
                                        </span>
                                        <span className='text-academy-ink-faint text-[12px]'>
                                            {candidate.tsgId ? `TSG ${candidate.tsgId} · ` : ''}
                                            {destination}
                                            {candidate.existingLabel ? ' · a tenant with this label exists (it is not changed)' : ''}
                                            {candidate.problem ? ` · cannot import: ${PROBLEM_TEXT[candidate.problem]}` : ''}
                                        </span>
                                    </span>
                                </label>
                            )
                        })}
                    </fieldset>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='submit' tier='primary' disabled={busy || selected.length === 0}>
                            Import {selected.length} profile{selected.length === 1 ? '' : 's'}
                        </Button>
                        <Button
                            type='button'
                            tier='tertiary'
                            disabled={busy}
                            onClick={() => void run({ kind: 'discardReview', review: view.review })}
                        >
                            Discard
                        </Button>
                    </div>
                </form>
            ) : null}
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </section>
    )
}
