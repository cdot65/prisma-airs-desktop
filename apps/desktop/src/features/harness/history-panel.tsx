import { useEffect, useId, useRef, useState } from 'react'
import { Button } from '@ai-security-academy/presentation'
import type {
    ContextRef,
    HarnessHistoryView,
    HistoryFailure,
    HistoryFormat,
    NotContinuable,
    SelectedWorkspace,
} from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../tenants/dispatch'

/** Why protected history could not be read or written. */
export const HISTORY_FAILURE_TEXT: Record<HistoryFailure, string> = {
    locked: 'The Keychain is locked, so saved conversations cannot be read or saved. Nothing was written unencrypted.',
    denied: 'macOS did not allow access to the history key. Nothing was written unencrypted.',
    unavailable: 'Saved conversations are not available on this Mac right now.',
    full: 'This workspace already keeps 16 saved conversations. Delete one to keep new conversations.',
    tooLarge: 'This conversation is too large to save.',
    unreadable: 'This saved conversation cannot be read with this workspace’s key.',
    missing: 'That saved conversation no longer exists.',
}

const NOT_CONTINUABLE_TEXT: Record<NotContinuable, string> = {
    running: 'This is the conversation running now.',
    otherCredential:
        'It ran with another key, sign-in or endpoint than this workspace uses now, so it can be read and exported but not continued.',
    otherProject:
        'It ran with another project folder, or with changes allowed differently, than the Harness uses now. Choose that folder and access again to continue it, or start a new conversation.',
    otherMcp:
        'It ran with other workspace MCP servers or sign-ins than this workspace uses now, or they could not be checked. Turn the same servers on and sign in to them to continue it, or start a new conversation.',
    engineUnavailable: 'The Harness engine is not bundled with this build.',
    workspaceUnavailable: 'AI Gateway’s workspaces could not be read, so it can be read and exported here but not continued.',
    notRestorable:
        'It was cut short before the engine saved anything to restore, so it can be read and exported but not continued. A call it shows as interrupted may have run.',
}

const FORMATS: readonly [HistoryFormat, string][] = [
    ['markdown', 'Markdown'],
    ['text', 'Plain text'],
    ['json', 'JSON'],
]

export function savedAt(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

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

/**
 * The chosen workspace's saved conversations (C20): saved encrypted on this
 * Mac after every answer. One can be read, exported, deleted, or continued
 * in a new engine when it ran with the credential this workspace uses now.
 * With no workspace chosen, the last chosen one's, read from this Mac only
 * (T23): read, exported and deleted, never continued.
 */
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

    async function send(intent: Parameters<Dispatch>[0]): Promise<boolean> {
        onMessage(null)
        const outcome = await dispatch(intent)
        if (outcome.kind !== 'admitted') onMessage('That was not done.')
        return outcome.kind === 'admitted'
    }

    const busy = history?.busy ?? false
    const exportState = history?.export ?? null
    if (none || (selected === null && history !== null && !history.local)) return null
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-history'>
            <h2 ref={listRef} tabIndex={-1} id={`${ids}-heading`} className='text-[15px] font-semibold'>
                Saved conversations
            </h2>
            <p className='text-academy-ink-faint text-[12px]'>
                {selected
                    ? `Conversations in ${selected.workspace.name} are saved on this Mac after every answer, encrypted with a key kept in the Keychain. Up to 16 are kept; nothing is deleted to make room.`
                    : 'AI Gateway’s workspaces are not available, so these are the conversations saved on this Mac for the workspace you chose last. They can be read, exported and deleted here; continuing one needs that workspace from AI Gateway.'}
            </p>
            {history === null || history.state === 'loading' ? (
                <p role='status' className='text-[13px]'>
                    Reading saved conversations…
                </p>
            ) : history.state === 'failed' && history.failure ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {HISTORY_FAILURE_TEXT[history.failure]}
                </p>
            ) : history.conversations.length === 0 && history.unreadable.length === 0 ? (
                <p className='text-[13px]'>No saved conversations in this workspace.</p>
            ) : (
                <ul role='list' aria-label='Saved conversations' className='flex flex-col gap-2' data-slot='harness-history-list'>
                    {history.conversations.map((c) => (
                        <li key={c.id} className='border-academy-rule flex flex-wrap items-center gap-3 rounded-[12px] border p-3'>
                            <span className='flex min-w-0 flex-1 flex-col'>
                                <span className='truncate text-[14px]'>{c.title || 'Untitled conversation'}</span>
                                <span className='text-academy-ink-faint text-[12px]'>
                                    {`${c.entries} messages · ${savedAt(c.updated)} · ${c.source === 'companySignIn' ? 'company sign-in' : 'workspace key'}`}
                                </span>
                            </span>
                            <Button
                                type='button'
                                tier='secondary'
                                size='sm'
                                disabled={busy}
                                aria-label={`Open ${c.title || 'untitled conversation'}`}
                                onClick={() => void send({ kind: 'openHarnessHistory', context, workspace, id: c.id })}
                            >
                                Open
                            </Button>
                        </li>
                    ))}
                    {history.unreadable.map((id) => (
                        <li key={id} className='border-academy-rule flex flex-wrap items-center gap-3 rounded-[12px] border p-3'>
                            <span className='text-academy-ink-faint flex-1 text-[13px]'>{HISTORY_FAILURE_TEXT.unreadable}</span>
                            {/* Its key may be missing only for now (C20 F5): deleting asks first. */}
                            {confirming === id ? (
                                <>
                                    <span role='alert' className='w-full text-[13px]'>
                                        Delete this unreadable conversation from this Mac? If its key comes back it could be read again; once deleted
                                        it cannot be recovered.
                                    </span>
                                    <Button
                                        type='button'
                                        tier='secondary'
                                        size='sm'
                                        disabled={busy}
                                        onClick={async () => {
                                            setConfirming(null)
                                            await send({ kind: 'deleteHarnessHistory', context, workspace, id })
                                        }}
                                    >
                                        Delete permanently
                                    </Button>
                                    <Button type='button' tier='tertiary' size='sm' onClick={() => setConfirming(null)}>
                                        Keep it
                                    </Button>
                                </>
                            ) : (
                                <Button type='button' tier='tertiary' size='sm' disabled={busy} onClick={() => setConfirming(id)}>
                                    Delete unreadable conversation…
                                </Button>
                            )}
                        </li>
                    ))}
                </ul>
            )}
            {history?.full ? <p className='text-academy-accent-warn text-[13px]'>{HISTORY_FAILURE_TEXT.full}</p> : null}
            {history?.problem ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]' data-slot='harness-history-problem'>
                    {HISTORY_FAILURE_TEXT[history.problem]}
                </p>
            ) : null}
            {opened ? (
                <section aria-labelledby={`${ids}-opened`} className='flex flex-col gap-3' data-slot='harness-history-opened'>
                    <h3 ref={openedRef} tabIndex={-1} id={`${ids}-opened`} className='text-[14px] font-semibold'>
                        {opened.title || 'Untitled conversation'}
                    </h3>
                    <p className='text-academy-ink-faint text-[12px]'>{`Saved ${savedAt(opened.updated)}`}</p>
                    <ol role='list' aria-label='Saved messages' className='flex flex-col gap-2'>
                        {opened.entries.map((e) => (
                            <li key={e.id} className='border-academy-rule rounded-[12px] border p-3'>
                                <p className='text-academy-ink-faint text-[12px] font-semibold'>{e.role === 'operator' ? 'You' : 'Harness'}</p>
                                <p className='text-[14px] whitespace-pre-wrap'>{e.text}</p>
                            </li>
                        ))}
                    </ol>
                    {opened.notContinuable ? <p className='text-[13px]'>{NOT_CONTINUABLE_TEXT[opened.notContinuable]}</p> : null}
                    <div className='flex flex-wrap items-end gap-2'>
                        {opened.notContinuable === null && workspace !== null ? (
                            <Button
                                type='button'
                                tier='primary'
                                size='sm'
                                disabled={busy || running}
                                onClick={() => void send({ kind: 'resumeHarness', context, workspace, id: opened.id })}
                            >
                                Continue this conversation
                            </Button>
                        ) : null}
                        <label className='flex flex-col gap-1 text-[12px]'>
                            <span>Export as</span>
                            <select
                                value={format}
                                onChange={(event) => setFormat(event.target.value as HistoryFormat)}
                                className='border-academy-rule rounded-[8px] border p-1 text-[13px]'
                            >
                                {FORMATS.map(([value, label]) => (
                                    <option key={value} value={value}>
                                        {label}
                                    </option>
                                ))}
                            </select>
                        </label>
                        <Button
                            type='button'
                            tier='secondary'
                            size='sm'
                            disabled={exportState?.state === 'saving'}
                            onClick={() => void send({ kind: 'exportHarnessHistory', context, workspace, id: opened.id, format })}
                        >
                            Export…
                        </Button>
                        {confirming === opened.id ? (
                            <>
                                <Button
                                    type='button'
                                    tier='secondary'
                                    size='sm'
                                    disabled={busy}
                                    onClick={async () => {
                                        setConfirming(null)
                                        await send({ kind: 'deleteHarnessHistory', context, workspace, id: opened.id })
                                    }}
                                >
                                    Delete permanently
                                </Button>
                                <Button type='button' tier='tertiary' size='sm' onClick={() => setConfirming(null)}>
                                    Keep it
                                </Button>
                            </>
                        ) : (
                            <Button
                                type='button'
                                tier='tertiary'
                                size='sm'
                                disabled={busy || opened.notContinuable === 'running'}
                                onClick={() => setConfirming(opened.id)}
                            >
                                Delete…
                            </Button>
                        )}
                        <Button type='button' tier='tertiary' size='sm' onClick={() => void send({ kind: 'closeHarnessHistory', context, workspace })}>
                            Close
                        </Button>
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
