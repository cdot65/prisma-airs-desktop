import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info } from 'lucide-react'
import type {
    ContextRef,
    GatewayList,
    InferenceApi,
    InferenceFailure,
    InferenceRun,
    ListedWorkspace,
    PlaygroundDraft,
    PlaygroundDraftInput,
    PlaygroundState,
    PlaygroundView,
    ShellSnapshot,
    StorageMode,
    WorkspaceRef,
} from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

/** Why a run did not complete, as the operator reads it. */
export const INFERENCE_FAILURE_TEXT: Record<InferenceFailure, string> = {
    blocked: 'A Gateway guardrail blocked the request or its response.',
    unauthorized: 'The Gateway did not accept the inference key. Store the key again.',
    denied: 'The inference key may not use this model or route.',
    notFound: 'The Gateway did not find this model, route or endpoint.',
    rejected: 'The Gateway rejected the request.',
    rateLimited: 'The Gateway is limiting requests. Wait, then run again.',
    unavailable: 'The Gateway or its provider failed.',
    timeout: 'The run took too long and was stopped.',
    network: 'The Gateway could not be reached, or the connection ended.',
    destination: 'The Gateway tried to send the run somewhere else. Nothing was followed.',
    invalidResponse: 'The Gateway sent something that is not a valid streamed answer. The run ended there.',
    modelFailed: 'The model failed while answering. The Gateway reported the response as failed.',
    credentialLocked: 'The Keychain is locked. Unlock it and run again.',
    credentialDenied: 'macOS did not allow access to the inference key.',
    credentialMissing: 'The inference key is missing. Store it again.',
    configuration: 'The run could not be set up with this endpoint and key.',
}

const STATE_TEXT: Record<PlaygroundState, string> = {
    running: 'Receiving…',
    completed: 'Completed.',
    incomplete: 'Incomplete: the Gateway ended the answer before it was finished. The output below is partial.',
    truncated: 'Stopped at 64 KiB of output: the rest was not read.',
    stopped: 'Stopped.',
    failed: 'Failed.',
}

const DRAFT_PAUSE_MS = 800

const API_TEXT: Record<InferenceApi, string> = {
    chat: 'Chat completions',
    responses: 'Responses',
}

/**
 * Keeps a draft after a pause in typing, and at once when the form loses
 * focus or closes: no pending edit is lost by leaving.
 */
function useKeeper(send: (draft: PlaygroundDraftInput) => void) {
    const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
    const pending = useRef<PlaygroundDraftInput | null>(null)
    const sender = useRef(send)
    sender.current = send
    const flush = useCallback(() => {
        if (timer.current) clearTimeout(timer.current)
        timer.current = null
        const draft = pending.current
        pending.current = null
        if (draft) sender.current(draft)
    }, [])
    const keep = useCallback(
        (draft: PlaygroundDraftInput) => {
            pending.current = draft
            if (timer.current) clearTimeout(timer.current)
            timer.current = setTimeout(flush, DRAFT_PAUSE_MS)
        },
        [flush]
    )
    useEffect(() => flush, [flush])
    return { keep, flush }
}

interface Fields {
    api: InferenceApi
    model: string
    route: string
    system: string
    prompt: string
    file: string
}

function draftOf(f: Fields, id: string | undefined): PlaygroundDraftInput {
    const draft: PlaygroundDraftInput = { api: f.api, model: f.model, prompt: f.prompt }
    if (id) draft.id = id
    if (f.route) draft.route = f.route
    if (f.system) draft.system = f.system
    if (f.file) draft.file = f.file
    return draft
}

function ApiChoice({ name, value, onChange, label }: { name: string; value: InferenceApi; onChange: (api: InferenceApi) => void; label: string }) {
    return (
        <div role='radiogroup' aria-label={label} className='flex gap-4 text-[13px]'>
            {(['chat', 'responses'] as const).map((api) => (
                <label key={api} className='flex items-center gap-1'>
                    <input type='radio' name={name} checked={value === api} onChange={() => onChange(api)} /> {API_TEXT[api]}
                </label>
            ))}
        </div>
    )
}

/** Why the Gateway ended an answer early, from its sanitized finish reason. */
export function finishText(finish: string | null): string {
    switch (finish) {
        case 'max_output_tokens':
        case 'length':
            return `It reached the output token limit (${finish}).`
        case 'content_filter':
            return 'A content filter stopped it (content_filter).'
        case null:
            return 'The Gateway gave no reason.'
        default:
            return `The Gateway gave the reason “${finish}”.`
    }
}

/** How a run stands, in one sentence group (never "nothing was charged"). */
export function runText(run: PlaygroundView): string {
    const failure = run.failure ? ` ${INFERENCE_FAILURE_TEXT[run.failure]}` : run.state === 'incomplete' ? ` ${finishText(run.finish)}` : ''
    const charged =
        run.state === 'stopped' || (run.state === 'failed' && run.outcome === 'unknown')
            ? run.sent
                ? ' The Gateway may have run it and charged for it: find it in the logs by its trace.'
                : ' Nothing was sent.'
            : ''
    return `${STATE_TEXT[run.state]}${failure}${charged}`
}

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    context: ContextRef
    reference: WorkspaceRef
    workspace: ListedWorkspace
    routes: GatewayList | null
    onLogs: (trace: string) => void
}

const routeRows = (list: GatewayList | null) =>
    list ? (JSON.parse(list.rows) as Record<string, unknown>[]).filter((r) => typeof r.id === 'string') : []

/**
 * The Playground (C18): one streamed chat completion at a time from the
 * chosen workspace, sent with that workspace's inference key to the
 * endpoint stored with it. A run is always an explicit press; it is never
 * retried, and stopping it does not undo what the Gateway charged.
 */
export function PlaygroundPanel({ snapshot, dispatch, context, reference, workspace, routes, onLogs }: Props) {
    const ids = useId()
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const key = tenant?.inference
    const target = key?.target ?? null
    const keyHere = key?.state === 'ready' && target?.workspace.id === workspace.id
    const run = snapshot.gateway.playground
    const kept = snapshot.gateway.local.playground.find((d) => d.workspace.id === workspace.id)
    const [api, setApi] = useState<InferenceApi>(kept?.api ?? 'chat')
    const [model, setModel] = useState(kept?.model ?? '')
    const [route, setRoute] = useState(kept?.route ?? '')
    const [system, setSystem] = useState(kept?.system ?? '')
    const [prompt, setPrompt] = useState(kept?.prompt ?? '')
    const [file, setFile] = useState(kept?.file ?? '')
    const [endpoint, setEndpoint] = useState(target?.endpoint ?? '')
    const [secret, setSecret] = useState('')
    const [mode, setMode] = useState<StorageMode>('keychain')
    const [message, setMessage] = useState<string | null>(null)
    const [keyMessage, setKeyMessage] = useState<string | null>(null)
    const restored = useRef(kept !== undefined)
    const statusRef = useRef<HTMLParagraphElement>(null)
    // A kept draft is saved by its ID (that needs no current workspace
    // reference); a new one under the chosen workspace.
    const keptId = useRef(kept?.id)
    keptId.current = kept?.id
    const { keep: hold, flush } = useKeeper((draft) =>
        void dispatch(draft.id ? { kind: 'savePlaygroundDraft', context, draft } : { kind: 'savePlaygroundDraft', context, workspace: reference, draft })
    )

    // A draft read after the panel opened (the book loads) fills an untouched form.
    useEffect(() => {
        if (kept && !restored.current) {
            restored.current = true
            setApi(kept.api)
            setModel(kept.model)
            setRoute(kept.route ?? '')
            setSystem(kept.system ?? '')
            setPrompt(kept.prompt)
            setFile(kept.file ?? '')
        }
    }, [kept?.id])

    function keep(next: Partial<Fields>) {
        restored.current = true
        hold(draftOf({ api, model, route, system, prompt, file, ...next }, keptId.current))
    }

    async function storeKey() {
        setKeyMessage(null)
        const outcome = await dispatch({ kind: 'configureInferenceCredential', context, mode, workspace: reference, endpoint: endpoint.trim(), secret })
        if (outcome.kind === 'admitted') setSecret('')
        else setKeyMessage(outcomeMessage(outcome) ?? 'The key was not stored. Check the endpoint: an https address without a query.')
    }

    async function removeKey() {
        setKeyMessage(null)
        const outcome = await dispatch({ kind: 'removeInferenceCredential', context })
        if (outcome.kind !== 'admitted') setKeyMessage(outcomeMessage(outcome))
    }

    async function start() {
        setMessage(null)
        flush()
        const r: InferenceRun = { api, model: model.trim(), prompt }
        if (route) r.route = route
        if (system.trim()) r.system = system
        if (api === 'responses' && file) r.file = file
        const outcome = await dispatch({ kind: 'runInference', context, workspace: reference, run: r })
        if (outcome.kind !== 'admitted')
            setMessage(
                outcome.kind === 'refused' && outcome.refusal === 'invalidInput' && r.file
                    ? 'The attached file is not in the files last read for this workspace with this key. Read the files again in the Files tab, or remove the attachment.'
                    : (outcomeMessage(outcome) ?? 'The run was not started.')
            )
        else statusRef.current?.focus()
    }

    const running = run?.state === 'running'
    const rows = routeRows(routes)
    const read = snapshot.gateway.files?.workspace.id === workspace.id ? snapshot.gateway.files.files : null
    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col gap-4' data-slot='gateway-playground'>
            <h3 id={`${ids}-h`} className='text-[15px] font-semibold'>
                Playground for {workspace.name}
            </h3>
            <fieldset className='flex flex-col gap-2' data-slot='gateway-inference-key'>
                <legend className='text-[14px] font-semibold'>Inference key</legend>
                <p className='text-[13px]' data-slot='gateway-inference-key-state'>
                    {key?.state === 'storing'
                        ? 'Storing the key…'
                        : key?.state === 'failed'
                          ? 'The key could not be stored.'
                          : !target || key?.state !== 'ready'
                            ? 'No inference key is stored for this tenant. The management client is never used for inference.'
                            : keyHere
                              ? `A key for ${workspace.name} is stored (${key.mode === 'session' ? 'this session only' : 'Keychain'}). It is sent only to ${target.endpoint}.`
                              : `The stored key is for workspace ${target.workspace.name}. Store a key for ${workspace.name} to run here (it replaces that one).`}
                </p>
                <form
                    className='flex flex-col gap-2'
                    onSubmit={(e) => {
                        e.preventDefault()
                        void storeKey()
                    }}
                >
                    <label className='flex flex-col text-[13px]'>
                        Runtime endpoint
                        <input
                            type='url'
                            value={endpoint}
                            onChange={(e) => setEndpoint(e.target.value)}
                            placeholder='https://'
                            spellCheck={false}
                            autoComplete='off'
                            className='font-mono'
                        />
                    </label>
                    <label className='flex flex-col text-[13px]'>
                        Workspace API key with completions access
                        <input type='password' value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete='off' spellCheck={false} />
                    </label>
                    <div role='radiogroup' aria-label='Where the key is kept' className='flex gap-4 text-[13px]'>
                        <label className='flex items-center gap-1'>
                            <input type='radio' name={`${ids}-mode`} checked={mode === 'keychain'} onChange={() => setMode('keychain')} /> Keychain
                        </label>
                        <label className='flex items-center gap-1'>
                            <input type='radio' name={`${ids}-mode`} checked={mode === 'session'} onChange={() => setMode('session')} /> This session only
                        </label>
                    </div>
                    <div className='flex gap-2'>
                        <Button type='submit' tier='secondary' size='sm' disabled={secret.length === 0 || endpoint.trim().length === 0}>
                            Store key for {workspace.name}
                        </Button>
                        {key?.state === 'ready' ? (
                            <Button type='button' tier='tertiary' size='sm' onClick={() => void removeKey()}>
                                Remove key
                            </Button>
                        ) : null}
                    </div>
                </form>
                {keyMessage ? (
                    <p role='alert' className='text-academy-accent-warn text-[13px]'>
                        {keyMessage}
                    </p>
                ) : null}
            </fieldset>
            <Callout tone='default' icon={Info} data-slot='gateway-playground-cost'>
                A run sends your prompt through the Gateway and may incur provider usage and cost. It is sent once and never retried. Stopping ends
                it on this Mac; it does not undo what was already charged.
            </Callout>
            <form
                className='flex flex-col gap-2'
                onBlur={flush}
                onSubmit={(e) => {
                    e.preventDefault()
                    if (!running) void start()
                }}
            >
                <ApiChoice
                    name={`${ids}-api`}
                    label='Inference API'
                    value={api}
                    onChange={(next) => {
                        setApi(next)
                        keep({ api: next })
                    }}
                />
                <label className='flex flex-col text-[13px]'>
                    Model
                    <input
                        value={model}
                        onChange={(e) => {
                            setModel(e.target.value)
                            keep({ model: e.target.value })
                        }}
                        placeholder='@provider/model'
                        spellCheck={false}
                        className='font-mono'
                    />
                </label>
                <label className='flex flex-col text-[13px]'>
                    Route
                    <select
                        value={route}
                        onChange={(e) => {
                            setRoute(e.target.value)
                            keep({ route: e.target.value })
                        }}
                    >
                        <option value=''>No route: send to the model as named</option>
                        {rows.map((r) => (
                            <option key={String(r.id)} value={String(r.id)}>
                                {typeof r.name === 'string' ? r.name : String(r.id)}
                            </option>
                        ))}
                        {route && !rows.some((r) => r.id === route) ? <option value={route}>{route} (not in the routes read)</option> : null}
                    </select>
                </label>
                <label className='flex flex-col text-[13px]'>
                    System instructions (optional)
                    <textarea
                        value={system}
                        rows={2}
                        onChange={(e) => {
                            setSystem(e.target.value)
                            keep({ system: e.target.value })
                        }}
                    />
                </label>
                <label className='flex flex-col text-[13px]'>
                    Prompt
                    <textarea
                        value={prompt}
                        rows={4}
                        onChange={(e) => {
                            setPrompt(e.target.value)
                            keep({ prompt: e.target.value })
                        }}
                    />
                </label>
                {api === 'responses' ? (
                    <label className='flex flex-col text-[13px]'>
                        Attached file (optional)
                        <select
                            value={file}
                            onChange={(e) => {
                                setFile(e.target.value)
                                keep({ file: e.target.value })
                            }}
                        >
                            <option value=''>No file</option>
                            {(read ?? []).map((f) => (
                                <option key={f.id} value={f.id}>
                                    {`${f.filename} (${f.id})`}
                                </option>
                            ))}
                            {file && !(read ?? []).some((f) => f.id === file) ? <option value={file}>{`${file} (not in the files read)`}</option> : null}
                        </select>
                        {read === null ? (
                            <span className='text-academy-ink-faint text-[12px]'>Read this workspace's files in the Files tab to attach one.</span>
                        ) : null}
                    </label>
                ) : null}
                <p className='text-academy-ink-faint text-[12px]'>
                    The form is kept on this Mac as a draft for {workspace.name}. At most 256 output tokens are asked for. A Responses run asks the
                    Gateway not to store the response.
                </p>
                <div className='flex gap-2'>
                    <Button type='submit' tier='primary' size='sm' disabled={!keyHere || running || model.trim().length === 0 || prompt.trim().length === 0}>
                        Run prompt
                    </Button>
                    {running && run ? (
                        <Button type='button' tier='secondary' size='sm' onClick={() => void dispatch({ kind: 'stopInference', operation: run.operation })}>
                            Stop run
                        </Button>
                    ) : null}
                    {run && !running ? (
                        <Button type='button' tier='tertiary' size='sm' onClick={() => void dispatch({ kind: 'clearInference', operation: run.operation })}>
                            Clear response
                        </Button>
                    ) : null}
                </div>
                {message ? (
                    <p role='alert' className='text-academy-accent-warn text-[13px]'>
                        {message}
                    </p>
                ) : null}
            </form>
            {run ? (
                <section aria-labelledby={`${ids}-r`} className='flex flex-col gap-2' data-slot='gateway-playground-run'>
                    <h4 id={`${ids}-r`} className='text-[14px] font-semibold'>
                        Response
                    </h4>
                    <p ref={statusRef} tabIndex={-1} role='status' aria-live='polite' className='text-[13px]' data-slot='gateway-playground-state'>
                        {runText(run)}
                    </p>
                    <pre className='bg-academy-surface max-h-96 overflow-auto rounded p-2 text-[13px] whitespace-pre-wrap' data-slot='gateway-playground-output'>
                        {run.output}
                    </pre>
                    <dl className='grid grid-cols-[auto_1fr] gap-x-3 text-[12px]'>
                        <dt>API</dt>
                        <dd>{API_TEXT[run.api]}</dd>
                        <dt>Model</dt>
                        <dd className='font-mono'>{run.servedModel ?? run.model}</dd>
                        <dt>Route</dt>
                        <dd className='font-mono'>{run.route ?? 'None'}</dd>
                        {run.file ? (
                            <>
                                <dt>Attached file</dt>
                                <dd className='font-mono'>{run.file}</dd>
                            </>
                        ) : null}
                        {run.state !== 'running' ? (
                            <>
                                <dt>How it ended</dt>
                                <dd data-slot='gateway-playground-finish'>
                                    {run.finish ? `${run.finish}${run.state === 'incomplete' ? ' (incomplete)' : ''}` : 'Not reported'}
                                </dd>
                            </>
                        ) : null}
                        <dt>Trace</dt>
                        <dd className='flex items-center gap-2 font-mono'>
                            {run.trace}
                            <Button type='button' tier='tertiary' size='sm' onClick={() => onLogs(run.trace)}>
                                Find in logs
                            </Button>
                        </dd>
                        {run.usage ? (
                            <>
                                <dt>Tokens</dt>
                                <dd>
                                    {`${run.usage.total ?? 'Not reported'}${
                                        run.usage.prompt !== null && run.usage.completion !== null ? ` (${run.usage.prompt} in, ${run.usage.completion} out)` : ''
                                    }`}
                                </dd>
                            </>
                        ) : null}
                    </dl>
                </section>
            ) : null}
            {!keyHere && key?.state === 'ready' ? (
                <Callout tone='default' icon={AlertTriangle}>
                    Runs from {workspace.name} need a key stored for it.
                </Callout>
            ) : null}
        </section>
    )
}

/**
 * Edits a Playground draft kept on this Mac (C18, T23), by its ID: it needs
 * no workspace listing, so it works offline. Nothing is sent from here; a
 * run needs the workspace chosen and its key.
 */
export function KeptDraftEditor({ draft, context, dispatch, onDone }: { draft: PlaygroundDraft; context: ContextRef; dispatch: Dispatch; onDone: () => void }) {
    const ids = useId()
    const [fields, setFields] = useState<Fields>({
        api: draft.api,
        model: draft.model,
        route: draft.route ?? '',
        system: draft.system ?? '',
        prompt: draft.prompt,
        file: draft.file ?? '',
    })
    const [message, setMessage] = useState<string | null>(null)
    const { keep, flush } = useKeeper((d) =>
        void dispatch({ kind: 'savePlaygroundDraft', context, draft: d }).then((outcome) =>
            setMessage(outcome.kind === 'admitted' ? null : (outcomeMessage(outcome) ?? 'The draft was not kept. A route or file ID is letters, digits, “_” or “-”.'))
        )
    )
    const firstRef = useRef<HTMLInputElement>(null)
    useEffect(() => firstRef.current?.focus(), [])

    function change(next: Partial<Fields>) {
        const merged = { ...fields, ...next }
        setFields(merged)
        keep(draftOf(merged, draft.id))
    }

    return (
        <form
            className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-3'
            aria-labelledby={`${ids}-h`}
            data-slot='gateway-kept-playground-editor'
            onBlur={flush}
            onSubmit={(e) => {
                e.preventDefault()
                flush()
                onDone()
            }}
        >
            <h4 id={`${ids}-h`} className='text-[14px] font-semibold'>
                {`Playground draft for ${draft.workspace.name}`}
            </h4>
            <ApiChoice name={`${ids}-api`} label='Draft inference API' value={fields.api} onChange={(api) => change({ api })} />
            <label className='flex flex-col text-[13px]'>
                Draft model
                <input ref={firstRef} value={fields.model} onChange={(e) => change({ model: e.target.value })} spellCheck={false} className='font-mono' />
            </label>
            <label className='flex flex-col text-[13px]'>
                Draft route ID (optional)
                <input value={fields.route} onChange={(e) => change({ route: e.target.value.trim() })} spellCheck={false} className='font-mono' />
            </label>
            <label className='flex flex-col text-[13px]'>
                Draft system instructions (optional)
                <textarea value={fields.system} rows={2} onChange={(e) => change({ system: e.target.value })} />
            </label>
            <label className='flex flex-col text-[13px]'>
                Draft prompt
                <textarea value={fields.prompt} rows={4} onChange={(e) => change({ prompt: e.target.value })} />
            </label>
            <label className='flex flex-col text-[13px]'>
                Draft attached file ID (optional, Responses)
                <input value={fields.file} onChange={(e) => change({ file: e.target.value.trim() })} spellCheck={false} className='font-mono' />
            </label>
            <p className='text-academy-ink-faint text-[12px]'>{`Kept on this Mac as you type. To run it, choose ${draft.workspace.name} in the Gateway and open the Playground.`}</p>
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
            <Button type='submit' tier='secondary' size='sm' className='self-start'>
                Done editing
            </Button>
        </form>
    )
}
