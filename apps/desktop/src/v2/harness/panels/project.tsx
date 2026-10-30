import { useId, useState } from 'react'
import type { ContextRef, HarnessProjectView } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../../../features/tenants/dispatch'
import { Btn } from './controls'

interface Props {
    context: ContextRef
    project: HarnessProjectView | null
    /** A folder just refused as the project. */
    refused: string | null
    /** A conversation is running (changing the project or access ends it). */
    running: boolean
    dispatch: Dispatch
    onMessage: (message: string | null) => void
}

const REFUSED = 'Folders that hold system files, credentials or this app’s own data cannot be used.'

/**
 * The folder the Harness works in (C21, D048), as a v2 panel (C38): chosen
 * in the native folder sheet, read-only unless the operator allows changes.
 * Every command and file change still waits for the operator's approval,
 * and the engine's sandbox keeps both inside the folder.
 */
export function ProjectPanel({ context, project, refused, running, dispatch, onMessage }: Props) {
    const ids = useId()
    const [busy, setBusy] = useState(false)

    async function send(intent: Parameters<Dispatch>[0]) {
        onMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') onMessage(`That was not done. ${REFUSED}`)
        } finally {
            setBusy(false)
        }
    }

    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-2 rounded-[12px] border p-3' style={{ borderColor: 'var(--v2-rule)' }} data-slot='harness-project'>
            <h2 id={`${ids}-heading`} className='text-[13px] font-bold'>
                Project folder
            </h2>
            {project ? (
                <>
                    <p className='font-mono text-[12px] break-all' data-slot='harness-project-path'>
                        {project.path}
                    </p>
                    <label className='flex items-start gap-2 text-[13px]'>
                        <input
                            type='checkbox'
                            className='mt-[3px] size-4 min-h-0'
                            checked={project.write}
                            disabled={busy}
                            aria-describedby={`${ids}-write`}
                            onChange={(event) => void send({ kind: 'setHarnessWrite', context, write: event.target.checked })}
                        />
                        Allow changes in this folder
                    </label>
                    <p id={`${ids}-write`} className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                        {project.write
                            ? 'The Harness may ask to run commands and change files here. Each waits for your approval and stays inside this folder.'
                            : 'Read-only: the Harness may ask to run commands here, each with your approval, but cannot change anything.'}
                    </p>
                </>
            ) : (
                <p className='text-[13px]' style={{ color: 'var(--v2-faint)' }}>
                    No folder. The Harness can use this app’s own read tools and propose changes for your review, but cannot run commands.
                </p>
            )}
            {refused ? (
                <p role='alert' className='text-[13px] break-all' style={{ color: 'var(--v2-flag)' }} data-slot='harness-project-refused'>
                    {`${refused} cannot be used. ${REFUSED}`}
                </p>
            ) : null}
            {running ? (
                <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                    Changing the folder or its access ends the running conversation.
                </p>
            ) : null}
            <div className='flex flex-wrap gap-2'>
                <Btn disabled={busy} onClick={() => void send({ kind: 'chooseHarnessProject', context })}>
                    {project ? 'Choose another folder…' : 'Choose a folder…'}
                </Btn>
                {project ? (
                    <Btn tier='tertiary' disabled={busy} onClick={() => void send({ kind: 'clearHarnessProject', context })}>
                        Remove folder
                    </Btn>
                ) : null}
            </div>
        </section>
    )
}
