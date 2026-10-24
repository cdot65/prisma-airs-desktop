import { useId, useState } from 'react'
import { Button } from '@ai-security-academy/presentation'
import type { DiagnosticsClear, DiagnosticsExport, DiagnosticsView } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

const EXPORT_TEXT: Record<DiagnosticsExport, string> = {
    saving: 'Choose where to save the diagnostics in the open dialog.',
    saved: 'Diagnostics saved.',
    cancelled: 'Nothing was saved.',
    failed: 'The diagnostics could not be saved.',
}

const CLEAR_TEXT: Record<DiagnosticsClear, string> = {
    cleared: 'Diagnostics cleared.',
    failed: 'The diagnostics could not all be removed, so the events counted above are still kept. Try again.',
}

interface Props {
    view: DiagnosticsView
    dispatch: Dispatch
}

/**
 * Local, redacted diagnostics (C24, D051). The desktop keeps only the kind
 * of each request and why one was refused, storage and reset outcomes, and
 * where the app stopped unexpectedly; never prompts, responses, keys or
 * names. Nothing is sent anywhere: export is explicit and saved where the
 * operator chooses.
 */
export function DiagnosticsPanel({ view, dispatch }: Props) {
    const legend = useId()
    const [message, setMessage] = useState<string | null>(null)
    const busy = view.export === 'saving'

    async function run(intent: Parameters<Dispatch>[0]) {
        setMessage(null)
        const outcome = await dispatch(intent)
        setMessage(outcomeMessage(outcome))
    }

    return (
        <fieldset aria-labelledby={legend} data-slot='diagnostics' className='border-academy-rule flex flex-col gap-3 rounded-[12px] border p-5'>
            <legend id={legend} className='text-academy-ink px-1 text-[15px] font-semibold'>
                Diagnostics
            </legend>
            {!view.available ? (
                <p className='text-academy-ink-soft text-[13px]'>This build keeps no diagnostics.</p>
            ) : (
                <>
                    <p className='text-academy-ink-soft max-w-prose text-[13px]'>
                        Kept only on this Mac for {view.retentionDays} days and never sent anywhere: the kind of each
                        request and why one was refused, storage and reset outcomes, and where the app stopped
                        unexpectedly. Prompts, responses, keys, tokens and names are never kept.
                    </p>
                    <p className='text-academy-ink text-[13px]' data-slot='diagnostics-count'>
                        {view.events} event{view.events === 1 ? '' : 's'} kept.
                    </p>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void run({ kind: 'exportDiagnostics' })}>
                            Export diagnostics…
                        </Button>
                        <Button type='button' tier='tertiary' disabled={busy} onClick={() => void run({ kind: 'clearDiagnostics' })}>
                            Clear diagnostics
                        </Button>
                    </div>
                    {view.export ? (
                        <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                            {EXPORT_TEXT[view.export]}
                        </p>
                    ) : null}
                    {view.clear === 'failed' ? (
                        <p role='alert' data-slot='diagnostics-clear' className='text-academy-accent-warn text-[13px]'>
                            {CLEAR_TEXT.failed}
                        </p>
                    ) : view.clear === 'cleared' ? (
                        <p role='status' aria-live='polite' data-slot='diagnostics-clear' className='text-academy-ink-soft text-[13px]'>
                            {CLEAR_TEXT.cleared}
                        </p>
                    ) : null}
                </>
            )}
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </fieldset>
    )
}
