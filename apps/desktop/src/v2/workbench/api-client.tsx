import { forwardRef, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { DocumentView, ImportNote, ShellSnapshot, WorkbenchView as WorkbenchState } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../../features/tenants/dispatch'
import { type ObserveWorkbench, type WorkbenchCall, resultText, workbenchCall } from '../../features/workbench/workbench-call'
import { Btn } from '../controls'
import { Glyph } from '../icons'
import { Modal } from '../modal'
import { API_COPY as C, methodColor } from './copy'
import { type EditorHandle, NOTE_TEXT, RequestEditor } from './request-editor'
import { RequestTree, requestName } from './request-tree'
import { type Run, type Tool, ToolsSheet } from './tools-sheet'

export interface ApiClientProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveWorkbench
}

type Closing = { kind: 'tab'; id: string; name: string } | { kind: 'collection' }
const TOOLS: Tool[] = ['environments', 'secrets', 'schemas', 'import', 'git']
const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
// A chip never widens the title row: it shrinks and truncates within it (a 900-point window at 200% overflowed by its width).
const CHIP = 'flex h-8 max-w-full min-h-0 min-w-0 items-center truncate rounded-[7px] border text-[13px]'
const chipStyle = {
    gap: 8,
    padding: '0 12px',
    borderColor: 'var(--v2-rule)',
    background: 'var(--v2-paper)',
    color: 'var(--v2-ink)',
}

/**
 * The API Client as R2App draws it (C41): the title row with the
 * Collection and Environment chips, the 286px request tree, the open
 * requests' tabs, the request line, sub-tabs and response pane, the
 * footer. New components over the workbench intents: the C14/C15
 * behaviour (local-first collections, drafts protected on this Mac,
 * reviewed tenant-credential changes, immutable history) is unchanged.
 * Rust owns each document's revision, conflicts and the private drafts;
 * nothing in this view sends a request by itself.
 */
export const ApiClient = forwardRef<HTMLHeadingElement, ApiClientProps>(function ApiClient({ snapshot, dispatch, observe }, ref) {
    const call = useMemo(() => workbenchCall(dispatch, observe), [dispatch, observe])
    const state = snapshot.workbench
    const collection = state.collection
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const [message, setMessage] = useState('')
    const [offerNew, setOfferNew] = useState(false)
    const [tool, setTool] = useState<Tool | null>(null)
    const [menu, setMenu] = useState(false)
    const [doc, setDoc] = useState<DocumentView | null>(null)
    const [notes, setNotes] = useState<ImportNote[]>([])
    const loading = useRef<string | null>(null)
    const editors = useRef(new Map<string, EditorHandle>())
    const [closing, setClosing] = useState<Closing | null>(null)

    // A refusal stays shown until the next action that succeeds (a cancelled sheet keeps it).
    const run: Run = async (request) => {
        const r = await call(request)
        const text = resultText(r)
        if (text) {
            setMessage(text)
            setOfferNew(r.kind === 'refused' && r.problem.kind === 'project' && r.problem.detail === 'notCollection')
        } else if (r.kind !== 'cancelled') {
            setMessage('')
            setOfferNew(false)
        }
        return r
    }

    // Closing protects the latest typing first. If that fails, or Rust still
    // sees unprotected edits, the operator chooses: keep editing, or close
    // and discard them.
    const close = async (target: Closing, discard = false) => {
        setClosing(null)
        const handles = target.kind === 'tab' ? [editors.current.get(target.id)] : [...editors.current.values()]
        if (discard) {
            for (const h of handles) h?.abandon()
        } else {
            const kept = await Promise.all(handles.map((h) => (h ? h.flush() : Promise.resolve(true))))
            if (kept.includes(false)) {
                setClosing(target)
                return
            }
        }
        const r = await run(target.kind === 'tab' ? { kind: 'closeTab', id: target.id, discard } : { kind: 'closeCollection', discard })
        if (r.kind === 'refused' && r.problem.kind === 'unprotected') {
            setClosing(target)
            return
        }
        if (r.kind === 'closed' || r.kind === 'done') {
            if (target.kind === 'tab') editors.current.delete(target.id)
            else editors.current.clear()
        }
    }

    const show = (r: Awaited<ReturnType<WorkbenchCall>>) => {
        if (r.kind === 'document') {
            setDoc(r.document)
            setNotes(r.notes)
        }
    }

    const selected = collection?.selected ?? null
    const selectedTab = collection?.tabs.find((t) => t.id === selected) ?? null
    // The core's selection is followed when it changes, not re-asserted against the document shown: after + the
    // draft is shown from the core's reply before the snapshot naming it selected arrives, and re-opening the
    // previously selected request in between made the core select that one again (the draft's tab lost).
    const followed = useRef<string | null>(null)
    useEffect(() => {
        if (!selectedTab || followed.current === selectedTab.id) return
        followed.current = selectedTab.id
        if (doc?.id === selectedTab.id || loading.current === selectedTab.id) return
        loading.current = selectedTab.id
        void call({ kind: 'openRequest', relative: selectedTab.relative }).then((r) => {
            loading.current = null
            if (r.kind === 'document') setDoc(r.document)
        })
    }, [selectedTab, doc, call])
    useEffect(() => {
        if (!collection) setDoc(null)
    }, [collection])
    const loaded = state.loaded
    useEffect(() => {
        if (!loaded) void call({ kind: 'load' })
    }, [loaded, call])
    // The collection menu closes on Escape and on a click elsewhere.
    useEffect(() => {
        if (!menu) return
        const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setMenu(false)
        const onClick = () => setMenu(false)
        window.addEventListener('keydown', onKey)
        window.addEventListener('click', onClick)
        return () => {
            window.removeEventListener('keydown', onKey)
            window.removeEventListener('click', onClick)
        }
    }, [menu])

    const where = collection ? (collection.builtIn ? C.chips.builtIn : collection.folder) : ''
    const environment = collection?.environments.find((e) => e.name === collection.environment) ?? null

    const band = (
        <div
            data-slot='page-header-v2'
            className='flex shrink-0 flex-wrap items-end'
            style={{
                gap: 12,
                padding: 'var(--v2-page-top) var(--v2-page-x) 18px',
                borderBottom: '1px solid var(--v2-rule)',
            }}
        >
            <div className='min-w-0'>
                <h1 ref={ref} id='workbench-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)', letterSpacing: '-0.01em' }}>
                    {C.title}
                </h1>
                <p className='text-[14px]' style={{ marginTop: 4, ...soft }} data-slot='api-lead'>
                    {collection ? C.lead(collection.name, tenant ? `${where} · ${tenant.label}` : where) : C.leadNone}
                </p>
            </div>
            {collection ? (
                <div className='ml-auto flex min-w-0 max-w-full flex-wrap items-center' style={{ gap: 8 }} data-slot='api-chips'>
                    <span className='relative'>
                        <button
                            type='button'
                            aria-haspopup='menu'
                            aria-expanded={menu}
                            aria-label={`${C.chips.collection} ${collection.name}`}
                            className={CHIP}
                            style={chipStyle}
                            onClick={(e) => {
                                e.stopPropagation()
                                setMenu((m) => !m)
                            }}
                            data-slot='collection-chip'
                        >
                            <Glyph name='folder' size={13} color='var(--v2-ink)' />
                            <span style={faint}>{C.chips.collection}</span>
                            <b>{collection.name}</b>
                            <span style={faint}>{where}</span>
                            <Glyph name='chev' size={10} color='var(--v2-faint)' />
                        </button>
                        {menu ? (
                            <div
                                role='menu'
                                aria-label={C.menu}
                                className='absolute top-full right-0 z-10 mt-1 flex min-w-52 flex-col rounded-[10px] border p-1 text-[13px]'
                                style={{
                                    borderColor: 'var(--v2-rule)',
                                    background: 'var(--v2-paper)',
                                    boxShadow: '0 12px 30px rgba(0,0,0,0.18)',
                                }}
                                data-slot='collection-menu'
                            >
                                <button type='button' role='menuitem' className='rounded-[7px] px-3 py-1.5 text-left' onClick={() => void run({ kind: 'refresh' })}>
                                    {C.refresh}
                                </button>
                                {TOOLS.map((t) => (
                                    <button key={t} type='button' role='menuitem' className='rounded-[7px] px-3 py-1.5 text-left' aria-haspopup='dialog' onClick={() => setTool(t)}>
                                        {C.tools[t]}
                                    </button>
                                ))}
                                <button type='button' role='menuitem' className='rounded-[7px] px-3 py-1.5 text-left' onClick={() => void close({ kind: 'collection' })}>
                                    {C.closeCollection}
                                </button>
                            </div>
                        ) : null}
                    </span>
                    <label className={CHIP} style={chipStyle} data-slot='environment-chip'>
                        <span
                            aria-hidden
                            className='rounded-full'
                            style={{
                                width: 8,
                                height: 8,
                                background: environment ? 'var(--v2-ok)' : 'var(--v2-faint)',
                            }}
                        />
                        <span style={faint}>{C.chips.environment}</span>
                        <select
                            aria-label={C.chips.environment}
                            value={collection.environment ?? ''}
                            onChange={(e) =>
                                void run({
                                    kind: 'selectEnvironment',
                                    name: e.target.value === '' ? null : e.target.value,
                                })
                            }
                            className='min-h-0 border-0 bg-transparent font-bold'
                            style={{
                                border: 0,
                                minHeight: 0,
                                height: 24,
                                padding: 0,
                                borderRadius: 0,
                                background: 'transparent',
                                color: 'var(--v2-ink)',
                            }}
                        >
                            <option value=''>{C.chips.noEnvironment}</option>
                            {collection.environments.map((e) => (
                                <option key={e.name} value={e.name}>
                                    {e.name}
                                </option>
                            ))}
                        </select>
                        {environment ? <span style={faint}>{C.chips.envOrigin[environment.origin] ?? environment.origin}</span> : null}
                    </label>
                </div>
            ) : null}
        </div>
    )

    const alerts = (
        <>
            {state.network !== 'live' ? (
                <p className='text-[12px]' style={faint} data-slot='workbench-network'>
                    {state.network === 'loopbackOnly' ? C.network.loopbackOnly : C.network.none}
                </p>
            ) : null}
            {state.failure ? (
                <p role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-block-bg)' }}>
                    {C.privateFailure(state.failure)}
                </p>
            ) : null}
            {message && collection ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-block)' }} data-slot='workbench-message'>
                    {message}
                </p>
            ) : null}
            {collection?.issues.length ? (
                <div role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-flag-bg)' }} data-slot='workbench-issues'>
                    <p>{C.issues(collection.issues.length, collection.complete)}</p>
                    <ul className='mt-1 list-disc pl-5 text-[12px]'>
                        {collection.issues.slice(0, 10).map((i) => (
                            <li key={`${i.relative}:${i.kind}`}>{`${i.relative}: ${i.kind}`}</li>
                        ))}
                    </ul>
                </div>
            ) : null}
            {collection?.unsaved ? (
                <p role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-block-bg)' }}>
                    {C.unsaved(collection.unsaved)}
                </p>
            ) : null}
        </>
    )

    if (!state.available) {
        return (
            <section
                aria-labelledby='workbench-heading'
                data-slot='api-client-v2'
                className='flex min-h-0 min-w-0 flex-grow flex-col'
                style={{
                    margin: 'calc(-1 * var(--v2-page-top)) calc(-1 * var(--v2-page-x)) -28px',
                }}
            >
                {band}
                <p role='alert' className='text-[13px]' style={{ padding: '20px var(--v2-page-x)' }}>
                    {C.unavailable}
                </p>
            </section>
        )
    }

    return (
        <section
            aria-labelledby='workbench-heading'
            data-slot='api-client-v2'
            className='flex min-h-0 min-w-0 flex-grow flex-col'
            style={{
                margin: 'calc(-1 * var(--v2-page-top)) calc(-1 * var(--v2-page-x)) -28px',
            }}
        >
            {band}
            {!collection ? (
                <div className='flex flex-col' style={{ gap: 12, padding: '20px var(--v2-page-x)' }}>
                    {alerts}
                    {state.loaded ? (
                        <Start state={state} run={run} message={message} offerNew={offerNew} />
                    ) : (
                        <p role='status' className='text-[13px]' style={soft}>
                            {C.opening}
                        </p>
                    )}
                </div>
            ) : (
                <>
                    <div className='flex min-h-0 flex-grow' data-slot='api-body'>
                        <RequestTree
                            collection={collection}
                            onOpen={(relative) => void run({ kind: 'openRequest', relative }).then(show)}
                            onCreate={async (kind, relative) => {
                                const r = await run(kind === 'request' ? { kind: 'newRequest', relative } : { kind: 'newFolder', relative })
                                if (kind === 'request') show(r)
                                return r.kind === 'document' || r.kind === 'done'
                            }}
                        />
                        <div className='flex min-h-0 min-w-0 flex-grow flex-col' data-slot='api-main'>
                            <div
                                role='tablist'
                                aria-label={C.openRequests}
                                className='flex shrink-0 flex-wrap items-end'
                                style={{
                                    minHeight: 36,
                                    gap: 2,
                                    padding: '0 8px',
                                    borderBottom: '1px solid var(--v2-rule)',
                                    background: 'var(--v2-side)',
                                }}
                                data-slot='workbench-tabs'
                            >
                                {collection.tabs.map((t) => {
                                    const name = requestName(t.relative)
                                    const on = t.id === collection.selected
                                    const marks = [t.new ? 'new' : t.draft ? 'unsaved' : '', t.conflict ? `${t.conflict} on disk` : '', t.interrupted ? 'interrupted save' : '']
                                        .filter(Boolean)
                                        .join(', ')
                                    return (
                                        <span
                                            key={t.id}
                                            className='flex items-center rounded-t-[7px] border border-b-0'
                                            style={{
                                                height: 30,
                                                maxWidth: 240,
                                                borderColor: on ? 'var(--v2-rule)' : 'transparent',
                                                background: on ? 'var(--v2-bg)' : 'transparent',
                                            }}
                                        >
                                            <button
                                                type='button'
                                                role='tab'
                                                aria-selected={on}
                                                aria-label={`${t.method} ${name}${marks ? ` (${marks})` : ''}`}
                                                onClick={() => void run({ kind: 'selectTab', id: t.id })}
                                                className='flex min-h-0 min-w-0 items-center text-[12px] whitespace-nowrap'
                                                style={{
                                                    gap: 7,
                                                    height: 28,
                                                    padding: '0 6px 0 12px',
                                                    border: 0,
                                                    background: 'transparent',
                                                    color: 'var(--v2-ink)',
                                                }}
                                            >
                                                <span className='font-mono text-[11px] font-bold' style={{ color: methodColor(t.method) }}>
                                                    {t.method}
                                                </span>
                                                <span className='truncate'>{name}</span>
                                                {t.draft || t.new ? (
                                                    <span
                                                        aria-hidden
                                                        className='shrink-0 rounded-full'
                                                        style={{
                                                            width: 6,
                                                            height: 6,
                                                            background: 'var(--v2-acc)',
                                                        }}
                                                    />
                                                ) : null}
                                                {t.conflict || t.interrupted ? <Glyph name='alert' size={11} color='var(--v2-flag)' /> : null}
                                            </button>
                                            <button
                                                type='button'
                                                aria-label={C.closeTab(name)}
                                                onClick={() => void close({ kind: 'tab', id: t.id, name })}
                                                className='grid size-6 min-h-0 place-items-center rounded-[5px] text-[13px]'
                                                style={{
                                                    border: 0,
                                                    background: 'transparent',
                                                    ...faint,
                                                }}
                                            >
                                                ×
                                            </button>
                                        </span>
                                    )
                                })}
                            </div>
                            {alerts !== null ? (
                                <div className='flex flex-col empty:hidden' style={{ gap: 8, padding: '0 20px' }}>
                                    {alerts}
                                </div>
                            ) : null}
                            {notes.length ? (
                                <div className='flex flex-col text-[13px]' style={{ gap: 4, margin: '12px 20px 0', padding: '12px 14px', borderRadius: 10, background: 'var(--v2-acc-soft)' }} data-slot='workbench-notes'>
                                    <p className='font-semibold'>{C.notes}</p>
                                    <ul className='list-disc pl-5'>
                                        {notes.map((n, i) => (
                                            <li key={i}>{NOTE_TEXT(n)}</li>
                                        ))}
                                    </ul>
                                </div>
                            ) : null}
                            {doc && selectedTab && doc.id === selectedTab.id ? (
                                <RequestEditor
                                    key={`${doc.id}:${doc.revision}:${doc.relative}`}
                                    initial={doc}
                                    call={call}
                                    schemas={collection.schemas}
                                    historyKey={`${collection.id}:${state.running.length}`}
                                    onClosed={() => setDoc(null)}
                                    register={(handle) => editors.current.set(doc.id, handle)}
                                    context={snapshot.active?.context ?? null}
                                    running={state.running.filter((r) => r.request === doc.id)}
                                    review={state.review?.request === doc.id ? state.review : null}
                                />
                            ) : collection.tabs.length === 0 ? (
                                <p className='text-[13px]' style={{ padding: '20px', ...soft }} data-slot='workbench-empty'>
                                    {collection.requests.length === 0 ? C.noRequests : C.selectRequest}
                                </p>
                            ) : null}
                            <div
                                className='flex shrink-0 items-center text-[12px]'
                                style={{
                                    height: 26,
                                    gap: 14,
                                    padding: '0 14px',
                                    borderTop: '1px solid var(--v2-rule)',
                                    background: 'var(--v2-side)',
                                    ...faint,
                                }}
                                data-slot='api-footer'
                            >
                                <span className='flex items-center' style={{ gap: 5 }}>
                                    <span
                                        aria-hidden
                                        className='rounded-full'
                                        style={{
                                            width: 6,
                                            height: 6,
                                            background: state.busy ? 'var(--v2-flag)' : 'var(--v2-ok)',
                                        }}
                                    />
                                    {state.busy ? C.working : C.footer.ready}
                                </span>
                                <span className='ml-auto'>{C.footer.requests(collection.requests.length)}</span>
                                {collection.archives ? <span>{C.footer.archives(collection.archives)}</span> : null}
                            </div>
                        </div>
                    </div>
                    {tool ? (
                        <ToolsSheet
                            tool={tool}
                            state={state}
                            collection={collection}
                            run={run}
                            onClose={() => setTool(null)}
                            onCreated={(r) => {
                                show(r)
                                setTool(null)
                            }}
                        />
                    ) : null}
                </>
            )}
            {closing
                ? createPortal(
                      <Modal label={closing.kind === 'tab' ? C.closing.tab(closing.name) : C.closing.collection} width={480} onClose={() => setClosing(null)} data-slot='workbench-closing-dialog'>
                          <div
                              role='alertdialog'
                              aria-label={closing.kind === 'tab' ? C.closing.tab(closing.name) : C.closing.collection}
                              className='flex flex-col text-[13px]'
                              style={{ gap: 12 }}
                              data-slot='workbench-closing'
                          >
                              <h2 className='text-[17px] font-bold'>{closing.kind === 'tab' ? C.closing.tab(closing.name) : C.closing.collection}</h2>
                              <p>{C.closing.text}</p>
                              <div className='flex justify-end' style={{ gap: 8 }}>
                                  <Btn onClick={() => setClosing(null)}>{C.closing.keep}</Btn>
                                  <Btn tier='primary' onClick={() => void close(closing, true)}>
                                      {C.closing.discard}
                                  </Btn>
                              </div>
                          </div>
                      </Modal>,
                      document.body
                  )
                : null}
        </section>
    )
})

/** No collection open: open, create, the built-in, recents and the v1 profile import. */
function Start({ state, run, message, offerNew }: { state: WorkbenchState; run: Run; message: string; offerNew: boolean }) {
    const [chosen, setChosen] = useState<number[]>([])
    return (
        <div className='flex flex-col' style={{ gap: 14, maxWidth: 760 }} data-slot='workbench-start'>
            <p className='text-[14px]'>{C.start.none}</p>
            {message ? (
                <div
                    role='alert'
                    className='flex flex-col rounded-[10px] text-[13px]'
                    style={{
                        gap: 8,
                        padding: '12px 14px',
                        background: 'var(--v2-flag-bg)',
                    }}
                    data-slot='workbench-message'
                >
                    <p>{message}</p>
                    {offerNew ? (
                        <Btn className='self-start' onClick={() => void run({ kind: 'createCollection' })}>
                            {C.start.create}
                        </Btn>
                    ) : null}
                </div>
            ) : null}
            <div className='flex flex-wrap' style={{ gap: 8 }}>
                <Btn tier='primary' onClick={() => void run({ kind: 'openFolder' })}>
                    {C.start.open}
                </Btn>
                <Btn onClick={() => void run({ kind: 'createCollection' })}>{C.start.create}</Btn>
                <Btn onClick={() => void run({ kind: 'openBuiltIn' })}>{C.start.builtIn}</Btn>
            </div>
            <p className='text-[12px]' style={faint}>
                {C.start.builtInNote}
            </p>
            {state.recent.length ? (
                <section aria-label={C.start.recent} className='flex flex-col' style={{ gap: 6 }}>
                    <h2 className='text-[15px] font-semibold'>Recent</h2>
                    {state.recent.map((r) => (
                        <Btn key={r.index} tier='tertiary' className='self-start' onClick={() => void run({ kind: 'openRecent', index: r.index })}>
                            {`${r.name} (${r.folder})`}
                        </Btn>
                    ))}
                </section>
            ) : null}
            {state.adoption ? (
                <section
                    aria-label='v1 API Client profile'
                    className='flex flex-col rounded-[12px] border text-[13px]'
                    style={{
                        gap: 8,
                        padding: 16,
                        borderColor: 'var(--v2-rule)',
                        background: 'var(--v2-paper)',
                    }}
                    data-slot='workbench-adoption'
                >
                    <h2 className='text-[15px] font-semibold'>{C.start.adoption}</h2>
                    <p style={soft}>{C.start.adoptionLead}</p>
                    {state.adoption.length === 0 ? <p>{C.start.adoptionNone}</p> : null}
                    {state.adoption.map((c) => (
                        <label key={c.index} className='flex items-center' style={{ gap: 8 }}>
                            <input type='checkbox' checked={chosen.includes(c.index)} onChange={(e) => setChosen((x) => (e.target.checked ? [...x, c.index] : x.filter((i) => i !== c.index)))} />
                            {`${c.name} (${c.folder}) · ${c.environments.length} private environments${c.skipped ? ` · ${c.skipped} left out` : ''}`}
                        </label>
                    ))}
                    <div className='flex flex-wrap' style={{ gap: 8 }}>
                        <Btn tier='primary' disabled={chosen.length === 0} onClick={() => void run({ kind: 'adopt', selected: chosen })}>
                            {C.start.importSelected}
                        </Btn>
                        <Btn onClick={() => void run({ kind: 'dismissAdoption' })}>{C.start.startEmpty}</Btn>
                    </div>
                </section>
            ) : !state.adoptionSettled ? (
                <Btn tier='tertiary' className='self-start' onClick={() => void run({ kind: 'readAdoption' })}>
                    {C.start.readAdoption}
                </Btn>
            ) : null}
        </div>
    )
}
