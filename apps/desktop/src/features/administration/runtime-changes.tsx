import { type ReactNode, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout, Input, cn } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, KeyRound } from 'lucide-react'
import type {
    ChangeAction,
    ChangeDraft,
    ChangeFailure,
    ChangeKind,
    ChangeProblem,
    ChangeResultView,
    ChangeReviewView,
    ContextRef,
    Provisioning,
    ScopeCleanup,
    StagedFileFailure,
    StagedFileView,
    StepState,
    WorkspaceRef,
} from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { MANAGEMENT_FAILURE_TEXT } from './management-read'

/** What the operator chose to change (body is the editor's initial text). */
export interface ChangeTarget {
    kind: ChangeKind
    action: ChangeAction
    target: string
    title: string
    body: string
    /** A Gateway change inside the chosen workspace: its native reference. */
    workspace?: WorkspaceRef
    /** A Gateway draft kept on this Mac that this change edits (C17). */
    draftId?: string
    /** Why it cannot be reviewed now (the draft's workspace is not chosen). */
    reviewBlocked?: string
}

const PROBLEM_TEXT: Record<ChangeProblem, string> = {
    unsupported: 'This change is not supported.',
    target: 'The item to change is missing or invalid.',
    body: 'The draft must be a single JSON object.',
    contract: 'The draft does not match what the service accepts. Check the field names and values.',
    description: 'A custom topic description can have at most 240 characters.',
    rotation: 'Key rotation must be 30, 90 or 180 days.',
    keyFields: 'Enter the key name, application, environment, cloud provider and deployment profile.',
    applicationExists: 'An application with that name already exists. Add a key version to it instead.',
    keyExists: 'An API key with that name already exists.',
    inventoryLimit: 'The inventory is too large to check completely, so the change was not prepared.',
    deploymentProfile: 'The deployment profile was not found exactly once.',
    sourceMismatch: 'The key version must use the existing key’s application.',
    notFound: 'The item is no longer in the service’s inventory.',
    dataProfile:
        'The data profile is incomplete: check the name (at most 32 characters), its rules, techniques, confidence levels and occurrence bounds.',
    dictionaryFile: 'The keyword list must be non-empty text of at most 65,536 characters.',
    fileName: 'The original file name cannot contain slashes, quotes or control characters (at most 255 characters).',
    stagedFile: 'The chosen file is no longer available. Choose it again.',
    previewOnly: 'Directional settings and customized prompt-injection modes were local previews in the previous desktop; remove them from the draft. They are never sent.',
    credentialPolicy:
        'Credentials are never part of a draft: remove keys, secret mappings and credential-bearing settings. They are entered only where the desktop protects them.',
    protectedConfiguration:
        'This item holds credentials the change would replace or lose, so it cannot be changed here. Change its other fields, or its credentials where they are protected.',
    workspaceMismatch: 'The draft names a different workspace. Changes apply only to the workspace chosen here.',
    configuration: 'The settings do not meet the provider family’s or the guardrail catalog’s rules.',
    scopeExists: 'That IAM scope already exists. Choose to reuse it, or name a new one.',
    scopeMissing: 'That IAM scope does not exist, so it cannot be reused.',
    referenced: 'Other configuration still references this item, so it was not changed.',
}

const STAGED_TEXT: Record<StagedFileFailure, string> = {
    tooLarge: 'That file is larger than 65,536 characters.',
    notText: 'That file is not UTF-8 text.',
    empty: 'That file has no keywords.',
    unreadable: 'That file could not be read.',
}

export function changeFailureText(failure: ChangeFailure): string {
    switch (failure.kind) {
        case 'problem':
            return PROBLEM_TEXT[failure.problem]
        case 'conflict':
            return 'The item changed since it was reviewed. Review the change again.'
        case 'service':
            // A change is stopped by the operator or a tenant change, never
            // "read".
            return failure.failure === 'cancelled'
                ? 'The desktop stopped waiting for this change.'
                : MANAGEMENT_FAILURE_TEXT[failure.failure]
        case 'expired':
            return 'This review expired before it was applied. Review the change again.'
        case 'capacity':
            return 'The desktop could not start the change right now, so nothing was sent. Review it again.'
        case 'workspaceChanged':
            return 'Another workspace was chosen before this change was sent, so nothing was sent. Review it again in the workspace it is for.'
    }
}

export const CHANGE_NOUN: Record<ChangeKind, string> = {
    profiles: 'security profile',
    topics: 'custom topic',
    applications: 'application',
    apiKeys: 'API key',
    dataPatterns: 'data pattern',
    dataProfiles: 'data profile',
    filteringProfiles: 'data filtering profile',
    dictionaries: 'dictionary',
    gatewayWorkspaces: 'workspace',
    gatewayProviders: 'provider',
    gatewayRoutes: 'route',
    gatewayGuardrails: 'guardrail',
    gatewayServiceKeys: 'service key',
    gatewayUserKeys: 'user key',
    gatewayIntegrations: 'integration',
}

/** How a change's action reads in a result line. */
export function verbOf(kind: ChangeKind, action: ChangeAction): string {
    if (kind === 'gatewayWorkspaces' && action === 'delete') return 'Archive'
    if (kind === 'gatewayWorkspaces' && action === 'create') return 'Provision'
    if (action === 'models') return 'Change models of'
    return action.charAt(0).toUpperCase() + action.slice(1)
}

export const isGatewayKind = (kind: ChangeKind) => kind.startsWith('gateway')

const TEXTAREA = cn(
    'rounded-academy-md bg-academy-bg-inset border-academy-rule focus-visible:border-academy-accent-ring w-full border px-3.5 py-2.5',
    'text-academy-ink min-h-48 resize-y font-mono text-[12px] leading-snug'
)

function Json({ label, text }: { label: string; text: string | null }) {
    if (text === null) return null
    let pretty = text
    try {
        pretty = JSON.stringify(JSON.parse(text), null, 2)
    } catch {
        // Shown as received.
    }
    return (
        <div className='flex min-w-0 flex-1 flex-col gap-1'>
            <span className='text-academy-ink-soft text-[12px] font-medium'>{label}</span>
            <pre className='bg-academy-bg-inset rounded-academy-md max-h-72 overflow-auto p-3 font-mono text-[11px] break-all whitespace-pre-wrap'>
                {pretty}
            </pre>
        </div>
    )
}

interface WorkbenchProps {
    change: ChangeTarget
    review: ChangeReviewView | null
    /** The dictionary keyword file chosen in the native sheet, if any. */
    staged?: StagedFileView | null
    context: ContextRef
    dispatch: Dispatch
    onClose: () => void
    /** Keeps the edited text as a local draft (C17); called after edits pause. */
    onDraft?: (body: string) => void
    /** Shown under the editor (where the draft is kept). */
    note?: ReactNode
    /** v2 (C31, I3): Apply stays disabled until the typed name matches the review's. */
    requireTyped?: boolean
}

/** How long typing pauses before a draft is kept. */
const DRAFT_PAUSE_MS = 800

/** Splits a dictionary body into its metadata JSON and keyword text. */
function dictionaryParts(body: string): { metadata: string; keywords: string } {
    try {
        const parsed = JSON.parse(body) as { metadata?: unknown; file?: unknown }
        return { metadata: JSON.stringify(parsed.metadata ?? {}, null, 2), keywords: typeof parsed.file === 'string' ? parsed.file : '' }
    } catch {
        return { metadata: body, keywords: '' }
    }
}

/**
 * Edit, review and apply one Runtime change. The editor is uncontrolled:
 * Review and Apply read its current text at the moment they are pressed
 * (no debounced copy), and Rust compares Apply's text with the reviewed
 * draft. While the native confirmation is open the editor is read-only and
 * the main window is held by the sheet.
 */
export function ChangeWorkbench({ change, review, staged = null, context, dispatch, onClose, onDraft, note, requireTyped = false }: WorkbenchProps) {
    const ids = useId()
    const [typedText, setTypedText] = useState('')
    const heading = useRef<HTMLHeadingElement>(null)
    const editor = useRef<HTMLTextAreaElement>(null)
    const keywords = useRef<HTMLTextAreaElement>(null)
    const [useFile, setUseFile] = useState(false)
    const dictionary = change.kind === 'dictionaries'
    const parts = dictionary ? dictionaryParts(change.body) : null
    const typed = useRef<HTMLInputElement>(null)
    const [message, setMessage] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const mine =
        review && review.kind === change.kind && review.action === change.action && review.target === change.target
            ? review
            : null
    const confirming = mine?.state === 'confirming'
    const blocked = (mine?.references.length ?? 0) > 0
    const deletes = change.action === 'delete'

    // The chosen file is used only once one is actually staged; a cancelled
    // or failed choice leaves the typed keywords in use.
    const fileReady = staged?.state === 'ready' && !!staged.token
    const usingFile = useFile && fileReady
    // Opening a change moves focus to it.
    useEffect(() => {
        heading.current?.focus()
    }, [])
    // Edited text is kept as a draft once typing pauses, and on close.
    const pending = useRef<ReturnType<typeof setTimeout> | null>(null)
    const kept = useRef(change.body)
    function keepDraft() {
        if (pending.current) clearTimeout(pending.current)
        pending.current = null
        const text = editor.current?.value
        if (!onDraft || text === undefined || text === kept.current) return
        kept.current = text
        onDraft(text)
    }
    function edited() {
        if (!onDraft) return
        if (pending.current) clearTimeout(pending.current)
        pending.current = setTimeout(keepDraft, DRAFT_PAUSE_MS)
    }
    useEffect(() => () => keepDraft(), [])
    const [awaitingFile, setAwaitingFile] = useState(false)
    useEffect(() => {
        if (!awaitingFile || staged?.state === 'choosing') return
        setAwaitingFile(false)
        setUseFile(staged?.state === 'ready')
    }, [awaitingFile, staged?.state])

    /** The draft as entered now, or null (with a message) when it cannot be sent. */
    function draft(): ChangeDraft | null {
        const text = editor.current?.value ?? ''
        let body = deletes ? '{}' : text
        if (dictionary && !deletes) {
            // Read fresh from the fields. The metadata must be exactly one JSON
            // object: it is parsed, never spliced into the body as text.
            let metadata: unknown
            try {
                metadata = JSON.parse(text)
            } catch {
                metadata = null
            }
            if (typeof metadata !== 'object' || metadata === null || Array.isArray(metadata)) {
                setMessage('The dictionary metadata must be one JSON object.')
                return null
            }
            body = JSON.stringify(
                usingFile && staged?.token
                    ? { metadata, fileRef: staged.token }
                    : { metadata, file: keywords.current?.value ?? '' }
            )
        }
        const captured: ChangeDraft = { kind: change.kind, action: change.action, target: change.target, body }
        if (change.workspace) captured.workspace = change.workspace
        return captured
    }

    async function chooseFile() {
        setAwaitingFile(true)
        setMessage(outcomeMessage(await dispatch({ kind: 'chooseDictionaryFile', context })))
    }

    async function prepare() {
        // The reviewed text is kept first, so a later settlement finds it.
        keepDraft()
        setMessage(null)
        const captured = draft()
        if (!captured) return
        setBusy(true)
        const outcome = await dispatch({ kind: 'reviewChange', context, draft: captured })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    async function apply() {
        if (!mine) return
        setMessage(null)
        const captured = draft()
        if (!captured) return
        setBusy(true)
        const outcome = await dispatch({
            kind: 'requestApply',
            review: mine.review,
            draft: captured,
            confirmation: typed.current?.value ?? '',
        })
        setBusy(false)
        if (outcome.kind === 'refused' && outcome.refusal === 'reviewInvalid') {
            setMessage('The draft changed since it was reviewed, or the review expired. Review the change again.')
        } else if (outcome.kind === 'refused' && outcome.refusal === 'invalidInput') {
            setMessage(`Type ${mine.confirmation ?? 'the confirmation'} exactly to confirm.`)
        } else setMessage(outcomeMessage(outcome))
    }

    async function discard() {
        keepDraft()
        if (mine) await dispatch({ kind: 'discardChange', review: mine.review })
        onClose()
    }

    return (
        <section
            aria-labelledby={`${ids}-heading`}
            data-slot='change-workbench'
            className='border-academy-rule flex flex-col gap-4 rounded-[12px] border p-5'
        >
            <h2 id={`${ids}-heading`} ref={heading} tabIndex={-1} className='text-academy-ink text-[15px] font-semibold outline-none'>
                {change.title}
            </h2>
            {deletes ? (
                <p className='text-[13px]'>Review what will be removed before applying.</p>
            ) : (
                <div className='flex flex-col gap-1'>
                    <label htmlFor={`${ids}-editor`} className='text-academy-ink text-[13px] font-medium'>
                        {dictionary ? 'dictionary metadata (JSON)' : `${CHANGE_NOUN[change.kind]} draft (JSON)`}
                    </label>
                    <textarea
                        ref={editor}
                        id={`${ids}-editor`}
                        defaultValue={parts ? parts.metadata : change.body}
                        readOnly={confirming}
                        spellCheck={false}
                        aria-describedby={`${ids}-editor-hint`}
                        className={TEXTAREA}
                        onInput={edited}
                        data-slot='change-editor'
                    />
                    <p id={`${ids}-editor-hint`} className='text-academy-ink-faint text-[12px]'>
                        Authorship is set from the management client. Server-assigned fields are ignored.
                        {change.kind === 'profiles'
                            ? ' Directional settings and customized prompt-injection modes were local previews in the previous desktop; they are not part of a draft and are never sent.'
                            : ''}
                    </p>
                    {note}
                    {parts ? (
                        <fieldset className='mt-2 flex flex-col gap-2' data-slot='dictionary-keywords'>
                            <legend className='text-academy-ink text-[13px] font-medium'>Keywords</legend>
                            <label className='flex min-h-11 items-center gap-2 text-[13px]'>
                                <input
                                    type='radio'
                                    name={`${ids}-source`}
                                    checked={!usingFile}
                                    disabled={confirming}
                                    onChange={() => setUseFile(false)}
                                    className='size-4 accent-[var(--academy-accent)]'
                                />
                                Type them (one per line)
                            </label>
                            <textarea
                                ref={keywords}
                                aria-label='Dictionary keywords, one per line'
                                defaultValue={parts.keywords}
                                readOnly={confirming}
                                disabled={usingFile}
                                spellCheck={false}
                                className={TEXTAREA}
                                data-slot='dictionary-text'
                            />
                            <label className='flex min-h-11 items-center gap-2 text-[13px]'>
                                <input
                                    type='radio'
                                    name={`${ids}-source`}
                                    checked={usingFile}
                                    disabled={confirming || !fileReady}
                                    onChange={() => setUseFile(true)}
                                    className='size-4 accent-[var(--academy-accent)]'
                                />
                                Use the chosen file
                                {staged?.state === 'ready' ? ` (${staged.name}, ${staged.lines} lines)` : ''}
                            </label>
                            <div>
                                <Button type='button' tier='secondary' disabled={busy || confirming} onClick={() => void chooseFile()}>
                                    Choose keyword file…
                                </Button>
                            </div>
                            <p role='status' className='text-academy-ink-faint text-[12px]' data-slot='dictionary-file-status'>
                                {staged?.state === 'choosing'
                                    ? 'Choose a text file in the desktop’s file sheet.'
                                    : staged?.state === 'failed' && staged.failure
                                      ? STAGED_TEXT[staged.failure]
                                      : staged?.state === 'ready'
                                        ? `Chosen: ${staged.name} (${staged.bytes} bytes). The file itself is only read.`
                                        : 'A file is read once by the desktop; it is never changed.'}
                            </p>
                        </fieldset>
                    ) : null}
                </div>
            )}
            {change.reviewBlocked ? (
                <p className='text-[13px]' data-slot='change-review-blocked'>
                    {change.reviewBlocked}
                </p>
            ) : null}
            <div className='flex flex-wrap gap-2'>
                <Button
                    type='button'
                    tier='secondary'
                    disabled={busy || confirming || !!change.reviewBlocked}
                    onClick={() => void prepare()}
                >
                    {mine ? 'Review again' : 'Review change'}
                </Button>
                <Button type='button' tier='tertiary' disabled={busy} onClick={() => void discard()}>
                    Close
                </Button>
            </div>
            {mine ? (
                <div className='flex flex-col gap-3' data-slot='change-review'>
                    {mine.state === 'preparing' ? (
                        <p role='status' className='text-academy-ink-soft text-[13px]'>
                            Reading the current item for review…
                        </p>
                    ) : (mine.state === 'failed' || mine.state === 'expired') && mine.failure ? (
                        <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='change-review-failed'>
                            {changeFailureText(mine.failure)}
                        </Callout>
                    ) : (
                        <>
                            <Callout tone='warning' icon={Info} data-slot='change-impact'>
                                {mine.impact}
                            </Callout>
                            {mine.workspace ? (
                                <p className='text-[13px]' data-slot='change-scope'>
                                    Applies to workspace <strong>{mine.workspace.name}</strong> ({mine.workspace.slug}), IAM scope{' '}
                                    {mine.workspace.scopeName}.
                                </p>
                            ) : null}
                            {blocked ? (
                                <div role='alert' className='flex flex-col gap-1 text-[13px]' data-slot='change-references'>
                                    <p className='font-medium'>
                                        This cannot be applied while other configuration references it:
                                    </p>
                                    <ul className='list-disc pl-5'>
                                        {mine.references.map((r) => (
                                            <li key={r}>{r}</li>
                                        ))}
                                    </ul>
                                </div>
                            ) : null}
                            <div className='flex flex-wrap gap-3'>
                                <Json label='Existing key' text={mine.sourceKey} />
                                <Json label='Current' text={mine.before} />
                                <Json label='After applying' text={mine.after} />
                            </div>
                            {mine.after === null && change.action === 'delete' ? (
                                <p className='text-[13px]'>After applying, the item is removed.</p>
                            ) : null}
                            <label htmlFor={`${ids}-confirm`} className='text-academy-ink text-[13px] font-medium'>
                                Type {mine.confirmation ?? 'the confirmation'} to confirm
                            </label>
                            <Input
                                ref={typed}
                                id={`${ids}-confirm`}
                                autoComplete='off'
                                spellCheck={false}
                                readOnly={confirming}
                                data-slot='change-confirmation'
                                onChange={(e) => setTypedText(e.currentTarget.value)}
                            />
                            <div>
                                <Button
                                    type='button'
                                    tier='primary'
                                    disabled={busy || confirming || blocked || (requireTyped && typedText !== (mine.confirmation ?? ''))}
                                    onClick={() => void apply()}
                                >
                                    Apply…
                                </Button>
                            </div>
                            {confirming ? (
                                <p role='status' className='text-academy-ink-soft text-[13px]' data-slot='change-confirming'>
                                    Confirm or cancel in the desktop’s dialog. The draft is held until you do.
                                </p>
                            ) : null}
                        </>
                    )}
                </div>
            ) : null}
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]' data-slot='change-message'>
                    {message}
                </p>
            ) : null}
        </section>
    )
}

export function resultText(result: ChangeResultView): string {
    if (!result.settled) return 'Applying…'
    // Each provisioning step keeps its own outcome; the steps say what is known.
    if (result.provisioning)
        return `${result.failure ? `${changeFailureText(result.failure)} ` : ''}${provisioningText(result.provisioning)}`
    if (result.failure) {
        const why = changeFailureText(result.failure)
        switch (result.outcome) {
            case 'unknown':
                return `${why} The change may or may not have been applied. Refresh the inventory before trying again.`
            case 'completed':
                return `${why} The service accepted the change.`
            default:
                return `${why} Nothing was changed.`
        }
    }
    switch (result.readback) {
        case 'differs':
            return result.action === 'delete'
                ? 'Applied, but the item is still listed when read back. Refresh before trying again.'
                : 'Applied, but reading it back does not show the reviewed change. Refresh before trying again.'
        case 'found':
            return 'Applied. The item was read back.'
        case 'absent':
            return result.action === 'delete'
                ? 'Applied. The item is no longer in the inventory.'
                : 'Applied, but the item was not found when read back. Refresh the inventory.'
        case 'incomplete':
            return 'Applied. The inventory was too large to read the item back.'
        case 'stopped':
            return 'Applied: the service accepted the change. The desktop stopped waiting before reading it back; refresh the inventory to see the current state.'
        default:
            return 'Applied. Reading it back failed; refresh the inventory to see the current state.'
    }
}

const STEP_TEXT: Record<StepState, string> = {
    notStarted: 'not attempted',
    notSent: 'not sent',
    rejected: 'refused by the service',
    unknown: 'outcome unknown',
    completed: 'done',
    existing: 'existing scope reused',
    deleted: 'created, then deleted again',
}

/** What became of an IAM scope this attempt created (empty when none). */
const CLEANUP_TEXT: Record<ScopeCleanup, string> = {
    notNeeded: '',
    retained: 'The IAM scope was kept because the workspace exists or may exist. ',
    deleted: 'The IAM scope this attempt created was deleted again. ',
    failed: 'Deleting the IAM scope this attempt created was refused, so it remains. ',
    unknown: 'Whether the IAM scope this attempt created was deleted is not known. ',
}

/** What reading the tenant back did not establish. */
function unconfirmed(p: Provisioning): string[] {
    const missing: string[] = []
    if (p.workspaceRead === 'ambiguous') missing.push('which workspace is this attempt’s')
    else if (p.workspaceRead !== 'found') missing.push('the workspace')
    if (p.scopeRead !== 'found') missing.push('the IAM scope')
    if (p.workspaceRead === 'found' && p.scopeRead === 'found' && p.boundRead !== true) missing.push('the scope’s binding to the workspace')
    return missing
}

/**
 * A provisioning's summary: each step's own outcome, what the SDK removed
 * again, and only what reading the tenant back established is called
 * verified.
 */
export function provisioningText(p: Provisioning): string {
    const steps = [p.scope, p.workspace, p.binding]
    const cleanup = CLEANUP_TEXT[p.cleanup]
    if (steps.includes('unknown')) return `Not every step’s outcome is known. ${cleanup}Check what was read back below before trying again.`
    const written = (p.scope === 'completed' || p.scope === 'existing') && p.workspace === 'completed' && p.binding === 'completed'
    if (written) {
        const missing = unconfirmed(p)
        return missing.length === 0
            ? 'Provisioned. The workspace, its IAM scope and the scope’s binding to it were read back.'
            : `Every step was accepted, but reading the tenant back did not confirm ${missing.join(', ')}. Refresh the workspaces before trying again.`
    }
    if (p.workspace === 'completed') return `Partially completed: the workspace exists but a later step failed. ${cleanup}Do not create it again.`
    return `Not provisioned. ${cleanup || (p.scope === 'completed' ? 'The IAM scope this attempt created remains. ' : 'Nothing was created. ')}`.trim()
}

/** Each provisioning step and what reading the tenant back found. */
export function ProvisioningSteps({ p }: { p: Provisioning }) {
    const read = (r: Provisioning['scopeRead']) =>
        r === 'found'
            ? 'found'
            : r === 'absent'
              ? 'not found'
              : r === 'incomplete'
                ? 'not established (inventory too large)'
                : r === 'ambiguous'
                  ? 'not identified (more than one could be this attempt’s)'
                  : 'could not be read'
    return (
        <dl className='grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]' data-slot='provisioning-steps'>
            <dt className='font-medium'>IAM scope {p.scopeName}</dt>
            <dd>
                {STEP_TEXT[p.scope]}
                {p.cleanup === 'notNeeded' ? '' : ` · ${CLEANUP_TEXT[p.cleanup].trim()}`}
            </dd>
            <dt className='font-medium'>Workspace</dt>
            <dd>{STEP_TEXT[p.workspace]}</dd>
            <dt className='font-medium'>Scope binding</dt>
            <dd>{STEP_TEXT[p.binding]}</dd>
            <dt className='font-medium'>Read back</dt>
            <dd>
                Scope {read(p.scopeRead)}; workspace {read(p.workspaceRead)}
                {p.workspaceSlug ? ` (${p.workspaceSlug})` : ''}
                {p.boundRead === null ? '' : p.boundRead ? '; the scope lists it' : '; the scope does not list it'}.
            </dd>
        </dl>
    )
}

/** The active tenant's applied changes and their one-time secrets. */
export function ChangeResults({
    results,
    dispatch,
    heading = 'Applied changes',
}: {
    results: ChangeResultView[]
    dispatch: Dispatch
    heading?: string
}) {
    const ids = useId()
    const [message, setMessage] = useState<string | null>(null)
    if (results.length === 0) return null

    async function send(intent: Parameters<Dispatch>[0]) {
        setMessage(outcomeMessage(await dispatch(intent)))
    }

    return (
        <section aria-labelledby={`${ids}-heading`} data-slot='change-results' className='flex flex-col gap-3'>
            <h2 id={`${ids}-heading`} className='text-academy-ink text-[15px] font-semibold'>
                {heading}
            </h2>
            <ul className='flex flex-col gap-3'>
                {results.map((result) => (
                    <li key={result.operation} className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-4' data-slot='change-result'>
                        <p className='text-academy-ink text-[13px] font-medium'>
                            {verbOf(result.kind, result.action)} {CHANGE_NOUN[result.kind]} {result.name ?? result.target}
                            {result.workspace ? ` in ${result.workspace.name}` : ''}
                        </p>
                        <p role='status' className='text-[13px]'>
                            {resultText(result)}
                        </p>
                        {result.provisioning ? <ProvisioningSteps p={result.provisioning} /> : null}
                        {!result.settled ? (
                            <div>
                                <Button type='button' tier='tertiary' onClick={() => void send({ kind: 'cancelChange', operation: result.operation })}>
                                    Stop waiting
                                </Button>
                                <p className='text-academy-ink-faint text-[12px]'>
                                    Stopping does not undo anything already sent; the result says what is known.
                                </p>
                            </div>
                        ) : null}
                        {result.secret === 'held' ? (
                            <Callout tone='warning' icon={KeyRound} data-slot='change-secret'>
                                <p>
                                    {isGatewayKind(result.kind)
                                        ? 'A new Gateway key is held in memory only. Show it once in the desktop’s dialog and store it securely.'
                                        : 'A new API key is held in memory only. Show it once in the desktop’s dialog and store it securely. The configured scan key is unchanged.'}
                                </p>
                                <div className='mt-2 flex flex-wrap gap-2'>
                                    <Button type='button' tier='secondary' onClick={() => void send({ kind: 'revealSecret', operation: result.operation })}>
                                        Show new key…
                                    </Button>
                                    <Button type='button' tier='tertiary' onClick={() => void send({ kind: 'forgetSecret', operation: result.operation })}>
                                        Clear key
                                    </Button>
                                </div>
                            </Callout>
                        ) : result.secret === 'forgotten' ? (
                            <p className='text-academy-ink-faint text-[12px]'>The new key was cleared from memory.</p>
                        ) : result.secret === 'shown' ? (
                            <p className='text-academy-ink-faint text-[12px]'>
                                The new key was shown once and is no longer held by the desktop.
                            </p>
                        ) : null}
                    </li>
                ))}
            </ul>
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </section>
    )
}

/** Editor templates (v1 field names). */
export const TEMPLATES = {
    profile: { profile_name: '', active: true, policy: { 'ai-security-profiles': [] } },
    topic: { topic_name: '', description: '', examples: [] as string[] },
    key: {
        api_key_name: '',
        cust_app: '',
        cust_env: 'production',
        cust_cloud_provider: 'aws',
        dp_name: '',
        revoked: false,
        rotation_time_interval: 30,
        rotation_time_unit: 'days',
    },
    rotate: { rotation_time_interval: 30, rotation_time_unit: 'days' },
    pattern: { name: '', type: 'custom', description: '', detection_config: { technique: 'regex' } },
    dataProfile: { name: '', detection_rules: [] as unknown[] },
    dictionary: {
        metadata: {
            name: 'New dictionary',
            original_file_name: 'keywords.txt',
            type: 'custom',
            category: 'Confidential',
            region_name: 'United States',
        },
        file: 'internal\nconfidential\n',
    },
}

export const pretty = (value: unknown) => JSON.stringify(value, null, 2)
