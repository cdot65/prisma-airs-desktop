import { useId, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { CollectionView, RequestEntry } from '@prisma-airs/desktop-contracts'
import { Btn, Field } from '../controls'
import { Glyph } from '../icons'
import { Modal } from '../modal'
import { API_COPY as C, methodColor } from './copy'

interface Props {
    collection: CollectionView
    /** Opens a request (the core reads its document). */
    onOpen: (relative: string) => void
    /** Creates a request draft or a folder; resolves whether it was created. */
    onCreate: (kind: 'request' | 'folder', relative: string) => Promise<boolean>
}

const LIMIT = 300

export const requestName = (relative: string) =>
    relative
        .split('/')
        .pop()
        ?.replace(/\.request\.toml$/, '') ?? relative
const folderOf = (relative: string) => relative.split('/').slice(0, -1).join('/')
const short = (method: string) => (method === 'DELETE' ? 'DEL' : method === 'OPTIONS' ? 'OPT' : method)

/**
 * The request tree (R2App's 286px pane): Find request, +, folders with
 * their counts and the requests under them with their methods. A search
 * lists the matching requests flat. Nothing here sends anything.
 */
export function RequestTree({ collection, onOpen, onCreate }: Props) {
    const ids = useId()
    const [query, setQuery] = useState('')
    const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set())
    const [creating, setCreating] = useState<null | 'request' | 'folder'>(null)
    const [path, setPath] = useState('')
    const selected = collection.tabs.find((t) => t.id === collection.selected)?.relative ?? null
    const needle = query.trim().toLowerCase()

    const { folders, byFolder } = useMemo(() => {
        const byFolder = new Map<string, RequestEntry[]>()
        for (const r of collection.requests) {
            const f = folderOf(r.relative)
            byFolder.set(f, [...(byFolder.get(f) ?? []), r])
        }
        const folders = [...new Set([...collection.folders, ...byFolder.keys()].filter((f) => f !== ''))].sort()
        return { folders, byFolder }
    }, [collection.requests, collection.folders])

    const matching = needle === '' ? null : collection.requests.filter((r) => `${r.method} ${r.relative} ${r.url}`.toLowerCase().includes(needle))

    const request = (r: RequestEntry, depth: number) => (
        <button
            key={r.id}
            type='button'
            aria-label={C.openRequest(r.method, r.relative)}
            aria-current={r.relative === selected ? 'true' : undefined}
            onClick={() => onOpen(r.relative)}
            className='flex w-full min-h-0 items-center rounded-[6px] text-left text-[12.5px]'
            style={{
                gap: 6,
                height: 30,
                padding: `0 8px 0 ${8 + depth * 14}px`,
                background: r.relative === selected ? 'var(--v2-inset)' : 'transparent',
                color: 'var(--v2-ink)',
                fontWeight: r.relative === selected ? 600 : 400,
            }}
            data-slot='tree-request'
        >
            <span className='shrink-0 font-mono text-[11px] font-bold' style={{ width: 34, color: methodColor(r.method) }}>
                {short(r.method)}
            </span>
            <span className='min-w-0 flex-grow truncate'>{requestName(r.relative)}</span>
        </button>
    )

    const toggle = (f: string) =>
        setCollapsed((s) => {
            const n = new Set(s)
            if (n.has(f)) n.delete(f)
            else n.add(f)
            return n
        })

    let shown = 0
    const list = matching
        ? matching.slice(0, LIMIT).map((r) => request(r, 0))
        : [
              ...folders.map((f) => {
                  const open = !collapsed.has(f)
                  const items = byFolder.get(f) ?? []
                  const depth = f.split('/').length - 1
                  return (
                      <div key={`f:${f}`} data-slot='tree-folder'>
                          <button
                              type='button'
                              aria-expanded={open}
                              aria-label={C.folder(f, open)}
                              onClick={() => toggle(f)}
                              className='flex w-full min-h-0 items-center rounded-[6px] text-left text-[12.5px] font-semibold'
                              style={{
                                  gap: 6,
                                  height: 30,
                                  padding: `0 8px 0 ${8 + depth * 14}px`,
                                  color: 'var(--v2-ink)',
                              }}
                          >
                              <span className='inline-flex shrink-0' style={{ transform: open ? 'none' : 'rotate(-90deg)' }}>
                                  <Glyph name='chev' size={10} color='var(--v2-faint)' />
                              </span>
                              <Glyph name='folder' size={13} color='var(--v2-faint)' />
                              <span className='min-w-0 flex-grow truncate'>{f.split('/').pop()}</span>
                              <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                  {items.length}
                              </span>
                          </button>
                          {open ? items.slice(0, Math.max(0, LIMIT - shown)).map((r) => ((shown += 1), request(r, depth + 1))) : null}
                      </div>
                  )
              }),
              ...(byFolder.get('') ?? []).slice(0, Math.max(0, LIMIT - shown)).map((r) => request(r, 0)),
          ]

    const total = matching ? matching.length : collection.requests.length

    return (
        <form
            aria-label={C.tree}
            data-slot='request-tree'
            className='flex min-h-0 shrink-0 flex-col'
            style={{
                width: 286,
                borderRight: '1px solid var(--v2-rule)',
                background: 'var(--v2-side)',
            }}
            onSubmit={(e) => e.preventDefault()}
        >
            <div className='flex' style={{ gap: 6, padding: 10 }}>
                <label
                    htmlFor={`${ids}-find`}
                    className='flex min-w-0 flex-grow items-center rounded-[7px] text-[12px]'
                    style={{
                        gap: 7,
                        height: 32,
                        padding: '0 9px',
                        background: 'var(--v2-inset)',
                        color: 'var(--v2-faint)',
                    }}
                >
                    <Glyph name='search' size={12} color='var(--v2-faint)' />
                    <input
                        id={`${ids}-find`}
                        type='search'
                        aria-label={C.find}
                        placeholder={C.find}
                        value={query}
                        autoComplete='off'
                        onChange={(e) => setQuery(e.target.value)}
                        className='min-h-0 min-w-0 flex-grow border-0 bg-transparent p-0 text-[13px]'
                        style={{
                            border: 0,
                            minHeight: 0,
                            height: 24,
                            borderRadius: 0,
                            background: 'transparent',
                            color: 'var(--v2-ink)',
                        }}
                        data-slot='tree-search'
                    />
                </label>
                <button
                    type='button'
                    aria-label={C.newRequest}
                    aria-haspopup='dialog'
                    title={C.newRequest}
                    onClick={() => {
                        setPath('')
                        setCreating('request')
                    }}
                    className='grid size-8 min-h-0 shrink-0 place-items-center rounded-[7px] border text-[15px]'
                    style={{
                        borderColor: 'var(--v2-rule)',
                        background: 'var(--v2-paper)',
                        color: 'var(--v2-ink)',
                    }}
                >
                    +
                </button>
            </div>
            <div className='min-h-0 flex-grow overflow-auto' style={{ padding: '0 6px 10px' }} data-slot='tree-list'>
                {list}
                {total === 0 ? (
                    <div className='flex flex-col items-start text-[12.5px] leading-[1.5]' style={{ gap: 10, padding: '16px 10px', color: 'var(--v2-faint)' }} data-slot='tree-empty'>
                        <span>{matching ? C.noMatch(query.trim(), collection.requests.length) : C.noRequests}</span>
                        {matching ? <Btn onClick={() => setQuery('')}>{C.clearSearch}</Btn> : null}
                    </div>
                ) : null}
                {total > LIMIT ? (
                    <p className='text-[12px]' style={{ padding: '8px 10px', color: 'var(--v2-faint)' }}>
                        {C.tooMany(LIMIT, total)}
                    </p>
                ) : null}
            </div>
            {creating
                ? createPortal(
                      <Modal label={creating === 'request' ? C.newRequestTitle : C.newFolder} width={480} onClose={() => setCreating(null)} data-slot='new-request-dialog'>
                          <form
                              className='flex flex-col'
                              style={{ gap: 12 }}
                              onSubmit={(e) => {
                                  e.preventDefault()
                                  const kind = creating
                                  void onCreate(kind, path.trim()).then((ok) => ok && setCreating(null))
                              }}
                          >
                              <h2 className='text-[17px] font-bold'>{creating === 'request' ? C.newRequestTitle : C.newFolder}</h2>
                              <div role='radiogroup' aria-label={C.create} className='flex' style={{ gap: 12 }}>
                                  {(['request', 'folder'] as const).map((k) => (
                                      <label key={k} className='flex items-center text-[13px]' style={{ gap: 6 }}>
                                          <input type='radio' name={`${ids}-kind`} checked={creating === k} onChange={() => setCreating(k)} />
                                          {k === 'request' ? C.newRequest : C.newFolder}
                                      </label>
                                  ))}
                              </div>
                              <label htmlFor={`${ids}-path`} className='flex flex-col text-[13px] font-medium' style={{ gap: 4 }}>
                                  {creating === 'request' ? C.newRequestPath : C.newFolderPath}
                                  <Field id={`${ids}-path`} value={path} autoFocus autoComplete='off' spellCheck={false} onChange={(e) => setPath(e.target.value)} />
                              </label>
                              {creating === 'request' ? (
                                  <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                      {C.newRequestLead}
                                  </p>
                              ) : null}
                              <div className='flex justify-end' style={{ gap: 8 }}>
                                  <Btn onClick={() => setCreating(null)}>{C.cancel}</Btn>
                                  <Btn tier='primary' type='submit' disabled={path.trim() === ''}>
                                      {C.create}
                                  </Btn>
                              </div>
                          </form>
                      </Modal>,
                      document.body
                  )
                : null}
        </form>
    )
}
