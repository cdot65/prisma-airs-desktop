import { useId, useRef, useState } from 'react'
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
    StagedFileFailure,
    StagedFileView,
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
            return MANAGEMENT_FAILURE_TEXT[failure.failure]
        case 'expired':
            return 'This review expired before it was applied. Review the change again.'
        case 'capacity':
            return 'The desktop could not start the change right now, so nothing was sent. Review it again.'
    }
}

const NOUN: Record<ChangeKind, string> = {
    profiles: 'security profile',
    topics: 'custom topic',
    applications: 'application',
    apiKeys: 'API key',
    dataPatterns: 'data pattern',
    dataProfiles: 'data profile',
    filteringProfiles: 'data filtering profile',
    dictionaries: 'dictionary',
}

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
}

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
export function ChangeWorkbench({ change, review, staged = null, context, dispatch, onClose }: WorkbenchProps) {
    const ids = useId()
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
    const deletes = change.action === 'delete'

    function draft(): ChangeDraft {
        const text = editor.current?.value ?? ''
        let body = deletes ? '{}' : text
        if (dictionary && !deletes) {
            // Read fresh from the fields; the metadata text is passed as written.
            const file =
                useFile && staged?.state === 'ready' && staged.token
                    ? `"fileRef":${JSON.stringify(staged.token)}`
                    : `"file":${JSON.stringify(keywords.current?.value ?? '')}`
            body = `{"metadata":${text},${file}}`
        }
        return { kind: change.kind, action: change.action, target: change.target, body }
    }

    async function chooseFile() {
        setMessage(outcomeMessage(await dispatch({ kind: 'chooseDictionaryFile', context })))
        setUseFile(true)
    }

    async function prepare() {
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch({ kind: 'reviewChange', context, draft: draft() })
        setBusy(false)
        setMessage(outcomeMessage(outcome))
    }

    async function apply() {
        if (!mine) return
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch({
            kind: 'requestApply',
            review: mine.review,
            draft: draft(),
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
        if (mine) await dispatch({ kind: 'discardChange', review: mine.review })
        onClose()
    }

    return (
        <section
            aria-labelledby={`${ids}-heading`}
            data-slot='change-workbench'
            className='border-academy-rule flex flex-col gap-4 rounded-[12px] border p-5'
        >
            <h2 id={`${ids}-heading`} className='text-academy-ink text-[15px] font-semibold'>
                {change.title}
            </h2>
            {deletes ? (
                <p className='text-[13px]'>Review what will be removed before applying.</p>
            ) : (
                <div className='flex flex-col gap-1'>
                    <label htmlFor={`${ids}-editor`} className='text-academy-ink text-[13px] font-medium'>
                        {dictionary ? 'dictionary metadata (JSON)' : `${NOUN[change.kind]} draft (JSON)`}
                    </label>
                    <textarea
                        ref={editor}
                        id={`${ids}-editor`}
                        defaultValue={parts ? parts.metadata : change.body}
                        readOnly={confirming}
                        spellCheck={false}
                        aria-describedby={`${ids}-editor-hint`}
                        className={TEXTAREA}
                        data-slot='change-editor'
                    />
                    <p id={`${ids}-editor-hint`} className='text-academy-ink-faint text-[12px]'>
                        Authorship is set from the management client. Server-assigned fields are ignored.
                        {change.kind === 'profiles'
                            ? ' Directional settings and customized prompt-injection modes were local previews in the previous desktop; they are not part of a draft and are never sent.'
                            : ''}
                    </p>
                    {parts ? (
                        <fieldset className='mt-2 flex flex-col gap-2' data-slot='dictionary-keywords'>
                            <legend className='text-academy-ink text-[13px] font-medium'>Keywords</legend>
                            <label className='flex min-h-11 items-center gap-2 text-[13px]'>
                                <input
                                    type='radio'
                                    name={`${ids}-source`}
                                    checked={!useFile}
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
                                disabled={useFile}
                                spellCheck={false}
                                className={TEXTAREA}
                                data-slot='dictionary-text'
                            />
                            <label className='flex min-h-11 items-center gap-2 text-[13px]'>
                                <input
                                    type='radio'
                                    name={`${ids}-source`}
                                    checked={useFile}
                                    disabled={confirming || staged?.state !== 'ready'}
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
            <div className='flex flex-wrap gap-2'>
                <Button type='button' tier='secondary' disabled={busy || confirming} onClick={() => void prepare()}>
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
                            />
                            <div>
                                <Button
                                    type='button'
                                    tier='primary'
                                    disabled={busy || confirming}
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

function resultText(result: ChangeResultView): string {
    if (!result.settled) return 'Applying…'
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
        case 'found':
            return 'Applied. The item was read back.'
        case 'absent':
            return result.action === 'delete'
                ? 'Applied. The item is no longer in the inventory.'
                : 'Applied, but the item was not found when read back. Refresh the inventory.'
        case 'incomplete':
            return 'Applied. The inventory was too large to read the item back.'
        default:
            return 'Applied. Reading it back failed; refresh the inventory to see the current state.'
    }
}

/** The active tenant's applied changes and their one-time secrets. */
export function ChangeResults({ results, dispatch }: { results: ChangeResultView[]; dispatch: Dispatch }) {
    const ids = useId()
    const [message, setMessage] = useState<string | null>(null)
    if (results.length === 0) return null

    async function send(intent: Parameters<Dispatch>[0]) {
        setMessage(outcomeMessage(await dispatch(intent)))
    }

    return (
        <section aria-labelledby={`${ids}-heading`} data-slot='change-results' className='flex flex-col gap-3'>
            <h2 id={`${ids}-heading`} className='text-academy-ink text-[15px] font-semibold'>
                Applied changes
            </h2>
            <ul className='flex flex-col gap-3'>
                {results.map((result) => (
                    <li key={result.operation} className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-4' data-slot='change-result'>
                        <p className='text-academy-ink text-[13px] font-medium'>
                            {result.action} {NOUN[result.kind]} {result.name ?? result.target}
                        </p>
                        <p role='status' className='text-[13px]'>
                            {resultText(result)}
                        </p>
                        {result.secret === 'held' ? (
                            <Callout tone='warning' icon={KeyRound} data-slot='change-secret'>
                                <p>
                                    A new API key is held in memory only. Show it once in the desktop’s dialog and store it
                                    securely. The configured scan key is unchanged.
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
