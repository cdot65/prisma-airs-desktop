import { useEffect, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Download, ShieldAlert } from 'lucide-react'
import type { RunRecord, RunView, RunningView, SendReviewView } from '@prisma-airs/desktop-contracts'
import { type WorkbenchCall, resultText, runFailureText, runSummary, sizeText } from './workbench-call'

/** A request in flight: its progress and Cancel. */
export function Running({ running, call }: { running: RunningView; call: WorkbenchCall }) {
    return (
        <p role='status' aria-live='polite' className='flex flex-wrap items-center gap-2 text-[13px]' data-slot='workbench-running'>
            {`Sending ${running.method}… ${sizeText(running.bytes)} transferred`}
            <Button type='button' tier='secondary' size='sm' onClick={() => void call({ kind: 'cancel', run: running.run })}>
                Cancel request
            </Button>
        </p>
    )
}

/** A tenant-credential change awaiting the operator (secrets masked). */
export function ReviewPanel({ review, onApply, onDismiss }: { review: SendReviewView; onApply: () => void; onDismiss: () => void }) {
    const expires = new Date(review.expires * 1000)
    return (
        <Callout tone='warning' icon={ShieldAlert} role='alert' data-slot='workbench-review'>
            <p className='text-academy-ink font-semibold'>{`Review this change to the tenant: ${review.operation}`}</p>
            <p className='font-mono break-all'>{`${review.method} ${review.url}`}</p>
            <p>{`Goes to ${review.destination} with the active tenant's credential. It is sent once and never again by itself.`}</p>
            {review.headers.length ? (
                <ul className='font-mono text-[12px]'>
                    {review.headers.map(([n, v], i) => (
                        <li key={i} className='break-all'>{`${n}: ${v}`}</li>
                    ))}
                </ul>
            ) : null}
            {review.body ? <pre className='bg-academy-bg-inset max-h-48 overflow-auto p-2 font-mono text-[12px] whitespace-pre-wrap'>{review.body}</pre> : null}
            {review.files.length ? <p>{`Files sent as they are now: ${review.files.join(', ')}`}</p> : null}
            <p className='text-academy-ink-faint text-[12px]'>{`Secret values are masked. The review expires at ${expires.toLocaleTimeString()}; any change to the request, its files or environment needs a new review.`}</p>
            <div className='mt-3 flex flex-wrap gap-2'>
                <Button type='button' tier='primary' onClick={onApply}>
                    Send to tenant…
                </Button>
                <Button type='button' tier='secondary' onClick={onDismiss}>
                    Dismiss review
                </Button>
            </div>
        </Callout>
    )
}

/** A run's outcome and response. */
export function RunPanel({ run, call }: { run: RunView; call: WorkbenchCall }) {
    const record = run.record
    const [saved, setSaved] = useState('')
    useEffect(() => setSaved(''), [record.id])
    const save = async () => {
        setSaved('')
        const r = await call({ kind: 'saveResponse', run: record.id })
        if (r.kind === 'responseSaved') setSaved(`Saved ${sizeText(r.bytes)} as ${r.name}.`)
        else if (r.kind === 'cancelled') setSaved('')
        else setSaved(resultText(r) ?? '')
    }
    return (
        <section aria-label='Response' data-slot='workbench-response' className='border-academy-rule rounded-academy-md flex flex-col gap-2 border p-4 text-[13px]'>
            <h3 className='text-[14px] font-semibold'>Response</h3>
            <p data-slot='workbench-run-summary'>{runSummary(record)}</p>
            <p className='text-academy-ink-faint font-mono text-[12px] break-all'>{`${record.method} ${record.url}`}</p>
            {record.failure ? <p role='alert'>{runFailureText(record.failure)}</p> : null}
            {record.uncertain ? (
                <Callout tone='warning' icon={AlertTriangle}>
                    The request may have reached the server; no response arrived. It was not sent again. Check the result before sending again.
                </Callout>
            ) : null}
            {record.redirect ? <p>{`Redirect to ${record.redirect} was not followed. Send a request to it to follow it.`}</p> : null}
            {run.tokenDropped ? <p>The server refused the token; it was dropped. Send again to get a new one (nothing was resent).</p> : null}
            {record.headers.length ? (
                <details>
                    <summary>{`Headers (${record.headers.length})`}</summary>
                    <ul className='font-mono text-[12px]'>
                        {record.headers.map(([n, v], i) => (
                            <li key={i} className='break-all'>{`${n}: ${v}`}</li>
                        ))}
                    </ul>
                </details>
            ) : null}
            {run.preview !== null && run.preview !== '' ? (
                <pre aria-label='Response body' className='bg-academy-bg-inset max-h-96 overflow-auto p-2 font-mono text-[12px] whitespace-pre-wrap'>
                    {run.preview}
                </pre>
            ) : null}
            {run.preview === null && record.bytes > 0 ? <p>The body is not text; save it to open it elsewhere.</p> : null}
            {record.bytes > run.previewBytes && run.preview !== null ? <p className='text-academy-ink-faint text-[12px]'>{`Showing the first ${sizeText(run.previewBytes)}.`}</p> : null}
            {record.stored ? (
                <div className='flex flex-wrap items-center gap-2'>
                    <Button type='button' tier='secondary' size='sm' onClick={() => void save()}>
                        <Download aria-hidden className='size-4' />
                        Save response…
                    </Button>
                    <span role='status' aria-live='polite' data-slot='workbench-response-saved'>
                        {saved}
                    </span>
                </div>
            ) : null}
        </section>
    )
}

/** The open collection's history: what was sent, never sent again. */
export function History({ call, revision }: { call: WorkbenchCall; revision: string }) {
    const [runs, setRuns] = useState<RunRecord[]>([])
    const [more, setMore] = useState(false)
    const [shown, setShown] = useState<RunView | null>(null)
    const [message, setMessage] = useState('')
    useEffect(() => {
        void call({ kind: 'history', before: null }).then((r) => {
            if (r.kind === 'history') {
                setRuns(r.runs)
                setMore(r.more)
            } else setMessage(resultText(r) ?? '')
        })
    }, [call, revision])
    const older = async () => {
        const last = runs.at(-1)
        if (!last) return
        const r = await call({ kind: 'history', before: last.id })
        if (r.kind === 'history') {
            setRuns((x) => [...x, ...r.runs])
            setMore(r.more)
        }
    }
    const clear = async () => {
        const r = await call({ kind: 'clearHistory' })
        if (r.kind === 'done') {
            setRuns([])
            setShown(null)
            setMore(false)
        } else setMessage(resultText(r) ?? '')
    }
    return (
        <section aria-label='History' data-slot='workbench-history' className='flex flex-col gap-3'>
            <p className='text-academy-ink-soft text-[13px]'>
                Requests sent from this collection on this Mac (30 days, up to 200). History shows what happened; nothing here sends anything again.
            </p>
            {message ? <p role='alert'>{message}</p> : null}
            {runs.length === 0 ? <p className='text-[13px]'>No requests sent yet.</p> : null}
            <ul className='flex flex-col gap-1'>
                {runs.map((r) => (
                    <li key={r.id}>
                        <Button
                            type='button'
                            tier='tertiary'
                            size='sm'
                            className='w-full justify-start text-left'
                            onClick={() =>
                                void call({ kind: 'showRun', run: r.id }).then((x) => {
                                    if (x.kind === 'ran') setShown(x.run)
                                })
                            }
                        >
                            {`${r.method} ${r.relative.replace(/\.request\.toml$/, '')} · ${runSummary(r)} · ${new Date(r.at * 1000).toLocaleString()}`}
                        </Button>
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap gap-2'>
                {more ? (
                    <Button type='button' tier='secondary' size='sm' onClick={() => void older()}>
                        Older requests
                    </Button>
                ) : null}
                {runs.length ? (
                    <Button type='button' tier='tertiary' size='sm' onClick={() => void clear()}>
                        Clear history
                    </Button>
                ) : null}
                <Button type='button' tier='tertiary' size='sm' onClick={() => void call({ kind: 'clearCookies' }).then((r) => setMessage(r.kind === 'done' ? 'Cookies cleared.' : (resultText(r) ?? '')))}>
                    Clear cookies
                </Button>
            </div>
            {shown ? <RunPanel run={shown} call={call} /> : null}
        </section>
    )
}
