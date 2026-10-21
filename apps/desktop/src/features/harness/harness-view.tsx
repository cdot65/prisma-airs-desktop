import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info } from 'lucide-react'
import type {
    ConversationView,
    FeatureId,
    HarnessEntry,
    HarnessFailure,
    HarnessPhase,
    HarnessStop,
    ShellSnapshot,
    TurnEnd,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { HISTORY_FAILURE_TEXT, HistoryPanel, savedAt } from './history-panel'
import { SIGN_IN_FAILURE_TEXT, SignInPanel } from './sign-in-panel'

/** The engine's prompt bound (eight 8 KiB text items). */
const MAX_PROMPT_BYTES = 32 * 1024

/** Why a conversation failed or could not start, as the operator reads it. */
export const HARNESS_FAILURE_TEXT: Record<HarnessFailure, string> = {
    engineUnavailable: 'The bundled engine is missing or is not the one this app was built with. Nothing was started.',
    credentialMissing: 'The inference key is missing. Store it again in the AI Gateway Playground.',
    credentialLocked: 'The Keychain is locked. Unlock it and start again.',
    credentialDenied: 'macOS did not allow access to the inference key.',
    startFailed: 'The engine could not be started on this Mac.',
    bootstrapRejected: 'The engine refused this workspace, key or endpoint.',
    timeout: 'The engine did not answer in time and was stopped.',
    protocol: 'The engine sent something unexpected and was stopped.',
    bounds: 'The engine sent more than this app accepts and was stopped.',
    exited: 'The engine stopped unexpectedly. Anything being answered was not finished.',
    refused: 'The engine refused the request.',
    turnFailed: 'The answer failed. The Gateway or the model ended it with an error.',
    cleanupFailed: 'The engine could not be stopped and removed in time. Quit the app to end it.',
    restoreFailed: 'The saved conversation could not be continued in a new engine. It is still saved; start a new conversation.',
    signInNeeded: 'Your company sign-in must be renewed. Sign in again, then start the conversation.',
}

const PHASE_TEXT: Record<HarnessPhase, string> = {
    starting: 'Starting the engine…',
    ready: 'Ready.',
    answering: 'Answering…',
    interrupting: 'Stopping the answer…',
    renewing: 'Renewing your sign-in and restarting the engine with the conversation…',
    ending: 'Ending the conversation…',
    ended: 'Ended.',
    failed: 'Failed.',
}

const STOP_TEXT: Record<HarnessStop, string> = {
    ended: 'You ended this conversation.',
    sleep: 'This conversation ended when the Mac went to sleep.',
    quit: 'This conversation ended when the app quit.',
    deadline: 'This conversation reached its eight-hour limit and ended.',
}

export const TURN_TEXT: Record<TurnEnd, string> = {
    completed: 'Answered.',
    interrupted: 'The answer was stopped.',
    truncated: 'The answer reached 256 KiB and was stopped; the rest was not kept.',
    // D046: the engine retries a failing Gateway itself; the app stops it at
    // its first reconnect, so up to five requests may have reached it.
    failed:
        'The answer failed: the Gateway or the model ended it with an error. The engine may have sent the message to the Gateway up to five times before this app stopped it, and each may have been processed; nothing was undone.',
    refused: 'The engine did not take that message.',
}

const ENTRY_NOTE: Partial<Record<HarnessEntry['state'], string>> = {
    interrupted: '(stopped before it finished)',
    truncated: '(cut at 256 KiB)',
    failed: '(failed)',
}

/** What the status line says for a conversation. */
export function conversationText(c: ConversationView): string {
    if (c.phase === 'failed' && c.failure) return HARNESS_FAILURE_TEXT[c.failure]
    if (c.phase === 'ended' && c.stop) return STOP_TEXT[c.stop]
    if (c.phase === 'ready' && c.lastTurn) {
        return TURN_TEXT[c.lastTurn]
    }
    return PHASE_TEXT[c.phase]
}

function utf8Bytes(text: string): number {
    return new TextEncoder().encode(text).length
}

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (id: FeatureId) => void
}

/**
 * The native Harness (C19, D046): the pinned engine runs on this Mac, and
 * every answer comes from the chosen AI Gateway workspace with the
 * inference key stored for it. The conversation exists only while it is
 * open: nothing of it is written to disk, and changing the tenant,
 * workspace or key, sleep or quitting ends it.
 */
export const HarnessView = forwardRef<HTMLHeadingElement, Props>(function HarnessView({ snapshot, dispatch, onNavigate }, ref) {
    const ids = useId()
    const context = snapshot.active?.context ?? null
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const selected = snapshot.gateway.selected
    const key = tenant?.inference
    const keyHere = key?.state === 'ready' && selected !== null && key.target?.workspace.id === selected.workspace.id
    const engine = snapshot.harness.engine
    const conversation = snapshot.harness.conversation
    const signIn = snapshot.harness.signIn
    const signInHere = signIn !== null && selected !== null && signIn.workspace.id === selected.workspace.id
    // The credential new conversations use: the company sign-in when it is
    // chosen for this workspace, otherwise the workspace key.
    const usesSignIn = signInHere && signIn.selected
    const credentialReady = usesSignIn ? signIn.account !== null : keyHere
    const endpoint = usesSignIn ? signIn.endpoint : key?.target?.endpoint
    const [prompt, setPrompt] = useState('')
    const [message, setMessage] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const promptRef = useRef<HTMLTextAreaElement>(null)
    const statusRef = useRef<HTMLParagraphElement>(null)
    const startRef = useRef<HTMLButtonElement>(null)
    // Clearing returns focus to Start once it is enabled again.
    const [focusStart, setFocusStart] = useState(false)
    const phase = conversation?.phase ?? null
    const live = phase !== null && phase !== 'ended' && phase !== 'failed'
    const tooLong = utf8Bytes(prompt) > MAX_PROMPT_BYTES

    // Focus follows the conversation: the message box once it is ready to
    // take a message, the status when it ends or fails.
    const wasPhase = useRef<HarnessPhase | null>(phase)
    useEffect(() => {
        const was = wasPhase.current
        wasPhase.current = phase
        if (phase === was) return
        if (phase === 'ready' && was === 'starting') promptRef.current?.focus()
        if ((phase === 'ended' || phase === 'failed') && was !== null) statusRef.current?.focus()
    }, [phase])

    // A message given back by a failed renewal returns to the box.
    const unsent = conversation?.unsent ?? null
    useEffect(() => {
        if (unsent !== null) {
            setPrompt((current) => (current === '' ? unsent : current))
            promptRef.current?.focus()
        }
    }, [unsent])

    useEffect(() => {
        if (focusStart && !busy) {
            setFocusStart(false)
            startRef.current?.focus()
        }
    }, [focusStart, busy])

    async function send(intent: Parameters<Dispatch>[0]): Promise<boolean> {
        setMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') setMessage(outcomeMessage(outcome) ?? 'That was not done.')
            return outcome.kind === 'admitted'
        } finally {
            setBusy(false)
        }
    }

    async function start() {
        if (!context || !selected) return
        if (conversation && !live) {
            const cleared = await send({ kind: 'clearHarness', conversation: conversation.id })
            if (!cleared) return
        }
        await send({ kind: 'startHarness', context, workspace: selected.reference })
    }

    async function submit() {
        if (!conversation || phase !== 'ready' || prompt.trim() === '' || tooLong) return
        if (await send({ kind: 'sendHarness', conversation: conversation.id, prompt })) setPrompt('')
    }

    const ready = engine.available && context !== null && selected !== null && credentialReady
    return (
        <section aria-labelledby={`${ids}-heading`} data-slot='harness' className='flex flex-col gap-5'>
            <PageHeading ref={ref} id={`${ids}-heading`} kicker='Native engine' title='Harness' />
            <Callout tone='default' icon={Info} data-slot='harness-about'>
                <p>
                    The Prisma AIRS Harness engine runs on this Mac. Every answer comes from your AI Gateway workspace, with its inference key or
                    your company sign-in, and nothing else. Each conversation is saved on this Mac after every answer, encrypted, and can be
                    continued later with the same credential. Changing the tenant, the workspace or its credential, sleep or quitting ends the
                    running conversation. Tools, files and project access are not offered yet.
                </p>
                <p className='text-academy-ink-faint mt-2 font-mono text-[11px]'>
                    {`Engine ${engine.runtimeVersion} · ${engine.engineCommit.slice(0, 12)} · ${engine.protocol}`}
                </p>
            </Callout>
            {!engine.available ? (
                <Callout tone='default' icon={AlertTriangle}>
                    The Harness engine is not bundled with this build.
                </Callout>
            ) : !context ? (
                <Callout tone='default' icon={AlertTriangle}>
                    Choose a tenant to use the Harness.
                </Callout>
            ) : !selected ? (
                <Callout tone='default' icon={AlertTriangle}>
                    <p>Choose a workspace in AI Gateway first; the Harness answers with its inference key.</p>
                    <Button type='button' tier='secondary' size='sm' className='mt-2' onClick={() => onNavigate('gateway')}>
                        Go to AI Gateway
                    </Button>
                </Callout>
            ) : (
                <>
                    {!keyHere && !signInHere ? (
                        <Callout tone='default' icon={AlertTriangle}>
                            <p>{`Store an inference key for ${selected.workspace.name} in the AI Gateway Playground, or set up company sign-in below, to use the Harness.`}</p>
                            <Button type='button' tier='secondary' size='sm' className='mt-2' onClick={() => onNavigate('gateway')}>
                                Go to AI Gateway
                            </Button>
                        </Callout>
                    ) : (
                        <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='harness-binding'>
                            <dt className='text-academy-ink-faint'>Workspace</dt>
                            <dd>{selected.workspace.name}</dd>
                            <dt className='text-academy-ink-faint'>Gateway endpoint</dt>
                            <dd className='font-mono text-[12px] break-all'>{endpoint}</dd>
                            <dt className='text-academy-ink-faint'>Credential</dt>
                            <dd>{usesSignIn ? (signIn.account ? `Company sign-in (${signIn.account})` : 'Company sign-in (not signed in)') : 'Workspace inference key'}</dd>
                        </dl>
                    )}
                    <SignInPanel
                        context={context}
                        selected={selected}
                        signIn={signIn}
                        keyHere={keyHere}
                        running={live}
                        dispatch={dispatch}
                        onMessage={setMessage}
                    />
                </>
            )}
            {!live ? (
                <Button ref={startRef} type='button' tier='primary' className='self-start' disabled={!ready || busy} onClick={() => void start()}>
                    {conversation ? 'Start a new conversation' : 'Start conversation'}
                </Button>
            ) : null}
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
            {conversation ? (
                <section aria-labelledby={`${ids}-conversation`} className='flex flex-col gap-3' data-slot='harness-conversation'>
                    <h2 id={`${ids}-conversation`} className='text-[15px] font-semibold'>
                        {`Conversation in ${conversation.workspace.name}`}
                    </h2>
                    <p ref={statusRef} tabIndex={-1} role='status' aria-live='polite' className='text-[13px]' data-slot='harness-status'>
                        {conversationText(conversation)}
                    </p>
                    {conversation.phase === 'failed' ? (
                        <p role='alert' className='sr-only'>
                            {conversationText(conversation)}
                        </p>
                    ) : null}
                    {/* role='list': WebKit drops list semantics from an unstyled list. */}
                    <ol role='list' aria-label='Messages' aria-busy={phase === 'answering'} className='flex flex-col gap-3' data-slot='harness-transcript'>
                        {conversation.entries.map((e) => (
                            <li key={e.id} className='border-academy-rule rounded-[12px] border p-3'>
                                <p className='text-academy-ink-faint text-[12px] font-semibold'>{e.role === 'operator' ? 'You' : 'Harness'}</p>
                                <p className='text-[14px] whitespace-pre-wrap'>{e.text || (e.state === 'streaming' ? '…' : '')}</p>
                                {ENTRY_NOTE[e.state] ? <p className='text-academy-ink-faint text-[12px]'>{ENTRY_NOTE[e.state]}</p> : null}
                            </li>
                        ))}
                    </ol>
                    {conversation.usage ? (
                        <p className='text-academy-ink-faint text-[12px]'>{`Tokens so far: ${conversation.usage.input} in, ${conversation.usage.output} out.`}</p>
                    ) : null}
                    {conversation.saveFailure ? (
                        <p className='text-academy-accent-warn text-[12px]' data-slot='harness-saved'>
                            {`Not saved: ${HISTORY_FAILURE_TEXT[conversation.saveFailure]}`}
                        </p>
                    ) : conversation.savedAt !== null ? (
                        <p className='text-academy-ink-faint text-[12px]' data-slot='harness-saved'>
                            {`Saved ${savedAt(conversation.savedAt)}`}
                        </p>
                    ) : null}
                    {conversation.signInFailure ? (
                        <p role='alert' className='text-academy-accent-warn text-[13px]' data-slot='harness-renew-failure'>
                            {`${SIGN_IN_FAILURE_TEXT[conversation.signInFailure]} Your message was not sent; it is back in the box.`}
                        </p>
                    ) : null}
                    {live ? (
                        <form
                            className='flex flex-col gap-2'
                            onSubmit={(event) => {
                                event.preventDefault()
                                void submit()
                            }}
                        >
                            <label htmlFor={`${ids}-prompt`} className='text-[13px] font-semibold'>
                                Message
                            </label>
                            <textarea
                                id={`${ids}-prompt`}
                                ref={promptRef}
                                rows={4}
                                value={prompt}
                                aria-describedby={`${ids}-hint`}
                                aria-invalid={tooLong}
                                className='border-academy-rule bg-academy-paper rounded-[8px] border p-2 text-[14px]'
                                onChange={(event) => setPrompt(event.target.value)}
                                onKeyDown={(event) => {
                                    if (event.key === 'Enter' && event.metaKey) {
                                        event.preventDefault()
                                        void submit()
                                    }
                                }}
                            />
                            <p id={`${ids}-hint`} className='text-academy-ink-faint text-[12px]'>
                                {tooLong ? 'This message is longer than 32 KiB. Shorten it to send.' : 'Press ⌘↩ to send. Up to 32 KiB.'}
                            </p>
                            <div className='flex flex-wrap gap-2'>
                                <Button type='submit' tier='primary' size='sm' disabled={phase !== 'ready' || busy || prompt.trim() === '' || tooLong}>
                                    Send
                                </Button>
                                {phase === 'answering' ? (
                                    <Button
                                        type='button'
                                        tier='secondary'
                                        size='sm'
                                        onClick={() => void send({ kind: 'interruptHarness', conversation: conversation.id })}
                                    >
                                        Stop answer
                                    </Button>
                                ) : null}
                                <Button
                                    type='button'
                                    tier='tertiary'
                                    size='sm'
                                    disabled={phase === 'ending'}
                                    onClick={() => void send({ kind: 'endHarness', conversation: conversation.id })}
                                >
                                    End conversation
                                </Button>
                            </div>
                        </form>
                    ) : (
                        <div className='flex gap-2'>
                            <Button
                                type='button'
                                tier='tertiary'
                                size='sm'
                                onClick={async () => {
                                    if (await send({ kind: 'clearHarness', conversation: conversation.id })) setFocusStart(true)
                                }}
                            >
                                Clear conversation
                            </Button>
                        </div>
                    )}
                </section>
            ) : null}
            {engine.available && context && selected ? (
                <HistoryPanel
                    context={context}
                    selected={selected}
                    history={snapshot.harness.history}
                    running={live}
                    dispatch={dispatch}
                    onMessage={setMessage}
                />
            ) : null}
        </section>
    )
})
