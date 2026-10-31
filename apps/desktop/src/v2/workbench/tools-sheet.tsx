import { useId, useState } from 'react'
import { createPortal } from 'react-dom'
import type { CollectionView, GitView, WorkbenchView as WorkbenchState } from '@prisma-airs/desktop-contracts'
import { type WorkbenchCall, resultText } from '../../features/workbench/workbench-call'
import { Btn, Field } from '../controls'
import { Modal } from '../modal'
import { API_COPY as C } from './copy'
import { TEXTAREA_STYLE } from './request-editor'

export type Tool = 'environments' | 'secrets' | 'schemas' | 'import' | 'git'
export type Run = (request: Parameters<WorkbenchCall>[0]) => ReturnType<WorkbenchCall>

const soft = { color: 'var(--v2-soft)' }
const faint = { color: 'var(--v2-faint)' }
const H2 = 'text-[17px] font-bold'
const LABEL = 'flex flex-col text-[13px] font-medium'

interface Props {
    tool: Tool
    state: WorkbenchState
    collection: CollectionView
    run: Run
    onClose: () => void
    /** A cURL import created a draft (the sheet closes; the API Client already shows the request). */
    onCreated: (r: Awaited<ReturnType<WorkbenchCall>>) => void
    /**
     * Dispatches a cURL import as a load of the collection: the API Client begins the load (its collection and
     * generation) before the call and settles the reply against it, so a reply arriving after a selection change
     * or a collection boundary is ignored instead of taking a fresh lifetime.
     */
    importCurl: (text: string, relative: string) => ReturnType<WorkbenchCall>
    /** Where focus returns on close: the Collection chip, since the menu item that opened the sheet closes with its menu (I8). */
    returnFocus?: HTMLElement | null
}

/**
 * The collection's tools (C14) in a right-docked sheet: environments,
 * secrets, schemas, imports and Git. Values entered for secrets go to the
 * Keychain through the core and are never shown again.
 */
export function ToolsSheet({ tool, state, collection, run: runOutside, onClose, onCreated, importCurl: importOutside, returnFocus = null }: Props) {
    const title = C.tools[tool].replace(/…$/, '')
    // A failed action is said inside the sheet: the page behind it is inert and hidden from assistive technology while
    // the sheet is open, so the API Client's own alert there reaches no one. Entered work and focus are untouched.
    const [failure, setFailure] = useState('')
    const noted = (r: Awaited<ReturnType<WorkbenchCall>>) => {
        const text = resultText(r)
        if (text) setFailure(text)
        else if (r.kind !== 'cancelled') setFailure('')
        return r
    }
    const run: Run = async (request) => noted(await runOutside(request))
    const importCurl = async (text: string, relative: string) => noted(await importOutside(text, relative))
    return createPortal(
        <Modal label={title} sheet width={620} onClose={onClose} returnFocus={returnFocus} data-slot='workbench-tools'>
            <div className='flex flex-col' style={{ gap: 14 }} data-slot={`workbench-${tool}`}>
                <div className='flex items-center' style={{ gap: 10 }}>
                    <h2 className={H2}>{title}</h2>
                    <Btn tier='tertiary' className='ml-auto' aria-label={`Close ${title}`} onClick={onClose}>
                        ×
                    </Btn>
                </div>
                {failure ? (
                    <p role='alert' className='rounded-[8px] text-[13px]' style={{ padding: '10px 12px', background: 'var(--v2-block-bg)', color: 'var(--v2-ink)' }} data-slot='workbench-tools-failure'>
                        {failure}
                    </p>
                ) : null}
                {tool === 'environments' ? (
                    <Environments collection={collection} run={run} />
                ) : tool === 'secrets' ? (
                    <Secrets collection={collection} run={run} />
                ) : tool === 'schemas' ? (
                    <Schemas collection={collection} run={run} />
                ) : tool === 'import' ? (
                    <Import state={state} run={run} importCurl={importCurl} onCreated={onCreated} />
                ) : (
                    <Git git={state.git} run={run} />
                )}
            </div>
        </Modal>,
        document.body
    )
}

function Environments({ collection, run }: { collection: CollectionView; run: Run }) {
    const ids = useId()
    const [open, setOpen] = useState<{
        name: string
        private: boolean
        text: string
        revision: string | null
    } | null>(null)
    const [name, setName] = useState('')
    const load = async (env: string, priv: boolean) => {
        const r = await run({ kind: 'openEnvironment', name: env, private: priv })
        if (r.kind === 'environment')
            setOpen({
                name: r.name,
                private: r.private,
                text: r.text,
                revision: r.revision,
            })
    }
    return (
        <section aria-label='Environments' className='flex flex-col text-[13px]' style={{ gap: 12 }}>
            <p style={soft}>Shared environments are TOML files saved in the collection. Private environments live only on this Mac and are never committed.</p>
            {collection.environments.length === 0 ? <p>No environments yet.</p> : null}
            <ul className='flex flex-col' style={{ gap: 6 }}>
                {collection.environments.map((e) => (
                    <li key={e.name} className='flex flex-wrap items-center' style={{ gap: 8 }}>
                        <span className='font-medium'>{e.name}</span>
                        <span style={faint}>{`(${C.chips.envOrigin[e.origin] ?? e.origin})`}</span>
                        {e.origin !== 'local' ? <Btn onClick={() => void load(e.name, false)}>{`Edit shared ${e.name}`}</Btn> : null}
                        <Btn onClick={() => void load(e.name, true)}>{`Edit private ${e.name}`}</Btn>
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap items-end' style={{ gap: 8 }}>
                <label className={LABEL} htmlFor={`${ids}-env`} style={{ gap: 4 }}>
                    Environment name
                    <Field id={`${ids}-env`} value={name} onChange={(e) => setName(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Btn disabled={!name} onClick={() => void load(name, false)}>
                    New shared environment
                </Btn>
                <Btn disabled={!name} onClick={() => void load(name, true)}>
                    New private environment
                </Btn>
                <Btn tier='tertiary' onClick={() => void load('_defaults', true)}>
                    Private collection defaults
                </Btn>
            </div>
            {open ? (
                <form
                    aria-label={`${open.private ? 'Private' : 'Shared'} environment ${open.name}`}
                    className='flex flex-col'
                    style={{ gap: 8 }}
                    onSubmit={(e) => {
                        e.preventDefault()
                        // The reply is bound to the environment it saved and to the text it was sent: it brings the new
                        // revision, and the editor's text only if nothing newer was typed meanwhile (a save's reply had
                        // replaced the editor, losing typing made while it was pending).
                        const sent = { name: open.name, private: open.private, text: open.text }
                        void run({
                            kind: 'saveEnvironment',
                            name: open.name,
                            private: open.private,
                            revision: open.revision,
                            text: open.text,
                        }).then((r) => {
                            if (r.kind === 'environment')
                                setOpen((o) => (o && o.name === sent.name && o.private === sent.private ? { ...o, revision: r.revision, text: o.text === sent.text ? r.text : o.text } : o))
                        })
                    }}
                >
                    <label className={LABEL} htmlFor={`${ids}-env-text`} style={{ gap: 4 }}>
                        {`${open.private ? 'Private' : 'Shared'} environment ${open.name}`}
                        <textarea id={`${ids}-env-text`} style={TEXTAREA_STYLE} value={open.text} spellCheck={false} onChange={(e) => setOpen({ ...open, text: e.target.value })} />
                    </label>
                    <div className='flex flex-wrap' style={{ gap: 8 }}>
                        <Btn tier='primary' type='submit'>
                            Save environment
                        </Btn>
                        {open.private && open.revision ? (
                            <Btn tier='tertiary' onClick={() => void run({ kind: 'deleteEnvironment', name: open.name }).then((r) => r.kind === 'done' && setOpen(null))}>
                                Delete private environment
                            </Btn>
                        ) : null}
                        <Btn tier='tertiary' onClick={() => setOpen(null)}>
                            Close
                        </Btn>
                    </div>
                </form>
            ) : null}
        </section>
    )
}

function Secrets({ collection, run }: { collection: CollectionView; run: Run }) {
    const ids = useId()
    const [values, setValues] = useState<Record<string, string>>({})
    return (
        <section aria-label='Secrets' className='flex flex-col text-[13px]' style={{ gap: 12 }}>
            <p style={soft}>Files hold secret references only. Values are kept in the Keychain on this Mac, never shown again and never written to a collection.</p>
            {collection.secrets.length === 0 ? <p>No secret references are used yet.</p> : null}
            {collection.secrets.map((s, i) =>
                s.managed ? (
                    <p key={s.reference}>
                        <span className='font-mono'>{s.reference}</span>: managed by the tenant in Settings (resolved only by tenant authority when sending arrives).
                    </p>
                ) : (
                    <form
                        key={s.reference}
                        className='flex flex-wrap items-end'
                        style={{ gap: 8 }}
                        onSubmit={(e) => {
                            e.preventDefault()
                            const value = values[s.reference] ?? ''
                            setValues((v) => ({ ...v, [s.reference]: '' }))
                            void run({ kind: 'setSecret', reference: s.reference, value })
                        }}
                    >
                        <label className={LABEL} htmlFor={`${ids}-secret-${i}`} style={{ gap: 4 }}>
                            {`${s.reference} (${s.stored ? 'value stored' : 'no value'})`}
                            <Field
                                id={`${ids}-secret-${i}`}
                                type='password'
                                autoComplete='off'
                                value={values[s.reference] ?? ''}
                                onChange={(e) => setValues((v) => ({ ...v, [s.reference]: e.target.value }))}
                            />
                        </label>
                        <Btn type='submit' disabled={!(values[s.reference] ?? '')}>
                            {`Store value for ${s.reference}`}
                        </Btn>
                        {s.stored ? (
                            <Btn tier='tertiary' onClick={() => void run({ kind: 'removeSecret', reference: s.reference })}>
                                {`Remove value for ${s.reference}`}
                            </Btn>
                        ) : null}
                    </form>
                )
            )}
        </section>
    )
}

function Schemas({ collection, run }: { collection: CollectionView; run: Run }) {
    const ids = useId()
    const [open, setOpen] = useState<{
        relative: string
        text: string
        revision: string | null
    } | null>(null)
    const [path, setPath] = useState('')
    return (
        <section aria-label='Schemas' className='flex flex-col text-[13px]' style={{ gap: 12 }}>
            <p style={soft}>JSON or YAML schema text, edited without validation or request generation. Associate one from a request's Body tab.</p>
            <ul className='flex flex-col' style={{ gap: 6 }}>
                {collection.schemas.map((s) => (
                    <li key={s}>
                        <Btn onClick={() => void run({ kind: 'openSchema', relative: s }).then((r) => r.kind === 'schema' && setOpen(r.schema))}>{`Edit ${s}`}</Btn>
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap items-end' style={{ gap: 8 }}>
                <label className={LABEL} htmlFor={`${ids}-schema`} style={{ gap: 4 }}>
                    New schema path (.json, .yaml or .yml)
                    <Field id={`${ids}-schema`} value={path} onChange={(e) => setPath(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Btn disabled={!path} onClick={() => setOpen({ relative: path, text: '', revision: null })}>
                    New schema
                </Btn>
            </div>
            {open ? (
                <form
                    aria-label={`Schema ${open.relative}`}
                    className='flex flex-col'
                    style={{ gap: 8 }}
                    onSubmit={(e) => {
                        e.preventDefault()
                        const sent = { relative: open.relative, text: open.text }
                        void run({
                            kind: 'saveSchema',
                            relative: open.relative,
                            revision: open.revision,
                            text: open.text,
                        }).then((r) => {
                            // Bound to the schema it saved and the text it was sent: the new revision, the text only if nothing newer was typed.
                            if (r.kind === 'schema') setOpen((o) => (o && o.relative === sent.relative ? { ...o, revision: r.schema.revision, text: o.text === sent.text ? r.schema.text : o.text } : o))
                        })
                    }}
                >
                    <label className={LABEL} htmlFor={`${ids}-schema-text`} style={{ gap: 4 }}>
                        {`Schema ${open.relative}`}
                        <textarea id={`${ids}-schema-text`} style={TEXTAREA_STYLE} value={open.text} spellCheck={false} onChange={(e) => setOpen({ ...open, text: e.target.value })} />
                    </label>
                    <div className='flex' style={{ gap: 8 }}>
                        <Btn tier='primary' type='submit'>
                            Save schema
                        </Btn>
                        <Btn tier='tertiary' onClick={() => setOpen(null)}>
                            Close
                        </Btn>
                    </div>
                </form>
            ) : null}
        </section>
    )
}

function Import({ state, run, importCurl, onCreated }: { state: WorkbenchState; run: Run; importCurl: (text: string, relative: string) => ReturnType<WorkbenchCall>; onCreated: (r: Awaited<ReturnType<WorkbenchCall>>) => void }) {
    const ids = useId()
    const [text, setText] = useState('')
    const [path, setPath] = useState('imported/request')
    const [chosen, setChosen] = useState<number[]>([])
    const [created, setCreated] = useState('')
    return (
        <section aria-label='Import' className='flex flex-col text-[13px]' style={{ gap: 16 }}>
            <form
                className='flex flex-col'
                style={{ gap: 8 }}
                onSubmit={(e) => {
                    e.preventDefault()
                    void importCurl(text, path).then((r) => {
                        if (r.kind === 'document') {
                            setText('')
                            onCreated(r)
                        }
                    })
                }}
            >
                <h3 className='text-[15px] font-semibold'>Paste one cURL command</h3>
                <p style={soft}>The command is parsed, never executed. Literal credentials become secret references you provide in Secrets.</p>
                <label className={LABEL} htmlFor={`${ids}-curl`} style={{ gap: 4 }}>
                    cURL command
                    <textarea id={`${ids}-curl`} style={TEXTAREA_STYLE} value={text} spellCheck={false} onChange={(e) => setText(e.target.value)} />
                </label>
                <label className={LABEL} htmlFor={`${ids}-curl-path`} style={{ gap: 4 }}>
                    Draft path (folder/name)
                    <Field id={`${ids}-curl-path`} value={path} onChange={(e) => setPath(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Btn tier='primary' type='submit' className='self-start' disabled={!text.trim()}>
                    Create draft
                </Btn>
            </form>
            <div className='flex flex-col' style={{ gap: 8 }}>
                <h3 className='text-[15px] font-semibold'>Insomnia export</h3>
                <p style={soft}>Insomnia v5 collections (schema 5.1, YAML or JSON, up to 1 MiB). Scripts and plugins are not converted.</p>
                <Btn className='self-start' onClick={() => void run({ kind: 'chooseInsomnia' })}>
                    Choose Insomnia export…
                </Btn>
                {state.import ? (
                    <div className='flex flex-col' style={{ gap: 6 }} data-slot='workbench-import-preview'>
                        <p className='font-medium'>Conversion report</p>
                        {state.import.map((r) => (
                            <label key={r.index} className='flex items-center' style={{ gap: 8 }}>
                                <input
                                    type='checkbox'
                                    disabled={r.problem !== null}
                                    checked={chosen.includes(r.index)}
                                    onChange={(e) => setChosen((x) => (e.target.checked ? [...x, r.index] : x.filter((i) => i !== r.index)))}
                                />
                                {`${r.environment ? 'Environment' : 'Request'} ${r.label}${r.problem ? ` · not converted (${r.problem})` : ` · ${r.relative}`}`}
                            </label>
                        ))}
                        <div className='flex flex-wrap' style={{ gap: 8 }}>
                            <Btn tier='tertiary' onClick={() => setChosen(state.import?.filter((r) => r.problem === null).map((r) => r.index) ?? [])}>
                                Select all supported
                            </Btn>
                            <Btn
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
                            </Btn>
                            <Btn tier='tertiary' onClick={() => void run({ kind: 'dismissImport' })}>
                                Dismiss
                            </Btn>
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
        <section aria-label='Git' className='flex flex-col text-[13px]' style={{ gap: 8 }}>
            <p style={soft}>Branch and changes (read-only). Local editing, saving and previews work without Git.</p>
            <Btn className='self-start' onClick={() => void run({ kind: 'readGit' })}>
                Read Git status
            </Btn>
            {git ? (
                git.state === 'ready' ? (
                    <div className='flex flex-col' style={{ gap: 4 }}>
                        <p>{`Branch ${git.branch ?? 'detached'}${git.head ? ` at ${git.head}` : ''}${git.upstream ? ` · tracking ${git.upstream} (${git.ahead ?? 0} ahead, ${git.behind ?? 0} behind)` : ''}`}</p>
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
