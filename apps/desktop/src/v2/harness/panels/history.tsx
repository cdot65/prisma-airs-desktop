import { useEffect, useId, useRef, useState } from 'react'
import type { ContextRef, HarnessHistoryView, HistoryFormat, SelectedWorkspace } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../../../features/tenants/dispatch'
import { HISTORY_FAILURE_TEXT, HISTORY_FORMATS, NOT_CONTINUABLE_TEXT, actionOutcomeText, savedAt } from '../text'
import { Btn } from './controls'

interface Props {
    context: ContextRef
    /** None while no workspace is chosen (as when AI Gateway cannot be read). */
    selected: SelectedWorkspace | null
    history: HarnessHistoryView | null
    /** A conversation is running (one at a time). */
    running: boolean
    dispatch: Dispatch
    onMessage: (message: string | null) => void
}

const faint = { color: 'var(--v2-faint)' }
const warn = { color: 'var(--v2-flag)' }
const card = { borderColor: 'var(--v2-rule)' }

/**
 * The chosen workspace's saved conversations (C20), as a v2 panel (C38):
 * saved encrypted on this Mac after every answer. One can be read,
 * exported, deleted, or continued in a new engine when it ran with the
 * credential this workspace uses now. With no workspace chosen, the last
 * chosen one's, read from this Mac only (T23): read, exported and deleted,
 * never continued.
 */
const CHECK = 'Check the conversations shown here before trying again.'

export function HistoryPanel({ context, selected, history, running, dispatch, onMessage }: Props) {
    const ids = useId()
    const [format, setFormat] = useState<HistoryFormat>('markdown')
    const [confirming, setConfirming] = useState<string | null>(null)
    const openedRef = useRef<HTMLHeadingElement>(null)
    const listRef = useRef<HTMLHeadingElement>(null)
    const workspace = selected?.reference ?? null
    // Without a chosen workspace there may be none saved on this Mac either.
    const [none, setNone] = useState(false)

    // Read the list whenever the chosen workspace changes.
    useEffect(() => {
        let current = true
        setNone(false)
        void dispatch({ kind: 'listHarnessHistory', context, workspace }).then((outcome) => {
            if (current) setNone(workspace === null && outcome.kind !== 'admitted')
        })
        return () => {
            current = false
        }
    }, [dispatch, context, workspace])

    const opened = history?.opened ?? null
    const wasOpened = useRef<string | null>(opened?.id ?? null)
    useEffect(() => {
        const was = wasOpened.current
        wasOpened.current = opened?.id ?? null
        if (opened && opened.id !== was) openedRef.current?.focus()
        if (!opened && was !== null) listRef.current?.focus()
    }, [opened])

    // An unknown delivery stays Not confirmed naming the action (a deletion is permanent and may have happened;
    // the conversations shown here are the state to reconcile with); a refusal says its own reason.
    async function send(intent: Parameters<Dispatch>[0], action: string): Promise<boolean> {
        onMessage(null)
        const outcome = await dispatch(intent)
        onMessage(actionOutcomeText(outcome, action, CHECK))
        return outcome.kind === 'admitted'
    }

    const busy = history?.busy ?? false
    const exportState = history?.export ?? null
    if (none || (selected === null && history !== null && !history.local)) return null
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-history'>
            <h2 ref={listRef} tabIndex={-1} id={`${ids}-heading`} className='text-[13px] font-bold'>
                Saved conversations
            </h2>
            <p className='text-[12px]' style={faint}>
                {selected
                    ? `Conversations in ${selected.workspace.name} are saved on this Mac after every answer, encrypted with a key kept in the Keychain. Up to 16 are kept; nothing is deleted to make room.`
                    : 'AI Gateway’s workspaces are not available, so these are the conversations saved on this Mac for the workspace you chose last. They can be read, exported and deleted here; continuing one needs that workspace from AI Gateway.'}
            </p>
            {history === null || history.state === 'loading' ? (
                <p role='status' className='text-[13px]'>
                    Reading saved conversations…
                </p>
            ) : history.state === 'failed' && history.failure ? (
                <p role='alert' className='text-[13px]' style={warn}>
                    {HISTORY_FAILURE_TEXT[history.failure]}
                </p>
            ) : history.conversations.length === 0 && history.unreadable.length === 0 ? (
                <p className='text-[13px]'>No saved conversations in this workspace.</p>
            ) : (
                <ul role='list' aria-label='Saved conversations' className='flex flex-col gap-2' data-slot='harness-history-list'>
                    {history.conversations.map((c) => (
                        <li key={c.id} className='flex flex-wrap items-center gap-3 rounded-[10px] border p-2.5' style={card}>
                            <span className='flex min-w-0 flex-1 flex-col'>
                                <span className='truncate text-[13px] font-semibold'>{c.title || 'Untitled conversation'}</span>
                                <span className='text-[12px]' style={faint}>
                                    {`${c.entries} messages · ${savedAt(c.updated)} · ${c.source === 'companySignIn' ? 'company sign-in' : 'workspace key'}`}
                                </span>
                            </span>
                            <Btn disabled={busy} aria-label={`Open ${c.title || 'untitled conversation'}`} onClick={() => void send({ kind: 'openHarnessHistory', context, workspace, id: c.id }, 'opening the conversation')}>
                                Open
                            </Btn>
                        </li>
                    ))}
                    {history.unreadable.map((id) => (
                        <li key={id} className='flex flex-wrap items-center gap-3 rounded-[10px] border p-2.5' style={card}>
                            <span className='flex-1 text-[13px]' style={faint}>
                                {HISTORY_FAILURE_TEXT.unreadable}
                            </span>
                            {/* Its key may be missing only for now (C20 F5): deleting asks first. */}
                            {confirming === id ? (
                                <>
                                    <span role='alert' className='w-full text-[13px]'>
                                        Delete this unreadable conversation from this Mac? If its key comes back it could be read again; once deleted it cannot be recovered.
                                    </span>
                                    <Btn
                                        disabled={busy}
                                        onClick={async () => {
                                            setConfirming(null)
                                            await send({ kind: 'deleteHarnessHistory', context, workspace, id }, 'deleting the unreadable conversation')
                                        }}
                                    >
                                        Delete permanently
                                    </Btn>
                                    <Btn tier='tertiary' onClick={() => setConfirming(null)}>
                                        Keep it
                                    </Btn>
                                </>
                            ) : (
                                <Btn tier='tertiary' disabled={busy} onClick={() => setConfirming(id)}>
                                    Delete unreadable conversation…
                                </Btn>
                            )}
                        </li>
                    ))}
                </ul>
            )}
            {history?.full ? (
                <p className='text-[13px]' style={warn}>
                    {HISTORY_FAILURE_TEXT.full}
                </p>
            ) : null}
            {history?.problem ? (
                <p role='alert' className='text-[13px]' style={warn} data-slot='harness-history-problem'>
                    {HISTORY_FAILURE_TEXT[history.problem]}
                </p>
            ) : null}
            {opened ? (
                <section aria-labelledby={`${ids}-opened`} className='flex flex-col gap-3' data-slot='harness-history-opened'>
                    <h3 ref={openedRef} tabIndex={-1} id={`${ids}-opened`} className='text-[13px] font-semibold'>
                        {opened.title || 'Untitled conversation'}
                    </h3>
                    <p className='text-[12px]' style={faint}>{`Saved ${savedAt(opened.updated)}`}</p>
                    <ol role='list' aria-label='Saved messages' className='flex flex-col gap-2'>
                        {opened.entries.map((e) => (
                            <li key={e.id} className='rounded-[10px] border p-2.5' style={card}>
                                <p className='text-[12px] font-semibold' style={faint}>
                                    {e.role === 'operator' ? 'You' : 'Harness'}
                                </p>
                                <p className='text-[13px] whitespace-pre-wrap'>{e.text}</p>
                            </li>
                        ))}
                    </ol>
                    {opened.notContinuable ? <p className='text-[13px]'>{NOT_CONTINUABLE_TEXT[opened.notContinuable]}</p> : null}
                    <div className='flex flex-wrap items-end gap-2'>
                        {opened.notContinuable === null && workspace !== null ? (
                            <Btn tier='primary' disabled={busy || running} onClick={() => void send({ kind: 'resumeHarness', context, workspace, id: opened.id }, 'continuing the conversation')}>
                                Continue this conversation
                            </Btn>
                        ) : null}
                        <label className='flex flex-col gap-1 text-[12px]'>
                            <span>Export as</span>
                            <select value={format} onChange={(event) => setFormat(event.target.value as HistoryFormat)} className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px]' style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-paper)' }}>
                                {HISTORY_FORMATS.map(([value, label]) => (
                                    <option key={value} value={value}>
                                        {label}
                                    </option>
                                ))}
                            </select>
                        </label>
                        <Btn disabled={exportState?.state === 'saving'} onClick={() => void send({ kind: 'exportHarnessHistory', context, workspace, id: opened.id, format }, 'exporting the conversation')}>
                            Export…
                        </Btn>
                        {confirming === opened.id ? (
                            <>
                                <Btn
                                    disabled={busy}
                                    onClick={async () => {
                                        setConfirming(null)
                                        await send({ kind: 'deleteHarnessHistory', context, workspace, id: opened.id }, 'deleting the conversation')
                                    }}
                                >
                                    Delete permanently
                                </Btn>
                                <Btn tier='tertiary' onClick={() => setConfirming(null)}>
                                    Keep it
                                </Btn>
                            </>
                        ) : (
                            <Btn tier='tertiary' disabled={busy || opened.notContinuable === 'running'} onClick={() => setConfirming(opened.id)}>
                                Delete…
                            </Btn>
                        )}
                        <Btn tier='tertiary' onClick={() => void send({ kind: 'closeHarnessHistory', context, workspace }, 'closing the conversation')}>
                            Close
                        </Btn>
                    </div>
                    {confirming === opened.id ? (
                        <p role='alert' className='text-[13px]'>
                            Delete this saved conversation from this Mac? It cannot be recovered.
                        </p>
                    ) : null}
                    {exportState ? (
                        <p role='status' className='text-[13px]' data-slot='harness-export'>
                            {exportState.state === 'saving'
                                ? 'Choose where to save the transcript…'
                                : exportState.state === 'saved'
                                  ? `Saved ${exportState.name ?? 'the transcript'}.`
                                  : exportState.state === 'cancelled'
                                    ? 'Export cancelled.'
                                    : 'The transcript could not be saved.'}
                        </p>
                    ) : null}
                </section>
            ) : null}
        </section>
    )
}
