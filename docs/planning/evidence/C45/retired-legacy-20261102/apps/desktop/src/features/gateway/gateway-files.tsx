import { useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info } from 'lucide-react'
import type { ContextRef, FileActionView, GatewayFile, InferenceFailure, ListedWorkspace, ShellSnapshot, StagedFileFailure, WorkspaceRef } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { INFERENCE_FAILURE_TEXT } from './playground'

/** A byte count as the operator reads it. */
export function sizeText(bytes: number): string {
    if (bytes < 1024) return `${bytes} ${bytes === 1 ? 'byte' : 'bytes'}`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
}

const CHOOSE_FAILURE_TEXT: Record<StagedFileFailure, string> = {
    tooLarge: 'That file is larger than 4 MiB. Nothing was read for upload.',
    notText: 'That file could not be read.',
    empty: 'That file is empty. Nothing was read for upload.',
    unreadable: 'That file could not be read.',
}

/** A creation time as the operator reads it; one no date can render is unknown, never an error. */
export function createdText(seconds: number | null): string {
    const at = seconds === null ? Number.NaN : new Date(seconds * 1000).getTime()
    if (!Number.isFinite(at)) return 'Unknown time'
    // Split, never sliced: a year past 9999 is written with a sign and six digits.
    const [day, time = ''] = new Date(at).toISOString().split('T')
    return `${day} ${time.slice(0, 5)} UTC`
}

/** Why a file request failed (a run's words, except for an answer about another file). */
export function fileFailureText(failure: InferenceFailure): string {
    return failure === 'invalidResponse' ? 'The Gateway’s answer did not describe that file.' : INFERENCE_FAILURE_TEXT[failure]
}

/** How an upload or delete stands (never "nothing happened" once it was sent). */
export function fileActionText(a: FileActionView): string {
    const name = a.filename ?? a.file ?? 'the file'
    const failure = a.failure ? ` ${fileFailureText(a.failure)}` : ''
    const upload = a.action === 'upload'
    switch (a.outcome) {
        case null:
            return upload ? `Uploading ${name}…` : `Deleting ${name}…`
        case 'completed':
            return upload ? `Uploaded ${name} as ${a.file ?? 'a new file'}.` : `Deleted ${name} from the Gateway.`
        case 'rejected':
            return `The Gateway refused to ${upload ? 'store' : 'delete'} ${name}; nothing changed.${failure}`
        case 'notSent':
            return `Nothing was sent: ${name} was not ${upload ? 'uploaded' : 'deleted'}.${failure}`
        case 'unknown':
            return `${upload ? `The upload of ${name} may have been stored` : `${name} may have been deleted`}: its result is not known.${failure} It is kept on this Mac with trace ${a.trace} and is never sent again. Read the files to see what exists.`
    }
}

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    context: ContextRef
    reference: WorkspaceRef
    workspace: ListedWorkspace
    onLogs: (trace: string) => void
}

/**
 * The chosen workspace's Gateway files (C18, D045): read, uploaded and
 * deleted with the workspace's inference key, each request sent once. A
 * file to upload is chosen in the native sheet and read by the desktop;
 * this view sees only its name and size. Files stay in the Gateway until
 * they are deleted here or elsewhere; nothing reads one back.
 */
export function FilesPanel({ snapshot, dispatch, context, reference, workspace, onLogs }: Props) {
    const ids = useId()
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const key = tenant?.inference
    const keyHere = key?.state === 'ready' && key.target?.workspace.id === workspace.id
    const files = snapshot.gateway.files?.workspace.id === workspace.id ? snapshot.gateway.files : null
    const [message, setMessage] = useState<string | null>(null)
    const [confirming, setConfirming] = useState<GatewayFile | null>(null)
    const statusRef = useRef<HTMLParagraphElement>(null)
    const rows = useRef(new Map<string, HTMLTableRowElement>())
    const action = files?.action ?? null
    const going = files?.reading === true || (action !== null && action.outcome === null)
    // An upload or delete pressed here moves focus to its status once it appears.
    const focusNext = useRef(false)
    useEffect(() => {
        if (action && focusNext.current) {
            focusNext.current = false
            statusRef.current?.focus()
        }
    }, [action?.trace])

    async function send(intent: Parameters<Dispatch>[0], fallback: string) {
        setMessage(null)
        const outcome = await dispatch(intent)
        if (outcome.kind !== 'admitted') setMessage(outcomeMessage(outcome) ?? fallback)
        return outcome.kind === 'admitted'
    }

    function focusRow(id: string) {
        requestAnimationFrame(() => rows.current.get(id)?.querySelector<HTMLButtonElement>('button')?.focus())
    }

    const readText = !files
        ? 'Not read yet.'
        : files.reading
          ? 'Reading files…'
          : files.readFailure
            ? `The files could not be read. ${fileFailureText(files.readFailure)}`
            : files.files === null
              ? 'Not read yet.'
              : `${files.files.length === 0 ? 'No files' : `${files.files.length} ${files.files.length === 1 ? 'file' : 'files'}`}${
                    files.more ? ' shown: the Gateway holds more, and only the first 200 are shown here' : ''
                }.`

    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col gap-4' data-slot='gateway-files'>
            <h3 id={`${ids}-h`} className='text-[15px] font-semibold'>
                Files in {workspace.name}
            </h3>
            <Callout tone='default' icon={Info} data-slot='gateway-files-retention'>
                Files are read, uploaded and deleted with this workspace's inference key, each request sent once and never retried. An uploaded
                file stays in the Gateway until it is deleted; this Mac keeps no copy and never reads one back.
            </Callout>
            {!keyHere ? (
                <Callout tone='default' icon={AlertTriangle}>
                    Store an inference key for {workspace.name} in the Playground to use its files.
                </Callout>
            ) : null}
            <div className='flex items-center gap-2'>
                <Button
                    type='button'
                    tier='secondary'
                    size='sm'
                    disabled={!keyHere || going}
                    onClick={() => void send({ kind: 'readGatewayFiles', context, workspace: reference }, 'The files were not read.')}
                >
                    Read files
                </Button>
                <p role='status' aria-live='polite' className='text-[13px]' data-slot='gateway-files-state'>
                    {readText}
                </p>
            </div>
            {files?.files && files.files.length > 0 ? (
                <table className='text-[13px]' data-slot='gateway-files-list'>
                    <caption className='sr-only'>{`Files in ${workspace.name}`}</caption>
                    <thead>
                        <tr>
                            <th scope='col' className='text-left'>
                                Name
                            </th>
                            <th scope='col' className='text-left'>
                                ID
                            </th>
                            <th scope='col' className='text-left'>
                                Size
                            </th>
                            <th scope='col' className='text-left'>
                                Purpose
                            </th>
                            <th scope='col' className='text-left'>
                                Status
                            </th>
                            <th scope='col' className='text-left'>
                                Created
                            </th>
                            <th scope='col'>
                                <span className='sr-only'>Actions</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        {files.files.map((f) => (
                            <tr
                                key={f.id}
                                ref={(tr) => {
                                    if (tr) rows.current.set(f.id, tr)
                                    else rows.current.delete(f.id)
                                }}
                            >
                                <td>{f.filename}</td>
                                <td className='font-mono'>{f.id}</td>
                                <td>{sizeText(f.bytes)}</td>
                                <td>{f.purpose}</td>
                                <td>{f.status}</td>
                                <td>{createdText(f.createdAt)}</td>
                                <td>
                                    <Button
                                        type='button'
                                        tier='tertiary'
                                        size='sm'
                                        aria-label={`Delete ${f.filename} (${f.id})`}
                                        disabled={going}
                                        onClick={() => setConfirming(f)}
                                    >
                                        Delete…
                                    </Button>
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            ) : null}
            {confirming ? (
                <div role='group' aria-labelledby={`${ids}-confirm`} className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-3' data-slot='gateway-files-confirm'>
                    <p id={`${ids}-confirm`} className='text-[13px]'>
                        {`Delete ${confirming.filename} (${confirming.id}) from the Gateway? It is deleted for everyone using ${workspace.name}, and cannot be restored here.`}
                    </p>
                    <div className='flex gap-2'>
                        <Button
                            type='button'
                            tier='primary'
                            size='sm'
                            autoFocus
                            onClick={() => {
                                const target = confirming
                                setConfirming(null)
                                focusNext.current = true
                                void send({ kind: 'deleteGatewayFile', context, workspace: reference, file: target.id }, 'The file was not deleted.')
                            }}
                        >
                            Delete file
                        </Button>
                        <Button
                            type='button'
                            tier='tertiary'
                            size='sm'
                            onClick={() => {
                                const target = confirming
                                setConfirming(null)
                                focusRow(target.id)
                            }}
                        >
                            Cancel
                        </Button>
                    </div>
                </div>
            ) : null}
            <fieldset className='flex flex-col gap-2' data-slot='gateway-files-upload'>
                <legend className='text-[14px] font-semibold'>Upload a file</legend>
                <p className='text-academy-ink-faint text-[12px]'>
                    At most 4 MiB. It is uploaded for the “assistants” purpose, so a Responses run in the Playground can attach it.
                </p>
                <div className='flex flex-wrap items-center gap-2'>
                    <Button
                        type='button'
                        tier='secondary'
                        size='sm'
                        disabled={!keyHere || files?.choosing === true}
                        onClick={() => void send({ kind: 'chooseGatewayFile', context, workspace: reference }, 'The file sheet did not open.')}
                    >
                        Choose file…
                    </Button>
                    <span className='text-[13px]' data-slot='gateway-files-chosen'>
                        {files?.choosing ? 'Choosing…' : files?.chosen ? `${files.chosen.name} · ${sizeText(files.chosen.bytes)}` : 'No file chosen.'}
                    </span>
                </div>
                {files?.chooseFailure ? (
                    <p role='alert' className='text-academy-accent-warn text-[13px]'>
                        {CHOOSE_FAILURE_TEXT[files.chooseFailure]}
                    </p>
                ) : null}
                {files?.chosen ? (
                    <Button
                        type='button'
                        tier='primary'
                        size='sm'
                        className='self-start'
                        disabled={going}
                        onClick={() => {
                            const chosen = files.chosen
                            if (!chosen) return
                            focusNext.current = true
                            void send({ kind: 'uploadGatewayFile', context, workspace: reference, file: chosen.token }, 'The file was not uploaded.')
                        }}
                    >
                        {`Upload ${files.chosen.name} to ${workspace.name}`}
                    </Button>
                ) : null}
            </fieldset>
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
            {action ? (
                <div className='flex flex-col gap-1' data-slot='gateway-files-action'>
                    <p ref={statusRef} tabIndex={-1} role='status' aria-live='polite' className='text-[13px]' data-slot='gateway-files-action-state'>
                        {fileActionText(action)}
                    </p>
                    <p className='flex items-center gap-2 font-mono text-[12px]'>
                        {`Trace ${action.trace}`}
                        <Button type='button' tier='tertiary' size='sm' onClick={() => onLogs(action.trace)}>
                            Find in logs
                        </Button>
                    </p>
                </div>
            ) : null}
        </section>
    )
}
