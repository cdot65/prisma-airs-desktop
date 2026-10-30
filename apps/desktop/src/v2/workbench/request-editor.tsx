import { useCallback, useEffect, useId, useRef, useState } from 'react'
import type { DocumentView, Fields, ImportNote, Preview, RunView, RunningView, SendReviewView } from '@prisma-airs/desktop-contracts'
import { type WorkbenchCall, fieldText, resultText, saveRefusalText } from '../../features/workbench/workbench-call'
import { Btn } from '../controls'
import { Glyph } from '../icons'
import { API_COPY as C, BODY_KINDS, METHODS, methodColor } from './copy'
import { type Row, type RowKind, enabledCount, parseRows, serializeRows, splitUrl } from './request-rows'
import { ResponsePane } from './response-pane'
import { requestName } from './request-tree'

/** File > Save (Cmd+S) arrives from the native menu as this window event. */
export const SAVE_DOCUMENT_EVENT = 'airs-save-document'
/** How long typing pauses before the snapshot is protected on this Mac. */
export const PROTECT_DELAY_MS = 400

type Sub = 'params' | 'headers' | 'body' | 'auth' | 'variables' | 'preview'
const SUBS: Sub[] = ['params', 'headers', 'body', 'auth', 'variables', 'preview']

const same = (a: Fields, b: Fields) =>
    a.method === b.method && a.url === b.url && a.headers === b.headers && a.query === b.query && a.body === b.body && a.bodyKind === b.bodyKind && a.settings === b.settings

/**
 * What the completion status region announces after Send (C33, known gap 5):
 * "<request> · <code> · <duration>". The response body is outside it.
 */
export function completionText(record: { relative: string; status: number | null; elapsedMs: number }): string {
    const code = record.status !== null ? String(record.status) : 'no response'
    const ms = record.elapsedMs
    const duration = ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`
    return `${requestName(record.relative)} · ${code} · ${duration}`
}

export const NOTE_TEXT = (note: ImportNote): string => {
    switch (note.kind) {
        case 'flagOmitted':
            return 'Presentation and globbing flags were left out.'
        case 'authRemoved':
            return `The authentication credential was removed: store its value in Secrets as ${note.reference}.`
        case 'headerReplaced':
            return `A credential header now uses {{${note.variable}}}: store its value in Secrets as ${note.reference}.`
        case 'compressed':
            return '--compressed became Accept-Encoding: identity.'
        case 'emptyCookie':
            return 'The empty cookie option added no cookie.'
    }
}

const MONO = "ui-monospace, 'SF Mono', Menlo, monospace"
const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
export const TEXTAREA_STYLE = {
    border: '1px solid var(--v2-rule)',
    borderRadius: 8,
    background: 'var(--v2-paper)',
    color: 'var(--v2-ink)',
    fontFamily: MONO,
    fontSize: 12.5,
    lineHeight: 1.6,
    padding: 12,
    minHeight: 140,
    width: '100%',
    resize: 'vertical' as const,
}

export interface EditorHandle {
    /** Protects any edit not yet acknowledged; resolves whether everything typed is now kept on this Mac. */
    flush: () => Promise<boolean>
    /** The operator chose to close without the latest edits. */
    abandon: () => void
}

interface Props {
    initial: DocumentView
    call: WorkbenchCall
    schemas: string[]
    historyKey: string
    onClosed: (id: string) => void
    register?: (handle: EditorHandle) => void
    context?: string | null
    running?: RunningView[]
    review?: SendReviewView | null
}

/**
 * One request document as R2App draws it (C41): the method, URL and Send
 * on one line with the save state beside them, the Params / Headers / Body
 * / Auth / Variables / Preview sub-tabs on the left and the response pane
 * on the right. The C14/C15 behaviour is the legacy editor's, ported: the
 * editor text lives here (React owns immediate input); Rust owns the
 * baseline and revision; Save and Send take the fields the editor holds at
 * that moment; typing pauses protect the snapshot on this Mac; a reply
 * never overwrites newer typing.
 */
export function RequestEditor({ initial, call, schemas, historyKey, onClosed, register, context = null, running = [], review = null }: Props) {
    const ids = useId()
    const [doc, setDoc] = useState(initial)
    const [fields, setFields] = useState(initial.fields)
    const fieldsRef = useRef(initial.fields)
    const revisionRef = useRef(initial.revision)
    const idRef = useRef(initial.id)
    const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
    const editGen = useRef(0)
    const markedGen = useRef(0)
    const sentGen = useRef(0)
    const ackGen = useRef(0)
    const [status, setStatus] = useState('')
    const [saving, setSaving] = useState(false)
    const [preview, setPreview] = useState<Preview | null>(null)
    const [run, setRun] = useState<RunView | null>(null)
    const [sending, setSending] = useState(false)
    const [sub, setSub] = useState<Sub>('params')
    const [asText, setAsText] = useState<Record<RowKind, boolean>>({
        query: false,
        headers: false,
    })
    const [conflict, setConflict] = useState<'changed' | 'deleted' | 'interrupted' | null>(initial.interrupted ? 'interrupted' : initial.conflict)

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
        return call({
            kind: 'protect',
            id: idRef.current,
            revision: revisionRef.current,
            fields: fieldsRef.current,
            edit,
        }).then((r) => {
            if (r.kind === 'protected') {
                ackGen.current = Math.max(ackGen.current, edit)
                return true
            }
            setStatus(resultText(r) ?? '')
            return false
        })
    }, [call])

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
        const snapshot = fieldsRef.current
        const edit = editGen.current
        if (timer.current !== null) {
            clearTimeout(timer.current)
            timer.current = null
        }
        sentGen.current = Math.max(sentGen.current, edit)
        setSaving(true)
        setStatus('Saving…')
        const r = await call({
            kind: 'save',
            id: idRef.current,
            revision: revisionRef.current,
            fields: snapshot,
            edit,
        })
        setSaving(false)
        if (r.kind === 'saved' || r.kind === 'unchanged') {
            ackGen.current = Math.max(ackGen.current, edit)
            adopt(r.document, null)
            if (fieldsRef.current !== snapshot) {
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
        const r = await call({
            kind: 'resolve',
            id: idRef.current,
            revision: revisionRef.current,
            fields: fieldsRef.current,
            choice,
        })
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

    const sendNow = async (reviewId: string | null) => {
        const snapshot = fieldsRef.current
        setSending(true)
        setRun(null)
        setStatus(reviewId ? C.waitingReview : C.sending)
        const r = await call(
            reviewId
                ? {
                      kind: 'apply',
                      review: reviewId,
                      id: idRef.current,
                      revision: revisionRef.current,
                      fields: snapshot,
                      context,
                  }
                : {
                      kind: 'send',
                      id: idRef.current,
                      revision: revisionRef.current,
                      fields: snapshot,
                      context,
                  }
        )
        setSending(false)
        if (r.kind === 'ran') {
            setRun(r.run)
            setStatus(completionText(r.run.record))
            return
        }
        if (r.kind === 'reviewRequired') {
            setStatus(C.reviewBelow)
            return
        }
        if (r.kind === 'refused' && r.problem.kind === 'stale') await reload()
        setStatus(resultText(r) ?? '')
    }

    const readPreview = useCallback(async () => {
        const r = await call({
            kind: 'preview',
            id: idRef.current,
            fields: fieldsRef.current,
        })
        if (r.kind === 'preview') setPreview(r.preview)
        else setStatus(resultText(r) ?? '')
    }, [call])
    // Variables and Preview read the rendered request when they open.
    useEffect(() => {
        if (sub === 'variables' || sub === 'preview') void readPreview()
    }, [sub, readPreview])

    const saveRef = useRef(save)
    saveRef.current = save
    useEffect(() => {
        const onSave = () => void saveRef.current()
        window.addEventListener(SAVE_DOCUMENT_EVENT, onSave)
        return () => window.removeEventListener(SAVE_DOCUMENT_EVENT, onSave)
    }, [])

    const dirty = doc.new || doc.metadataDirty || !same(fields, doc.saved)
    const name = requestName(doc.relative)
    const state = doc.new ? C.state.new : dirty ? C.state.unsaved : C.state.saved
    // A save's word ("Saved", "No changes to save") gives way to the state once the request is edited again.
    useEffect(() => {
        if (dirty) setStatus((s) => (s === 'Saved' || s === 'No changes to save' ? '' : s))
    }, [dirty])
    const url = splitUrl(fields.url)

    const rowsTable = (kind: RowKind) => {
        const text = kind === 'query' ? fields.query : fields.headers
        const rows = parseRows(text, kind)
        const textMode = asText[kind] || rows === null
        const write = (next: Row[]) => edit(kind === 'query' ? { query: serializeRows(next, kind) } : { headers: serializeRows(next, kind) })
        const label = kind === 'query' ? C.sub.params : C.sub.headers
        return (
            <>
                <div className='flex flex-wrap items-center' style={{ gap: 8 }}>
                    <Btn disabled={textMode} onClick={() => write([...(rows ?? []), { enabled: true, name: '', value: '' }])}>
                        {C.addRow[kind === 'query' ? 'params' : 'headers']}
                    </Btn>
                    {rows === null ? (
                        <span className='text-[12px]' style={faint}>
                            {C.textOnly}
                        </span>
                    ) : null}
                    <span className='ml-auto flex rounded-[7px] text-[12px]' style={{ padding: 2, background: 'var(--v2-inset)' }} role='group' aria-label={`${label} view`}>
                        {(['table', 'text'] as const).map((m) => {
                            const on = m === 'text' ? textMode : !textMode
                            return (
                                <button
                                    key={m}
                                    type='button'
                                    aria-pressed={on}
                                    disabled={m === 'table' && rows === null}
                                    onClick={() => setAsText((s) => ({ ...s, [kind]: m === 'text' }))}
                                    className='min-h-0 rounded-[5px] font-semibold disabled:opacity-60'
                                    style={{
                                        padding: '2px 9px',
                                        border: 0,
                                        background: on ? 'var(--v2-paper)' : 'transparent',
                                        color: on ? 'var(--v2-ink)' : 'var(--v2-soft)',
                                    }}
                                >
                                    {m === 'table' ? C.table : C.text}
                                </button>
                            )
                        })}
                    </span>
                </div>
                {textMode ? (
                    <label className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                        {label}
                        <textarea value={text} spellCheck={false} style={TEXTAREA_STYLE} onChange={(e) => edit(kind === 'query' ? { query: e.target.value } : { headers: e.target.value })} />
                        <span className='text-[12px] font-normal' style={faint}>
                            {kind === 'query' ? 'One row per line: name=value. Start a row with "# " to disable it.' : 'One row per line: Name: value. Start a row with "# " to disable it.'}
                        </span>
                    </label>
                ) : (
                    <div role='table' aria-label={label} className='overflow-hidden rounded-[8px] border' style={{ borderColor: 'var(--v2-rule)' }} data-slot={`rows-${kind}`}>
                        <div
                            role='row'
                            className='grid text-[12px] font-semibold'
                            style={{
                                gridTemplateColumns: '30px 1fr 1fr 26px',
                                background: 'var(--v2-inset)',
                                ...faint,
                            }}
                        >
                            <span role='columnheader'>
                                <span className='sr-only'>{C.columns.enabled}</span>
                            </span>
                            <span role='columnheader' style={{ padding: 8 }}>
                                {C.columns.name}
                            </span>
                            <span role='columnheader' style={{ padding: 8 }}>
                                {C.columns.value}
                            </span>
                            <span role='columnheader'>
                                <span className='sr-only'>{C.columns.remove}</span>
                            </span>
                        </div>
                        {(rows ?? []).map((r, i) => {
                            const set = (patch: Partial<Row>) => write((rows ?? []).map((x, j) => (j === i ? { ...x, ...patch } : x)))
                            return (
                                <div
                                    key={i}
                                    role='row'
                                    className='grid items-center'
                                    style={{
                                        gridTemplateColumns: '30px 1fr 1fr 26px',
                                        borderTop: '1px solid var(--v2-rule)',
                                    }}
                                >
                                    <span role='cell' className='flex justify-center'>
                                        <input type='checkbox' aria-label={C.rowEnabled(r.name)} checked={r.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
                                    </span>
                                    <span role='cell' style={{ borderLeft: '1px solid var(--v2-rule)' }}>
                                        <input
                                            aria-label={`${C.rowName} ${i + 1}`}
                                            value={r.name}
                                            spellCheck={false}
                                            autoComplete='off'
                                            onChange={(e) => set({ name: e.target.value })}
                                            className='w-full min-h-0 border-0 bg-transparent'
                                            style={{
                                                padding: '10px 8px',
                                                border: 0,
                                                minHeight: 0,
                                                borderRadius: 0,
                                                background: 'transparent',
                                                fontFamily: MONO,
                                                fontSize: 12,
                                                color: r.enabled ? 'var(--v2-ink)' : 'var(--v2-faint)',
                                            }}
                                        />
                                    </span>
                                    <span role='cell' style={{ borderLeft: '1px solid var(--v2-rule)' }}>
                                        <input
                                            aria-label={`${C.rowValue} ${i + 1}`}
                                            value={r.value}
                                            spellCheck={false}
                                            autoComplete='off'
                                            onChange={(e) => set({ value: e.target.value })}
                                            className='w-full min-h-0 border-0 bg-transparent'
                                            style={{
                                                padding: '10px 8px',
                                                border: 0,
                                                minHeight: 0,
                                                borderRadius: 0,
                                                background: 'transparent',
                                                fontFamily: MONO,
                                                fontSize: 12,
                                                color: r.enabled ? 'var(--v2-ink)' : 'var(--v2-faint)',
                                            }}
                                        />
                                    </span>
                                    <span role='cell' className='flex justify-center'>
                                        <button
                                            type='button'
                                            aria-label={C.removeRow(r.name)}
                                            onClick={() => write((rows ?? []).filter((_, j) => j !== i))}
                                            className='grid size-6 min-h-0 place-items-center text-[14px]'
                                            style={{ border: 0, background: 'transparent', ...faint }}
                                        >
                                            ×
                                        </button>
                                    </span>
                                </div>
                            )
                        })}
                    </div>
                )}
            </>
        )
    }

    const panel =
        sub === 'params' || sub === 'headers' ? (
            rowsTable(sub === 'params' ? 'query' : 'headers')
        ) : sub === 'body' ? (
            <>
                <div className='flex flex-wrap items-end' style={{ gap: 12 }}>
                    <label htmlFor={`${ids}-kind`} className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                        {C.bodyKind}
                        <select
                            id={`${ids}-kind`}
                            value={fields.bodyKind}
                            onChange={(e) => edit({ bodyKind: e.target.value })}
                            className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px]'
                            style={{
                                borderColor: 'var(--v2-ctl)',
                                background: 'var(--v2-paper)',
                                color: 'var(--v2-ink)',
                            }}
                        >
                            {BODY_KINDS.map((k) => (
                                <option key={k} value={k}>
                                    {k}
                                </option>
                            ))}
                        </select>
                    </label>
                    <label className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                        {C.schema}
                        <select
                            value={doc.schema ?? ''}
                            onChange={(e) => void associate(e.target.value)}
                            className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px]'
                            style={{
                                borderColor: 'var(--v2-ctl)',
                                background: 'var(--v2-paper)',
                                color: 'var(--v2-ink)',
                            }}
                        >
                            <option value=''>{C.noSchema}</option>
                            {schemas.map((s) => (
                                <option key={s} value={s}>
                                    {s}
                                </option>
                            ))}
                        </select>
                    </label>
                </div>
                <label htmlFor={`${ids}-body`} className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                    {C.body}
                    <textarea
                        id={`${ids}-body`}
                        value={fields.body}
                        spellCheck={false}
                        readOnly={!doc.bodyEditable}
                        style={{ ...TEXTAREA_STYLE, minHeight: 200 }}
                        onChange={(e) => edit({ body: e.target.value })}
                    />
                </label>
                {!doc.bodyEditable ? (
                    <p className='text-[12px]' style={faint}>
                        {C.subText.bodyFixed}
                    </p>
                ) : null}
            </>
        ) : sub === 'auth' ? (
            <label htmlFor={`${ids}-settings`} className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                {C.settings}
                <textarea id={`${ids}-settings`} value={fields.settings} spellCheck={false} style={TEXTAREA_STYLE} onChange={(e) => edit({ settings: e.target.value })} />
            </label>
        ) : sub === 'variables' ? (
            <>
                {preview ? (
                    preview.variables.length ? (
                        <div role='table' aria-label={C.sub.variables} className='overflow-hidden rounded-[8px] border' style={{ borderColor: 'var(--v2-rule)' }} data-slot='rows-variables'>
                            <div
                                role='row'
                                className='grid text-[12px] font-semibold'
                                style={{
                                    gridTemplateColumns: '1fr 1fr 1fr',
                                    background: 'var(--v2-inset)',
                                    ...faint,
                                }}
                            >
                                <span role='columnheader' style={{ padding: 8 }}>
                                    {C.columns.name}
                                </span>
                                <span role='columnheader' style={{ padding: 8 }}>
                                    Layer
                                </span>
                                <span role='columnheader' style={{ padding: 8 }}>
                                    {C.columns.value}
                                </span>
                            </div>
                            {preview.variables.map((v) => (
                                <div
                                    key={v.name}
                                    role='row'
                                    className='grid'
                                    style={{
                                        gridTemplateColumns: '1fr 1fr 1fr',
                                        borderTop: '1px solid var(--v2-rule)',
                                        fontFamily: MONO,
                                        fontSize: 12,
                                    }}
                                >
                                    <span role='cell' className='break-all' style={{ padding: '10px 8px' }}>
                                        {v.name}
                                    </span>
                                    <span
                                        role='cell'
                                        style={{
                                            padding: '10px 8px',
                                            borderLeft: '1px solid var(--v2-rule)',
                                            ...soft,
                                        }}
                                    >
                                        {String(v.layer)}
                                    </span>
                                    <span
                                        role='cell'
                                        className='break-all'
                                        style={{
                                            padding: '10px 8px',
                                            borderLeft: '1px solid var(--v2-rule)',
                                            ...soft,
                                        }}
                                    >
                                        {v.secret ? `•••• ${v.secret}${v.stored === false ? ' (no value on this Mac)' : ''}` : 'from the environment'}
                                    </span>
                                </div>
                            ))}
                        </div>
                    ) : (
                        <p className='text-[13px]' style={faint}>
                            No variables are used by this request.
                        </p>
                    )
                ) : null}
                {preview?.unresolved.length ? <p role='alert' className='text-[13px]'>{`Unresolved variables: ${preview.unresolved.join(', ')}`}</p> : null}
                <Btn className='self-start' onClick={() => void readPreview()}>
                    {C.readPreview}
                </Btn>
            </>
        ) : (
            <>
                {preview ? (
                    <section aria-label='Request preview' data-slot='workbench-preview' className='flex flex-col text-[13px]' style={{ gap: 8 }}>
                        <pre
                            role='region'
                            aria-label={C.sub.preview}
                            className='break-all whitespace-pre-wrap'
                            style={{
                                ...TEXTAREA_STYLE,
                                minHeight: 0,
                                resize: 'none',
                                margin: 0,
                            }}
                        >
                            {[
                                `${preview.method} ${preview.url}${preview.query.length ? `?${preview.query.map(([n, v]) => `${n}=${v}`).join('&')}` : ''}`,
                                ...preview.headers.map(([n, v]) => `${n}: ${v}`),
                                ...(preview.body ? ['', preview.body] : []),
                            ].join('\n')}
                        </pre>
                        {preview.auth && preview.auth !== 'none' ? <p>{`Authentication: ${preview.auth}`}</p> : null}
                        {preview.unresolved.length ? <p role='alert'>{`Unresolved variables: ${preview.unresolved.join(', ')}`}</p> : null}
                        {preview.missingSecrets.length ? <p role='alert'>{`Secrets without a value on this Mac: ${preview.missingSecrets.join(', ')}`}</p> : null}
                        {preview.problem ? <p role='alert'>{fieldText(preview.problem)}</p> : null}
                        {preview.jsonWarning ? <p>The JSON body does not parse; it is kept exactly as typed.</p> : null}
                    </section>
                ) : null}
                <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                    <Btn onClick={() => void readPreview()}>{C.readPreview}</Btn>
                    <span className='text-[12px]' style={faint}>
                        {C.previewHint}
                    </span>
                </div>
            </>
        )

    const extra = (s: Sub): string => {
        if (s === 'params') return String(enabledCount(fields.query, 'query') || '')
        if (s === 'headers') return String(enabledCount(fields.headers, 'headers') || '')
        if (s === 'body') return fields.body ? fields.bodyKind.toUpperCase() : ''
        return ''
    }

    return (
        <form
            aria-label={`Request ${name}`}
            data-slot='workbench-document'
            className='flex min-h-0 flex-grow flex-col'
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
            {conflict ? (
                <div
                    role='alert'
                    className='flex flex-col text-[13px]'
                    style={{
                        gap: 8,
                        margin: '12px 20px 0',
                        padding: '12px 14px',
                        borderRadius: 10,
                        background: 'var(--v2-flag-bg)',
                    }}
                    data-slot='workbench-conflict'
                >
                    <p className='font-semibold'>{conflict === 'deleted' ? C.conflict.deleted : conflict === 'interrupted' ? C.conflict.interrupted : C.conflict.changed}</p>
                    <p>{conflict === 'interrupted' ? C.conflict.interruptedText : C.conflict.changedText}</p>
                    <div className='flex flex-wrap' style={{ gap: 8 }}>
                        <Btn onClick={() => void resolve('useDisk')}>{C.conflict.useDisk}</Btn>
                        <Btn onClick={() => void resolve('keepMine')}>{C.conflict.keepMine}</Btn>
                    </div>
                </div>
            ) : null}
            {doc.recovered && !conflict ? (
                <div
                    className='flex flex-col text-[13px]'
                    style={{
                        gap: 8,
                        margin: '12px 20px 0',
                        padding: '12px 14px',
                        borderRadius: 10,
                        background: 'var(--v2-acc-soft)',
                    }}
                    data-slot='workbench-recovered'
                >
                    <p className='font-semibold'>{C.recovered}</p>
                    <p>{C.recoveredText}</p>
                    <Btn className='self-start' onClick={() => void discard()}>
                        {C.discardDraft}
                    </Btn>
                </div>
            ) : null}
            <div
                className='flex flex-wrap items-center'
                style={{
                    gap: 8,
                    padding: '16px 20px',
                    borderBottom: '1px solid var(--v2-rule)',
                }}
                data-slot='workbench-request-line'
            >
                <div
                    className='flex min-w-0 flex-grow overflow-hidden rounded-[8px] border'
                    style={{
                        height: 34,
                        borderColor: 'var(--v2-rule)',
                        background: 'var(--v2-paper)',
                        flexBasis: 320,
                    }}
                >
                    <span className='relative flex shrink-0 items-center' style={{ width: 86, borderRight: '1px solid var(--v2-rule)' }}>
                        <select
                            aria-label={C.method}
                            value={fields.method}
                            onChange={(e) => edit({ method: e.target.value })}
                            className='h-full w-full min-h-0 appearance-none border-0 bg-transparent font-mono text-[12px] font-bold'
                            style={{
                                border: 0,
                                minHeight: 0,
                                borderRadius: 0,
                                background: 'transparent',
                                padding: '0 24px 0 10px',
                                color: methodColor(fields.method),
                            }}
                        >
                            {(METHODS.includes(fields.method) ? METHODS : [fields.method, ...METHODS]).map((m) => (
                                <option key={m} value={m}>
                                    {m}
                                </option>
                            ))}
                        </select>
                        <span className='pointer-events-none absolute' style={{ right: 8 }}>
                            <Glyph name='chev' size={10} color='var(--v2-faint)' />
                        </span>
                    </span>
                    <span className='flex min-w-0 flex-grow items-center' style={{ padding: '0 12px', gap: 4 }}>
                        {url.base ? (
                            <span
                                aria-hidden
                                className='shrink-0 rounded-[4px] font-mono text-[12px]'
                                style={{
                                    padding: '1px 5px',
                                    background: 'var(--v2-acc-soft)',
                                    color: 'var(--v2-acc)',
                                }}
                            >
                                {url.base}
                            </span>
                        ) : null}
                        <input
                            aria-label={C.url}
                            value={fields.url}
                            autoComplete='off'
                            spellCheck={false}
                            onChange={(e) => edit({ url: e.target.value })}
                            className='min-h-0 min-w-0 flex-grow border-0 bg-transparent font-mono text-[12.5px]'
                            style={{
                                border: 0,
                                minHeight: 0,
                                borderRadius: 0,
                                background: 'transparent',
                                padding: 0,
                                color: 'var(--v2-ink)',
                            }}
                        />
                    </span>
                </div>
                <Btn tier='primary' className='h-[34px] px-[18px] text-[13px]' disabled={sending} onClick={() => void sendNow(null)}>
                    {C.send}
                </Btn>
                <Btn className='h-[34px]' type='submit' disabled={saving}>
                    {C.save}
                </Btn>
                {dirty && !doc.new && !doc.recovered ? (
                    <Btn tier='tertiary' className='h-[34px]' onClick={() => void discard()}>
                        {C.discardChanges}
                    </Btn>
                ) : null}
                <span
                    role='status'
                    aria-live='polite'
                    className='flex min-w-0 items-center rounded-[8px] border text-[12px]'
                    style={{
                        gap: 6,
                        minHeight: 34,
                        padding: '6px 12px',
                        borderColor: 'var(--v2-rule)',
                        background: 'var(--v2-inset)',
                        color: 'var(--v2-soft)',
                        maxWidth: '100%',
                    }}
                    data-slot='workbench-save-status'
                    data-state={doc.new ? 'new' : dirty ? 'unsaved' : 'saved'}
                >
                    {status === '' || status === 'Saved' ? <Glyph name={dirty ? 'dot' : 'check'} size={12} color={dirty ? 'var(--v2-acc)' : 'var(--v2-ok)'} /> : null}
                    <span className='min-w-0'>{status || state}</span>
                </span>
            </div>
            <div className='flex min-h-0 flex-grow' data-slot='workbench-columns'>
                <div className='flex min-w-0 flex-col' style={{ flex: '1 1 360px', borderRight: '1px solid var(--v2-rule)' }}>
                    <div
                        role='tablist'
                        aria-label={C.request}
                        className='flex shrink-0 flex-wrap'
                        style={{
                            gap: 4,
                            padding: '0 12px',
                            borderBottom: '1px solid var(--v2-rule)',
                        }}
                    >
                        {SUBS.map((s) => (
                            <button
                                key={s}
                                type='button'
                                role='tab'
                                aria-selected={sub === s}
                                aria-controls={`${ids}-sub-panel`}
                                onClick={() => setSub(s)}
                                className='min-h-0 shrink-0 text-[12.5px] font-semibold whitespace-nowrap'
                                style={{
                                    height: 38,
                                    padding: '0 10px',
                                    border: 0,
                                    borderBottom: `2px solid ${sub === s ? 'var(--v2-acc)' : 'transparent'}`,
                                    background: 'transparent',
                                    color: sub === s ? 'var(--v2-ink)' : 'var(--v2-soft)',
                                }}
                            >
                                {C.sub[s]}
                                {extra(s) ? (
                                    <span className='font-normal' style={{ fontSize: 12, ...faint }}>
                                        {' '}
                                        {extra(s)}
                                    </span>
                                ) : null}
                            </button>
                        ))}
                    </div>
                    <div role='tabpanel' id={`${ids}-sub-panel`} className='flex min-h-0 flex-grow flex-col overflow-auto' style={{ gap: 12, padding: '18px 20px' }}>
                        <p className='text-[12px] leading-[1.45]' style={soft}>
                            {C.subText[sub]}
                        </p>
                        {panel}
                    </div>
                </div>
                <ResponsePane
                    run={run}
                    running={running}
                    review={review}
                    call={call}
                    historyKey={historyKey}
                    onApply={() => review && void sendNow(review.review)}
                    onDismiss={() => void call({ kind: 'dismissReview' }).then(() => setStatus(''))}
                />
            </div>
        </form>
    )
}
