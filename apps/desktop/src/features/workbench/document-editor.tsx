import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout, Input, cn } from '@ai-security-academy/presentation'
import { AlertTriangle, History, Save } from 'lucide-react'
import type { DocumentView, Fields, Preview } from '@prisma-airs/desktop-contracts'
import { type WorkbenchCall, fieldText, resultText, saveRefusalText } from './workbench-call'

/** File > Save (Cmd+S) arrives from the native menu as this window event. */
export const SAVE_DOCUMENT_EVENT = 'airs-save-document'

/** How long typing pauses before the snapshot is protected on this Mac. */
export const PROTECT_DELAY_MS = 400
export const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const BODY_KINDS = ['text', 'json', 'form', 'multipart', 'file']

export const TEXTAREA = cn(
    'rounded-academy-md bg-academy-bg-inset border-academy-rule focus-visible:border-academy-accent-ring w-full border px-3.5 py-2.5',
    'text-academy-ink min-h-24 resize-y font-mono text-[13px] leading-snug'
)
export const SELECT = 'border-academy-rule rounded-academy-sm bg-academy-bg-inset min-h-11 border px-2'

const same = (a: Fields, b: Fields) =>
    a.method === b.method &&
    a.url === b.url &&
    a.headers === b.headers &&
    a.query === b.query &&
    a.body === b.body &&
    a.bodyKind === b.bodyKind &&
    a.settings === b.settings

export const requestName = (relative: string) => relative.split('/').pop()?.replace(/\.request\.toml$/, '') ?? relative

interface Props {
    initial: DocumentView
    call: WorkbenchCall
    schemas: string[]
    /** The document was closed by a choice made here. */
    onClosed: (id: string) => void
    /** Hands the workbench this document's close handle. */
    register?: (handle: EditorHandle) => void
}

export interface EditorHandle {
    /**
     * Protects any edit not yet acknowledged; resolves whether everything
     * typed is now kept on this Mac. Closing a tab or the collection waits
     * for it.
     */
    flush: () => Promise<boolean>
    /** The operator chose to close without the latest edits. */
    abandon: () => void
}

/**
 * One request document. The editor text lives here (React owns immediate
 * input); Rust owns the baseline and revision. Save captures the fields the
 * editor holds at that moment, not a debounced copy; typing pauses protect
 * the snapshot on this Mac; a reply never overwrites newer typing.
 */
export function DocumentEditor({ initial, call, schemas, onClosed, register }: Props) {
    const ids = useId()
    const [doc, setDoc] = useState(initial)
    const [fields, setFields] = useState(initial.fields)
    const fieldsRef = useRef(initial.fields)
    const revisionRef = useRef(initial.revision)
    const idRef = useRef(initial.id)
    const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
    // Edit generations: every edit takes the next; Rust clears its
    // unprotected mark only for a snapshot covering its newest reported one.
    const editGen = useRef(0)
    const markedGen = useRef(0)
    const sentGen = useRef(0)
    const ackGen = useRef(0)
    const [status, setStatus] = useState('')
    const [saving, setSaving] = useState(false)
    const [preview, setPreview] = useState<Preview | null>(null)
    const [conflict, setConflict] = useState<'changed' | 'deleted' | 'interrupted' | null>(
        initial.interrupted ? 'interrupted' : initial.conflict
    )

    const adopt = useCallback((next: DocumentView, editor: Fields | null) => {
        setDoc(next)
        revisionRef.current = next.revision
        setConflict(next.interrupted ? 'interrupted' : next.conflict)
        if (editor !== null) {
            fieldsRef.current = editor
            setFields(editor)
        }
    }, [])

    const protect = useCallback((): Promise<boolean> => {
        timer.current = null
        const edit = editGen.current
        sentGen.current = Math.max(sentGen.current, edit)
        return call({ kind: 'protect', id: idRef.current, revision: revisionRef.current, fields: fieldsRef.current, edit }).then((r) => {
            if (r.kind === 'protected') {
                ackGen.current = Math.max(ackGen.current, edit)
                return true
            }
            setStatus(resultText(r) ?? '')
            return false
        })
    }, [call])

    // Protects whatever is not yet acknowledged (a pending pause, or a
    // protection that failed); resolves whether all typing is kept.
    const flush = useCallback((): Promise<boolean> => {
        const waiting = timer.current !== null
        if (timer.current !== null) clearTimeout(timer.current)
        if (waiting || editGen.current > ackGen.current) return protect()
        return Promise.resolve(true)
    }, [protect])

    const abandon = useCallback(() => {
        if (timer.current !== null) clearTimeout(timer.current)
        timer.current = null
        ackGen.current = editGen.current
    }, [])

    const registerRef = useRef(register)
    registerRef.current = register
    useEffect(() => {
        registerRef.current?.({ flush, abandon })
        // Leaving the document (another tab, another destination) protects
        // what was typed since the last pause; a later close waits for it.
        return () => {
            const left = flush()
            registerRef.current?.({ flush: () => left, abandon: () => {} })
        }
    }, [flush, abandon])

    const edit = (patch: Partial<Fields>) => {
        const next = { ...fieldsRef.current, ...patch }
        fieldsRef.current = next
        setFields(next)
        const generation = ++editGen.current
        // Re-armed whenever a submitted snapshot may have cleared the mark.
        if (markedGen.current <= sentGen.current) {
            markedGen.current = generation
            void call({ kind: 'editing', id: idRef.current, edit: generation })
        }
        if (timer.current !== null) clearTimeout(timer.current)
        timer.current = setTimeout(() => void protect(), PROTECT_DELAY_MS)
    }

    const reload = async () => {
        const r = await call({ kind: 'openRequest', relative: doc.relative })
        if (r.kind === 'document') adopt(r.document, r.document.fields)
    }

    const save = async () => {
        // The fields the editor holds now, whatever is still debouncing.
        const snapshot = fieldsRef.current
        const edit = editGen.current
        if (timer.current !== null) {
            clearTimeout(timer.current)
            timer.current = null
        }
        sentGen.current = Math.max(sentGen.current, edit)
        setSaving(true)
        setStatus('Saving…')
        const r = await call({ kind: 'save', id: idRef.current, revision: revisionRef.current, fields: snapshot, edit })
        setSaving(false)
        if (r.kind === 'saved' || r.kind === 'unchanged') {
            ackGen.current = Math.max(ackGen.current, edit)
            adopt(r.document, null)
            if (fieldsRef.current !== snapshot) {
                // Typed while saving: the newer text stays and is protected
                // against the new revision.
                setStatus('Saved. Edits made while saving are kept unsaved.')
                void protect()
            } else {
                setStatus(r.kind === 'saved' ? 'Saved' : 'No changes to save')
            }
            return
        }
        if (r.kind === 'saveRefused') {
            if (r.refusal.kind === 'conflict') setConflict('changed')
            if (r.refusal.kind === 'interrupted' || r.refusal.kind === 'io') setConflict('interrupted')
            setStatus(saveRefusalText(r.refusal))
            return
        }
        if (r.kind === 'refused' && r.problem.kind === 'stale') await reload()
        setStatus(resultText(r) ?? '')
    }

    const resolve = async (choice: 'useDisk' | 'keepMine') => {
        const r = await call({ kind: 'resolve', id: idRef.current, revision: revisionRef.current, fields: fieldsRef.current, choice })
        if (r.kind === 'closed') return onClosed(r.id)
        if (r.kind === 'document') {
            adopt(r.document, choice === 'useDisk' ? r.document.fields : null)
            setStatus(choice === 'useDisk' ? 'Using the files on disk. Your version was kept privately.' : 'Your edits now apply to the files on disk. Save to write them.')
            return
        }
        setStatus(resultText(r) ?? '')
    }

    const discard = async () => {
        const r = await call({ kind: 'discardDraft', id: idRef.current })
        if (r.kind === 'closed') return onClosed(r.id)
        if (r.kind === 'document') {
            ackGen.current = editGen.current
            adopt(r.document, r.document.fields)
            setStatus('Draft discarded')
            return
        }
        setStatus(resultText(r) ?? '')
    }

    const associate = async (schema: string) => {
        const r = await call({
            kind: 'associateSchema',
            id: idRef.current,
            revision: revisionRef.current,
            fields: fieldsRef.current,
            schema: schema === '' ? null : schema,
        })
        if (r.kind === 'document') adopt(r.document, null)
        else setStatus(resultText(r) ?? '')
    }

    const showPreview = async () => {
        const r = await call({ kind: 'preview', id: idRef.current, fields: fieldsRef.current })
        if (r.kind === 'preview') setPreview(r.preview)
        else setStatus(resultText(r) ?? '')
    }

    // File > Save (Cmd+S) from the native menu saves this document.
    const saveRef = useRef(save)
    saveRef.current = save
    useEffect(() => {
        const onSave = () => void saveRef.current()
        window.addEventListener(SAVE_DOCUMENT_EVENT, onSave)
        return () => window.removeEventListener(SAVE_DOCUMENT_EVENT, onSave)
    }, [])

    const dirty = doc.new || doc.metadataDirty || !same(fields, doc.saved)
    const name = requestName(doc.relative)
    return (
        <form
            aria-label={`Request ${name}`}
            data-slot='workbench-document'
            className='flex flex-col gap-4'
            onSubmit={(e) => {
                e.preventDefault()
                void save()
            }}
            onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
                    e.preventDefault()
                    void save()
                }
            }}
        >
            <div className='flex flex-wrap items-baseline gap-2'>
                <h2 className='text-[17px] font-semibold'>{name}</h2>
                <span className='text-academy-ink-faint text-[12px]'>{doc.relative}</span>
                <span className='text-academy-ink-soft text-[12px]' data-slot='workbench-dirty'>
                    {doc.new ? 'New · not saved yet' : dirty ? 'Unsaved changes' : 'Saved'}
                </span>
            </div>
            {conflict ? (
                <Callout tone='warning' icon={AlertTriangle} role='alert' data-slot='workbench-conflict'>
                    <p className='text-academy-ink font-semibold'>
                        {conflict === 'deleted' ? 'Deleted on disk' : conflict === 'interrupted' ? 'Interrupted save' : 'Changed on disk'}
                    </p>
                    <p>
                        {conflict === 'interrupted'
                            ? 'An earlier save of this request did not finish. Choose which version to continue from; both are kept privately first.'
                            : 'The files changed since this request was opened. Choose which version to continue from; both are kept privately first.'}
                    </p>
                    <div className='mt-3 flex flex-wrap gap-2'>
                        <Button type='button' tier='secondary' onClick={() => void resolve('useDisk')}>
                            Use disk
                        </Button>
                        <Button type='button' tier='secondary' onClick={() => void resolve('keepMine')}>
                            Keep mine
                        </Button>
                    </div>
                </Callout>
            ) : null}
            {doc.recovered && !conflict ? (
                <Callout tone='info' icon={History} data-slot='workbench-recovered'>
                    <p className='text-academy-ink font-semibold'>Recovered draft</p>
                    <p>Unsaved changes from an earlier session were restored. Save to keep them in the collection.</p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => void discard()}>
                        Discard draft
                    </Button>
                </Callout>
            ) : null}
            <div className='grid gap-3 sm:grid-cols-[10rem_1fr]'>
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-method`}>
                    Method
                    <Input
                        id={`${ids}-method`}
                        list={`${ids}-methods`}
                        value={fields.method}
                        autoComplete='off'
                        spellCheck={false}
                        onChange={(e) => edit({ method: e.target.value })}
                    />
                    <datalist id={`${ids}-methods`}>
                        {METHODS.map((m) => (
                            <option key={m} value={m} />
                        ))}
                    </datalist>
                </label>
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-url`}>
                    URL
                    <Input id={`${ids}-url`} value={fields.url} autoComplete='off' spellCheck={false} onChange={(e) => edit({ url: e.target.value })} />
                </label>
            </div>
            <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-query`}>
                Params
                <textarea
                    id={`${ids}-query`}
                    className={TEXTAREA}
                    value={fields.query}
                    spellCheck={false}
                    aria-describedby={`${ids}-rows`}
                    onChange={(e) => edit({ query: e.target.value })}
                />
            </label>
            <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-headers`}>
                Headers
                <textarea
                    id={`${ids}-headers`}
                    className={TEXTAREA}
                    value={fields.headers}
                    spellCheck={false}
                    aria-describedby={`${ids}-rows`}
                    onChange={(e) => edit({ headers: e.target.value })}
                />
            </label>
            <p id={`${ids}-rows`} className='text-academy-ink-faint -mt-2 text-[12px]'>
                One row per line: name=value for params, Name: value for headers. Start a row with "# " to disable it.
            </p>
            <div className='flex flex-col gap-1'>
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-kind`}>
                    Body kind
                    <select id={`${ids}-kind`} className={SELECT} value={fields.bodyKind} onChange={(e) => edit({ bodyKind: e.target.value })}>
                        {BODY_KINDS.map((k) => (
                            <option key={k} value={k}>
                                {k}
                            </option>
                        ))}
                    </select>
                </label>
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-body`}>
                    Body
                    <textarea
                        id={`${ids}-body`}
                        className={TEXTAREA}
                        value={fields.body}
                        spellCheck={false}
                        readOnly={!doc.bodyEditable}
                        onChange={(e) => edit({ body: e.target.value })}
                    />
                </label>
                {!doc.bodyEditable ? (
                    <p className='text-academy-ink-faint text-[12px]'>This body is not text or is larger than 4 MiB; it is kept exactly as it is.</p>
                ) : null}
            </div>
            <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-settings`}>
                Auth and variables (TOML)
                <textarea
                    id={`${ids}-settings`}
                    className={TEXTAREA}
                    value={fields.settings}
                    spellCheck={false}
                    aria-describedby={`${ids}-settings-hint`}
                    onChange={(e) => edit({ settings: e.target.value })}
                />
            </label>
            <p id={`${ids}-settings-hint`} className='text-academy-ink-faint -mt-2 text-[12px]'>
                [auth] names secret references (kind none, basic, bearer, api-key or oauth2); [variables] holds strings or {'{ secret = "reference" }'}. Values are entered in Secrets, never here.
            </p>
            <label className='flex flex-col gap-1 text-[13px] font-medium'>
                Schema
                <select className={SELECT} value={doc.schema ?? ''} onChange={(e) => void associate(e.target.value)}>
                    <option value=''>No schema</option>
                    {schemas.map((s) => (
                        <option key={s} value={s}>
                            {s}
                        </option>
                    ))}
                </select>
            </label>
            <div className='flex flex-wrap items-center gap-2'>
                <Button type='submit' tier='primary' disabled={saving}>
                    <Save aria-hidden className='size-4' />
                    Save
                </Button>
                <Button type='button' tier='secondary' onClick={() => void showPreview()}>
                    Preview
                </Button>
                {dirty && !doc.new && !doc.recovered ? (
                    <Button type='button' tier='secondary' onClick={() => void discard()}>
                        Discard changes
                    </Button>
                ) : null}
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='workbench-save-status'>
                    {status}
                </p>
            </div>
            {preview ? <PreviewPanel preview={preview} /> : null}
        </form>
    )
}

function PreviewPanel({ preview }: { preview: Preview }) {
    return (
        <section aria-label='Request preview' data-slot='workbench-preview' className='border-academy-rule rounded-academy-md flex flex-col gap-2 border p-4 text-[13px]'>
            <h3 className='text-[14px] font-semibold'>Preview (nothing is sent)</h3>
            <p className='font-mono break-all'>{`${preview.method} ${preview.url}`}</p>
            {preview.query.length ? <p className='font-mono break-all'>Params: {preview.query.map(([n, v]) => `${n}=${v}`).join('&')}</p> : null}
            {preview.headers.map(([n, v], i) => (
                <p key={i} className='font-mono break-all'>
                    {`${n}: ${v}`}
                </p>
            ))}
            {preview.body ? <pre className='bg-academy-bg-inset overflow-auto p-2 font-mono text-[12px] whitespace-pre-wrap'>{preview.body}</pre> : null}
            {preview.auth && preview.auth !== 'none' ? <p>{`Authentication: ${preview.auth}`}</p> : null}
            {preview.unresolved.length ? <p role='alert'>{`Unresolved variables: ${preview.unresolved.join(', ')}`}</p> : null}
            {preview.missingSecrets.length ? <p role='alert'>{`Secrets without a value on this Mac: ${preview.missingSecrets.join(', ')}`}</p> : null}
            {preview.problem ? <p role='alert'>{fieldText(preview.problem)}</p> : null}
            {preview.jsonWarning ? <p>The JSON body does not parse; it is kept exactly as typed.</p> : null}
            <p className='text-academy-ink-faint text-[12px]'>Secret values are masked. Sending requests arrives in a later release.</p>
        </section>
    )
}
