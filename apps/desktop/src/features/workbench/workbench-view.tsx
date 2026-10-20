import { forwardRef, useEffect, useId, useMemo, useRef, useState } from 'react'
import { Button, Callout, Input } from '@ai-security-academy/presentation'
import { AlertTriangle, FolderOpen, Info, X } from 'lucide-react'
import type {
    CollectionView,
    DocumentView,
    GitView,
    ImportNote,
    ShellSnapshot,
    WorkbenchView as WorkbenchState,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import type { Dispatch } from '../tenants/dispatch'
import { DocumentEditor, type EditorHandle, SELECT, TEXTAREA, requestName } from './document-editor'
import { History } from './request-send'
import { type ObserveWorkbench, type WorkbenchCall, resultText, workbenchCall } from './workbench-call'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveWorkbench
}

type Tool = 'requests' | 'history' | 'environments' | 'secrets' | 'schemas' | 'import' | 'git'
const TOOLS: [Tool, string][] = [
    ['requests', 'Requests'],
    ['history', 'History'],
    ['environments', 'Environments'],
    ['secrets', 'Secrets'],
    ['schemas', 'Schemas'],
    ['import', 'Import'],
    ['git', 'Git'],
]

const NOTE_TEXT = (note: ImportNote): string => {
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

/**
 * The API Client (C14): v1's local-first collections, edited here and
 * saved to their folders. Rust owns each document's revision, whether a
 * snapshot can be written, conflicts and the private drafts on this Mac;
 * nothing in this view sends a request.
 */
export const WorkbenchView = forwardRef<HTMLHeadingElement, Props>(function WorkbenchView({ snapshot, dispatch, observe }, ref) {
    const ids = useId()
    const call = useMemo(() => workbenchCall(dispatch, observe), [dispatch, observe])
    const state = snapshot.workbench
    const collection = state.collection
    const [message, setMessage] = useState('')
    const [tool, setTool] = useState<Tool>('requests')
    const [doc, setDoc] = useState<DocumentView | null>(null)
    const [notes, setNotes] = useState<ImportNote[]>([])
    const loading = useRef<string | null>(null)
    // Each editor's close handle (see DocumentEditor): closing waits for it.
    const editors = useRef(new Map<string, EditorHandle>())
    const [closing, setClosing] = useState<Closing | null>(null)

    const run = async (request: Parameters<WorkbenchCall>[0]) => {
        setMessage('')
        const r = await call(request)
        const text = resultText(r)
        if (text) setMessage(text)
        return r
    }

    // Closing protects the latest typing first. If that fails, or Rust
    // still sees unprotected edits, the operator chooses: keep editing, or
    // close and discard them.
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

    // The selected tab's document is read when it is not the one shown.
    const selected = collection?.selected ?? null
    const selectedTab = collection?.tabs.find((t) => t.id === selected) ?? null
    useEffect(() => {
        if (!selectedTab || doc?.id === selectedTab.id || loading.current === selectedTab.id) return
        loading.current = selectedTab.id
        void call({ kind: 'openRequest', relative: selectedTab.relative }).then((r) => {
            loading.current = null
            if (r.kind === 'document') setDoc(r.document)
        })
    }, [selectedTab, doc, call])
    useEffect(() => {
        if (!collection) setDoc(null)
    }, [collection])
    // The private profile (recent collections, adoption) is read on first
    // showing.
    const loaded = state.loaded
    useEffect(() => {
        if (!loaded) void call({ kind: 'load' })
    }, [loaded, call])

    const heading = <PageHeading ref={ref} id='workbench-heading' kicker='API Client' title='API Client' />
    if (!state.available) {
        return (
            <section aria-labelledby='workbench-heading' data-slot='workbench'>
                {heading}
                <Callout tone='danger' icon={AlertTriangle}>
                    The API Client's private folder is not available in this build.
                </Callout>
            </section>
        )
    }
    return (
        <section aria-labelledby='workbench-heading' data-slot='workbench' className='flex flex-col gap-5'>
            {heading}
            <p className='text-academy-ink-soft -mt-4 text-[13px]'>
                Collections live in your folders (Git-friendly TOML). Drafts, private environments, secret values, history and cookies stay on this
                Mac. Requests with tenant credentials go only to known operations, and changes need your review.
            </p>
            {state.network !== 'live' ? (
                <p className='text-academy-ink-faint -mt-3 text-[12px]' data-slot='workbench-network'>
                    {state.network === 'loopbackOnly' ? 'This build sends API Client requests to loopback addresses only.' : 'This build does not send API Client requests.'}
                </p>
            ) : null}
            {state.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    Private API Client data is unavailable ({state.failure}). Drafts cannot be protected until it opens.
                </Callout>
            ) : null}
            {message ? (
                <p role='alert' className='text-[13px]' data-slot='workbench-message'>
                    {message}
                </p>
            ) : null}
            {closing ? (
                <Callout tone='warning' icon={AlertTriangle} role='alert' data-slot='workbench-closing'>
                    <p className='text-academy-ink font-semibold'>
                        {closing.kind === 'tab' ? `Close ${closing.name} without your latest edits?` : 'Close the collection without your latest edits?'}
                    </p>
                    <p>The latest edits could not be kept on this Mac. Keep editing to try again, or close and discard them.</p>
                    <div className='mt-3 flex flex-wrap gap-2'>
                        <Button type='button' tier='secondary' onClick={() => setClosing(null)}>
                            Keep editing
                        </Button>
                        <Button type='button' tier='secondary' onClick={() => void close(closing, true)}>
                            Close and discard edits
                        </Button>
                    </div>
                </Callout>
            ) : null}
            {state.busy ? (
                <p role='status' className='text-academy-ink-faint text-[12px]'>
                    Working…
                </p>
            ) : null}
            {!collection ? (
                state.loaded ? (
                    <NoCollection state={state} run={run} />
                ) : (
                    <p role='status' className='text-academy-ink-soft text-[13px]'>
                        Opening the API Client…
                    </p>
                )
            ) : (
                <>
                    <CollectionBar collection={collection} run={run} onClose={() => void close({ kind: 'collection' })} />
                    <div role='tablist' aria-label='API Client tools' className='flex flex-wrap gap-1'>
                        {TOOLS.map(([id, label]) => (
                            <Button
                                key={id}
                                type='button'
                                role='tab'
                                size='sm'
                                aria-selected={tool === id}
                                tier={tool === id ? 'secondary' : 'tertiary'}
                                onClick={() => setTool(id)}
                            >
                                {label}
                            </Button>
                        ))}
                    </div>
                    {tool === 'requests' ? (
                        <div className='grid gap-5 lg:grid-cols-[18rem_1fr]'>
                            <Navigator collection={collection} run={run} onOpen={show} />
                            <div className='flex flex-col gap-3'>
                                <Tabs collection={collection} run={run} onClose={(id, name) => void close({ kind: 'tab', id, name })} />
                                {notes.length ? (
                                    <Callout tone='info' icon={Info} data-slot='workbench-notes'>
                                        <p className='text-academy-ink font-semibold'>Conversion report</p>
                                        <ul className='list-disc pl-5'>
                                            {notes.map((n, i) => (
                                                <li key={i}>{NOTE_TEXT(n)}</li>
                                            ))}
                                        </ul>
                                    </Callout>
                                ) : null}
                                {doc && selectedTab && doc.id === selectedTab.id ? (
                                    <DocumentEditor
                                        key={`${doc.id}:${doc.revision}:${doc.relative}`}
                                        initial={doc}
                                        call={call}
                                        schemas={collection.schemas}
                                        onClosed={() => setDoc(null)}
                                        register={(handle) => editors.current.set(doc.id, handle)}
                                        context={snapshot.active?.context ?? null}
                                        running={state.running.filter((r) => r.request === doc.id)}
                                        review={state.review?.request === doc.id ? state.review : null}
                                    />
                                ) : collection.tabs.length === 0 ? (
                                    <p className='text-academy-ink-soft text-[13px]'>
                                        {collection.requests.length === 0
                                            ? 'This collection has no requests yet.'
                                            : 'Select a request, or create one with New request.'}
                                    </p>
                                ) : null}
                            </div>
                        </div>
                    ) : tool === 'history' ? (
                        <History call={call} revision={`${collection.id}:${state.running.length}`} />
                    ) : tool === 'environments' ? (
                        <Environments collection={collection} run={run} ids={ids} />
                    ) : tool === 'secrets' ? (
                        <Secrets collection={collection} run={run} ids={ids} />
                    ) : tool === 'schemas' ? (
                        <Schemas collection={collection} run={run} ids={ids} />
                    ) : tool === 'import' ? (
                        <Import
                            state={state}
                            run={run}
                            ids={ids}
                            onCreated={(r) => {
                                show(r)
                                setTool('requests')
                            }}
                        />
                    ) : (
                        <Git git={state.git} run={run} />
                    )}
                </>
            )}
        </section>
    )
})

type Run = (request: Parameters<WorkbenchCall>[0]) => ReturnType<WorkbenchCall>
type Closing = { kind: 'tab'; id: string; name: string } | { kind: 'collection' }

function NoCollection({ state, run }: { state: WorkbenchState; run: Run }) {
    const [chosen, setChosen] = useState<number[]>([])
    return (
        <div className='flex flex-col gap-4' data-slot='workbench-start'>
            <p className='text-[14px]'>No collection is open.</p>
            <div className='flex flex-wrap gap-2'>
                <Button type='button' tier='primary' onClick={() => void run({ kind: 'openFolder' })}>
                    <FolderOpen aria-hidden className='size-4' />
                    Open collection…
                </Button>
                <Button type='button' tier='secondary' onClick={() => void run({ kind: 'createCollection' })}>
                    New collection…
                </Button>
                <Button type='button' tier='secondary' onClick={() => void run({ kind: 'openBuiltIn' })}>
                    Open Strata Cloud Manager
                </Button>
            </div>
            <p className='text-academy-ink-faint text-[12px]'>
                Strata Cloud Manager: the built-in collection of 193 SCM and Prisma AIRS request specifications, kept in this app's private folder.
            </p>
            {state.recent.length ? (
                <section aria-label='Recent collections' className='flex flex-col gap-1'>
                    <h2 className='text-[15px] font-semibold'>Recent</h2>
                    {state.recent.map((r) => (
                        <Button key={r.index} type='button' tier='tertiary' className='self-start' onClick={() => void run({ kind: 'openRecent', index: r.index })}>
                            {`${r.name} (${r.folder})`}
                        </Button>
                    ))}
                </section>
            ) : null}
            {state.adoption ? (
                <section aria-label='v1 API Client profile' className='border-academy-rule rounded-academy-md flex flex-col gap-2 border p-4 text-[13px]' data-slot='workbench-adoption'>
                    <h2 className='text-[15px] font-semibold'>Import your v1 API Client profile</h2>
                    <p className='text-academy-ink-soft'>
                        Recent collections and their private environments are copied. The v1 profile is only read. Secret values, drafts, history and
                        cookies do not transfer: store secret values again here.
                    </p>
                    {state.adoption.length === 0 ? <p>The v1 profile lists no collections that still exist.</p> : null}
                    {state.adoption.map((c) => (
                        <label key={c.index} className='flex items-center gap-2'>
                            <input
                                type='checkbox'
                                checked={chosen.includes(c.index)}
                                onChange={(e) => setChosen((x) => (e.target.checked ? [...x, c.index] : x.filter((i) => i !== c.index)))}
                            />
                            {`${c.name} (${c.folder}) · ${c.environments.length} private environments${c.skipped ? ` · ${c.skipped} left out` : ''}`}
                        </label>
                    ))}
                    <div className='flex flex-wrap gap-2'>
                        <Button type='button' tier='primary' disabled={chosen.length === 0} onClick={() => void run({ kind: 'adopt', selected: chosen })}>
                            Import selected
                        </Button>
                        <Button type='button' tier='secondary' onClick={() => void run({ kind: 'dismissAdoption' })}>
                            Start empty
                        </Button>
                    </div>
                </section>
            ) : !state.adoptionSettled ? (
                <Button type='button' tier='tertiary' className='self-start' onClick={() => void run({ kind: 'readAdoption' })}>
                    Import v1 API Client profile…
                </Button>
            ) : null}
        </div>
    )
}

function CollectionBar({ collection, run, onClose }: { collection: CollectionView; run: Run; onClose: () => void }) {
    return (
        <div className='flex flex-wrap items-end gap-3' data-slot='workbench-collection'>
            <div className='flex flex-col'>
                <h2 className='text-[17px] font-semibold'>{collection.name}</h2>
                <span className='text-academy-ink-faint text-[12px]'>
                    {`${collection.builtIn ? 'Built-in collection' : collection.folder} · ${collection.requests.length} requests${
                        collection.archives ? ` · ${collection.archives} earlier versions kept privately` : ''
                    }`}
                </span>
            </div>
            <label className='flex flex-col gap-1 text-[13px] font-medium'>
                Environment
                <select
                    className={SELECT}
                    value={collection.environment ?? ''}
                    onChange={(e) => void run({ kind: 'selectEnvironment', name: e.target.value === '' ? null : e.target.value })}
                >
                    <option value=''>No environment</option>
                    {collection.environments.map((e) => (
                        <option key={e.name} value={e.name}>
                            {`${e.name} (${e.origin === 'both' ? 'shared + local' : e.origin})`}
                        </option>
                    ))}
                </select>
            </label>
            <Button type='button' tier='secondary' size='sm' onClick={() => void run({ kind: 'refresh' })}>
                Refresh
            </Button>
            <Button type='button' tier='tertiary' size='sm' onClick={onClose}>
                Close collection
            </Button>
            {collection.issues.length ? (
                <Callout tone='warning' icon={AlertTriangle} role='alert' className='w-full' data-slot='workbench-issues'>
                    <p>
                        {collection.complete
                            ? `Some files need attention (${collection.issues.length} listed). Requests sharing an identity are not shown, and their files are not changed.`
                            : `This collection has problems to fix before files can change (${collection.issues.length} listed). Requests with problems are not shown.`}
                    </p>
                    <ul className='mt-1 list-disc pl-5 text-[12px]'>
                        {collection.issues.slice(0, 10).map((i) => (
                            <li key={`${i.relative}:${i.kind}`}>{`${i.relative}: ${i.kind}`}</li>
                        ))}
                    </ul>
                </Callout>
            ) : null}
            {collection.unsaved ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' className='w-full'>
                    Drafts could not be protected on this Mac ({collection.unsaved}). Save or keep the app open until this clears.
                </Callout>
            ) : null}
        </div>
    )
}

function Navigator({ collection, run, onOpen }: { collection: CollectionView; run: Run; onOpen: (r: Awaited<ReturnType<WorkbenchCall>>) => void }) {
    const ids = useId()
    const [filter, setFilter] = useState('')
    const [path, setPath] = useState('')
    const shown = collection.requests.filter((r) => `${r.method} ${r.relative} ${r.url}`.toLowerCase().includes(filter.toLowerCase()))
    return (
        <nav aria-label='Requests' className='flex flex-col gap-3' data-slot='workbench-navigator'>
            <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-find`}>
                Find a request
                <Input id={`${ids}-find`} value={filter} onChange={(e) => setFilter(e.target.value)} autoComplete='off' />
            </label>
            <ul className='flex max-h-[28rem] flex-col gap-0.5 overflow-auto'>
                {shown.slice(0, 300).map((r) => (
                    <li key={r.id}>
                        <Button
                            type='button'
                            tier='tertiary'
                            size='sm'
                            className='w-full justify-start text-left'
                            aria-label={`Open ${r.method} ${r.relative}`}
                            onClick={() => void run({ kind: 'openRequest', relative: r.relative }).then(onOpen)}
                        >
                            <span className='w-12 shrink-0 font-mono text-[11px]'>{r.method === 'DELETE' ? 'DEL' : r.method === 'OPTIONS' ? 'OPT' : r.method}</span>
                            <span className='truncate'>{r.relative.replace(/\.request\.toml$/, '')}</span>
                        </Button>
                    </li>
                ))}
            </ul>
            {shown.length === 0 ? <p className='text-academy-ink-soft text-[13px]'>{filter ? 'No request matches.' : 'No requests yet.'}</p> : null}
            {shown.length > 300 ? <p className='text-academy-ink-faint text-[12px]'>Showing 300 of {shown.length}; refine the search.</p> : null}
            <form
                className='flex flex-col gap-1'
                onSubmit={(e) => {
                    e.preventDefault()
                    void run({ kind: 'newRequest', relative: path }).then((r) => {
                        if (r.kind === 'document') setPath('')
                        onOpen(r)
                    })
                }}
            >
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-new`}>
                    New request (folder/name)
                    <Input id={`${ids}-new`} value={path} onChange={(e) => setPath(e.target.value)} autoComplete='off' spellCheck={false} aria-describedby={`${ids}-new-hint`} />
                </label>
                <p id={`${ids}-new-hint`} className='text-academy-ink-faint text-[12px]'>
                    The request stays a private draft until you Save it.
                </p>
                <Button type='submit' tier='secondary' size='sm' className='self-start' disabled={path.trim() === ''}>
                    New request
                </Button>
            </form>
            <form
                className='flex flex-col gap-1'
                onSubmit={(e) => {
                    e.preventDefault()
                    const data = new FormData(e.currentTarget)
                    void run({ kind: 'newFolder', relative: String(data.get('folder') ?? '') })
                }}
            >
                <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-folder`}>
                    New folder
                    <Input id={`${ids}-folder`} name='folder' autoComplete='off' spellCheck={false} />
                </label>
                <Button type='submit' tier='tertiary' size='sm' className='self-start'>
                    Create folder
                </Button>
            </form>
        </nav>
    )
}

function Tabs({ collection, run, onClose }: { collection: CollectionView; run: Run; onClose: (id: string, name: string) => void }) {
    if (collection.tabs.length === 0) return null
    return (
        <div role='tablist' aria-label='Open requests' className='flex flex-wrap gap-1' data-slot='workbench-tabs'>
            {collection.tabs.map((t) => {
                const name = requestName(t.relative)
                const marks = [t.new ? 'new' : t.draft ? 'unsaved' : '', t.conflict ? `${t.conflict} on disk` : '', t.interrupted ? 'interrupted save' : '']
                    .filter(Boolean)
                    .join(', ')
                return (
                    <span key={t.id} className='border-academy-rule rounded-academy-sm flex items-center border'>
                        <Button
                            type='button'
                            role='tab'
                            size='sm'
                            tier={t.id === collection.selected ? 'secondary' : 'tertiary'}
                            aria-selected={t.id === collection.selected}
                            aria-label={`${t.method} ${name}${marks ? ` (${marks})` : ''}`}
                            onClick={() => void run({ kind: 'selectTab', id: t.id })}
                        >
                            <span className='font-mono text-[11px]'>{t.method}</span> {name}
                            {t.draft || t.new ? <span aria-hidden> •</span> : null}
                            {t.conflict || t.interrupted ? <AlertTriangle aria-hidden className='size-3' /> : null}
                        </Button>
                        <Button type='button' size='sm' tier='tertiary' aria-label={`Close tab ${name}`} onClick={() => onClose(t.id, name)}>
                            <X aria-hidden className='size-3' />
                        </Button>
                    </span>
                )
            })}
        </div>
    )
}

function Environments({ collection, run, ids }: { collection: CollectionView; run: Run; ids: string }) {
    const [open, setOpen] = useState<{ name: string; private: boolean; text: string; revision: string | null } | null>(null)
    const [name, setName] = useState('')
    const load = async (env: string, priv: boolean) => {
        const r = await run({ kind: 'openEnvironment', name: env, private: priv })
        if (r.kind === 'environment') setOpen({ name: r.name, private: r.private, text: r.text, revision: r.revision })
    }
    return (
        <section aria-label='Environments' className='flex flex-col gap-3 text-[13px]' data-slot='workbench-environments'>
            <p className='text-academy-ink-soft'>
                Shared environments are TOML files saved in the collection. Private environments live only on this Mac and are never committed.
            </p>
            {collection.environments.length === 0 ? <p>No environments yet.</p> : null}
            <ul className='flex flex-col gap-1'>
                {collection.environments.map((e) => (
                    <li key={e.name} className='flex flex-wrap items-center gap-2'>
                        <span className='font-medium'>{e.name}</span>
                        <span className='text-academy-ink-faint'>{`(${e.origin === 'both' ? 'shared + local' : e.origin})`}</span>
                        {e.origin !== 'local' ? (
                            <Button type='button' size='sm' tier='tertiary' onClick={() => void load(e.name, false)}>
                                {`Edit shared ${e.name}`}
                            </Button>
                        ) : null}
                        <Button type='button' size='sm' tier='tertiary' onClick={() => void load(e.name, true)}>
                            {`Edit private ${e.name}`}
                        </Button>
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap items-end gap-2'>
                <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-env`}>
                    Environment name
                    <Input id={`${ids}-env`} value={name} onChange={(e) => setName(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Button type='button' size='sm' tier='secondary' disabled={!name} onClick={() => void load(name, false)}>
                    New shared environment
                </Button>
                <Button type='button' size='sm' tier='secondary' disabled={!name} onClick={() => void load(name, true)}>
                    New private environment
                </Button>
                <Button type='button' size='sm' tier='tertiary' onClick={() => void load('_defaults', true)}>
                    Private collection defaults
                </Button>
            </div>
            {open ? (
                <form
                    aria-label={`${open.private ? 'Private' : 'Shared'} environment ${open.name}`}
                    className='flex flex-col gap-2'
                    onSubmit={(e) => {
                        e.preventDefault()
                        void run({ kind: 'saveEnvironment', name: open.name, private: open.private, revision: open.revision, text: open.text }).then((r) => {
                            if (r.kind === 'environment') setOpen({ name: r.name, private: r.private, text: r.text, revision: r.revision })
                        })
                    }}
                >
                    <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-env-text`}>
                        {`${open.private ? 'Private' : 'Shared'} environment ${open.name}`}
                        <textarea id={`${ids}-env-text`} className={TEXTAREA} value={open.text} spellCheck={false} onChange={(e) => setOpen({ ...open, text: e.target.value })} />
                    </label>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='submit' size='sm' tier='primary'>
                            Save environment
                        </Button>
                        {open.private && open.revision ? (
                            <Button
                                type='button'
                                size='sm'
                                tier='tertiary'
                                onClick={() => void run({ kind: 'deleteEnvironment', name: open.name }).then((r) => r.kind === 'done' && setOpen(null))}
                            >
                                Delete private environment
                            </Button>
                        ) : null}
                        <Button type='button' size='sm' tier='tertiary' onClick={() => setOpen(null)}>
                            Close
                        </Button>
                    </div>
                </form>
            ) : null}
        </section>
    )
}

function Secrets({ collection, run, ids }: { collection: CollectionView; run: Run; ids: string }) {
    const [values, setValues] = useState<Record<string, string>>({})
    return (
        <section aria-label='Secrets' className='flex flex-col gap-3 text-[13px]' data-slot='workbench-secrets'>
            <p className='text-academy-ink-soft'>
                Files hold secret references only. Values are kept in the Keychain on this Mac, never shown again and never written to a collection.
            </p>
            {collection.secrets.length === 0 ? <p>No secret references are used yet.</p> : null}
            {collection.secrets.map((s, i) =>
                s.managed ? (
                    <p key={s.reference}>
                        <span className='font-mono'>{s.reference}</span>: managed by the tenant in Settings (resolved only by tenant authority when sending
                        arrives).
                    </p>
                ) : (
                    <form
                        key={s.reference}
                        className='flex flex-wrap items-end gap-2'
                        onSubmit={(e) => {
                            e.preventDefault()
                            const value = values[s.reference] ?? ''
                            setValues((v) => ({ ...v, [s.reference]: '' }))
                            void run({ kind: 'setSecret', reference: s.reference, value })
                        }}
                    >
                        <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-secret-${i}`}>
                            {`${s.reference} (${s.stored ? 'value stored' : 'no value'})`}
                            <Input
                                id={`${ids}-secret-${i}`}
                                type='password'
                                autoComplete='off'
                                value={values[s.reference] ?? ''}
                                onChange={(e) => setValues((v) => ({ ...v, [s.reference]: e.target.value }))}
                            />
                        </label>
                        <Button type='submit' size='sm' tier='secondary' disabled={!(values[s.reference] ?? '')}>
                            {`Store value for ${s.reference}`}
                        </Button>
                        {s.stored ? (
                            <Button type='button' size='sm' tier='tertiary' onClick={() => void run({ kind: 'removeSecret', reference: s.reference })}>
                                {`Remove value for ${s.reference}`}
                            </Button>
                        ) : null}
                    </form>
                )
            )}
        </section>
    )
}

function Schemas({ collection, run, ids }: { collection: CollectionView; run: Run; ids: string }) {
    const [open, setOpen] = useState<{ relative: string; text: string; revision: string | null } | null>(null)
    const [path, setPath] = useState('')
    return (
        <section aria-label='Schemas' className='flex flex-col gap-3 text-[13px]' data-slot='workbench-schemas'>
            <p className='text-academy-ink-soft'>JSON or YAML schema text, edited without validation or request generation. Associate one from a request.</p>
            <ul className='flex flex-col gap-1'>
                {collection.schemas.map((s) => (
                    <li key={s}>
                        <Button
                            type='button'
                            size='sm'
                            tier='tertiary'
                            onClick={() => void run({ kind: 'openSchema', relative: s }).then((r) => r.kind === 'schema' && setOpen(r.schema))}
                        >
                            {`Edit ${s}`}
                        </Button>
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap items-end gap-2'>
                <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-schema`}>
                    New schema path (.json, .yaml or .yml)
                    <Input id={`${ids}-schema`} value={path} onChange={(e) => setPath(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Button type='button' size='sm' tier='secondary' disabled={!path} onClick={() => setOpen({ relative: path, text: '', revision: null })}>
                    New schema
                </Button>
            </div>
            {open ? (
                <form
                    aria-label={`Schema ${open.relative}`}
                    className='flex flex-col gap-2'
                    onSubmit={(e) => {
                        e.preventDefault()
                        void run({ kind: 'saveSchema', relative: open.relative, revision: open.revision, text: open.text }).then(
                            (r) => r.kind === 'schema' && setOpen(r.schema)
                        )
                    }}
                >
                    <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-schema-text`}>
                        {`Schema ${open.relative}`}
                        <textarea id={`${ids}-schema-text`} className={TEXTAREA} value={open.text} spellCheck={false} onChange={(e) => setOpen({ ...open, text: e.target.value })} />
                    </label>
                    <div className='flex gap-2'>
                        <Button type='submit' size='sm' tier='primary'>
                            Save schema
                        </Button>
                        <Button type='button' size='sm' tier='tertiary' onClick={() => setOpen(null)}>
                            Close
                        </Button>
                    </div>
                </form>
            ) : null}
        </section>
    )
}

function Import({ state, run, ids, onCreated }: { state: WorkbenchState; run: Run; ids: string; onCreated: (r: Awaited<ReturnType<WorkbenchCall>>) => void }) {
    const [text, setText] = useState('')
    const [path, setPath] = useState('imported/request')
    const [chosen, setChosen] = useState<number[]>([])
    const [created, setCreated] = useState('')
    return (
        <section aria-label='Import' className='flex flex-col gap-4 text-[13px]' data-slot='workbench-import'>
            <form
                className='flex flex-col gap-2'
                onSubmit={(e) => {
                    e.preventDefault()
                    void run({ kind: 'importCurl', text, relative: path }).then((r) => {
                        if (r.kind === 'document') {
                            setText('')
                            onCreated(r)
                        }
                    })
                }}
            >
                <h2 className='text-[15px] font-semibold'>Paste one cURL command</h2>
                <p className='text-academy-ink-soft'>The command is parsed, never executed. Literal credentials become secret references you provide in Secrets.</p>
                <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-curl`}>
                    cURL command
                    <textarea id={`${ids}-curl`} className={TEXTAREA} value={text} spellCheck={false} onChange={(e) => setText(e.target.value)} />
                </label>
                <label className='flex flex-col gap-1 font-medium' htmlFor={`${ids}-curl-path`}>
                    Draft path (folder/name)
                    <Input id={`${ids}-curl-path`} value={path} onChange={(e) => setPath(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Button type='submit' size='sm' tier='primary' className='self-start' disabled={!text.trim()}>
                    Create draft
                </Button>
            </form>
            <div className='flex flex-col gap-2'>
                <h2 className='text-[15px] font-semibold'>Insomnia export</h2>
                <p className='text-academy-ink-soft'>Insomnia v5 collections (schema 5.1, YAML or JSON, up to 1 MiB). Scripts and plugins are not converted.</p>
                <Button type='button' size='sm' tier='secondary' className='self-start' onClick={() => void run({ kind: 'chooseInsomnia' })}>
                    Choose Insomnia export…
                </Button>
                {state.import ? (
                    <div className='flex flex-col gap-1' data-slot='workbench-import-preview'>
                        <p className='font-medium'>Conversion report</p>
                        {state.import.map((r) => (
                            <label key={r.index} className='flex items-center gap-2'>
                                <input
                                    type='checkbox'
                                    disabled={r.problem !== null}
                                    checked={chosen.includes(r.index)}
                                    onChange={(e) => setChosen((x) => (e.target.checked ? [...x, r.index] : x.filter((i) => i !== r.index)))}
                                />
                                {`${r.environment ? 'Environment' : 'Request'} ${r.label}${r.problem ? ` · not converted (${r.problem})` : ` · ${r.relative}`}`}
                            </label>
                        ))}
                        <div className='flex flex-wrap gap-2'>
                            <Button type='button' size='sm' tier='tertiary' onClick={() => setChosen(state.import?.filter((r) => r.problem === null).map((r) => r.index) ?? [])}>
                                Select all supported
                            </Button>
                            <Button
                                type='button'
                                size='sm'
                                tier='primary'
                                disabled={chosen.length === 0}
                                onClick={() =>
                                    void run({ kind: 'createImported', selected: chosen }).then((r) => {
                                        if (r.kind === 'created') {
                                            setChosen([])
                                            setCreated(`Created ${r.requests} request drafts and ${r.environments} environments; ${r.skipped} skipped.`)
                                        }
                                    })
                                }
                            >
                                Create selected
                            </Button>
                            <Button type='button' size='sm' tier='tertiary' onClick={() => void run({ kind: 'dismissImport' })}>
                                Dismiss
                            </Button>
                        </div>
                    </div>
                ) : null}
                {created ? <p role='status'>{created}</p> : null}
            </div>
        </section>
    )
}

function Git({ git, run }: { git: GitView | null; run: Run }) {
    return (
        <section aria-label='Git' className='flex flex-col gap-2 text-[13px]' data-slot='workbench-git'>
            <p className='text-academy-ink-soft'>Branch and changes (read-only). Local editing, saving and previews work without Git.</p>
            <Button type='button' size='sm' tier='secondary' className='self-start' onClick={() => void run({ kind: 'readGit' })}>
                Read Git status
            </Button>
            {git ? (
                git.state === 'ready' ? (
                    <div className='flex flex-col gap-1'>
                        <p>
                            {`Branch ${git.branch ?? 'detached'}${git.head ? ` at ${git.head}` : ''}${git.upstream ? ` · tracking ${git.upstream} (${git.ahead ?? 0} ahead, ${git.behind ?? 0} behind)` : ''}`}
                        </p>
                        {git.changes.length === 0 ? <p>No changes in this collection.</p> : null}
                        <ul className='font-mono text-[12px]'>
                            {git.changes.map((c) => (
                                <li key={`${c.status}:${c.path}`}>{`${c.status} ${c.path}`}</li>
                            ))}
                        </ul>
                        {git.truncated ? <p>Only the first 500 changes are shown.</p> : null}
                    </div>
                ) : (
                    <p>
                        {git.state === 'notRepository'
                            ? 'This collection is not in a Git working tree.'
                            : git.state === 'unavailable'
                              ? 'Git was not found on this Mac.'
                              : 'Git did not answer in time.'}
                    </p>
                )
            ) : null}
        </section>
    )
}
