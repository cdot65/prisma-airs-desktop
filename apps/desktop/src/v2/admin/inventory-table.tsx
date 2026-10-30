import { useEffect, useId, useRef, useState } from 'react'
import type { AdminAction, ContextRef, ManagementDetail, ManagementPage, ManagementResource, RotationView, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, RESOURCES, type ResourceInfo, fieldText, itemId, itemName, readManagement } from '../../features/administration/management-read'
import { type ChangeTarget, TEMPLATES, pretty } from '../../features/administration/runtime-changes'
import type { Dispatch } from '../../features/tenants/dispatch'
import { Glyph } from '../icons'
import { Btn } from '../controls'
import { ADMIN_COPY as C } from './copy'

/** Rows per page (the native side accepts 1..=100). */
export const PAGE_SIZE = 25

type Item = Record<string, unknown>

interface Shown {
    page: ManagementPage
    items: Item[]
    readAt: Date
}

type DetailState =
    | { kind: 'loading'; name: string }
    | { kind: 'shown'; name: string; detail: ManagementDetail; item: Item | null }
    | { kind: 'failed'; name: string; message: string }

interface Props {
    snapshot: ShellSnapshot
    context: ContextRef
    dispatch: Dispatch
    observe: ObserveManagement
    resource: ManagementResource
    /** The inventory and item a contextual action asked for (C30): selected on arrival. */
    arrival?: { resource: ManagementResource | null; item: string | null; sequence: number } | null
    /** The tab's admitted actions (`AdminInventory`): only these are offered. */
    admitted: AdminAction[]
    labels?: Partial<Record<ChangeTarget['action'], string>>
    onReview: (change: ChangeTarget) => void
    rotations: RotationView[]
    onFinishRotation: (application: string) => void
}

const time = (date: Date) => date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

const CHANGE_KIND: Partial<Record<ManagementResource, ChangeTarget['kind']>> = {
    profiles: 'profiles',
    topics: 'topics',
    applications: 'applications',
    apiKeys: 'apiKeys',
    dataPatterns: 'dataPatterns',
    dataProfiles: 'dataProfiles',
    filteringProfiles: 'filteringProfiles',
    dictionaries: 'dictionaries',
}

/** The changes offered from an inventory's toolbar (v1's templates). */
const CREATE: Partial<Record<ManagementResource, ChangeTarget>> = {
    profiles: { kind: 'profiles', action: 'create', target: '', title: 'New security profile', body: pretty(TEMPLATES.profile) },
    topics: { kind: 'topics', action: 'create', target: '', title: 'New custom topic', body: pretty(TEMPLATES.topic) },
    apiKeys: { kind: 'apiKeys', action: 'create', target: '', title: 'Create API key', body: pretty(TEMPLATES.key) },
    dataPatterns: { kind: 'dataPatterns', action: 'create', target: '', title: 'New data pattern', body: pretty(TEMPLATES.pattern) },
    dataProfiles: { kind: 'dataProfiles', action: 'create', target: '', title: 'New data profile', body: pretty(TEMPLATES.dataProfile) },
    dictionaries: { kind: 'dictionaries', action: 'create', target: '', title: 'New dictionary', body: JSON.stringify(TEMPLATES.dictionary) },
}

const ACTION_LABEL: Record<ChangeTarget['action'], string> = {
    create: 'Create…',
    update: 'Edit…',
    delete: 'Delete…',
    version: 'Add key version…',
    rotate: 'Rotate…',
    models: 'Models…',
}

/** The changes v1 offers for one item. */
function itemChanges(info: ResourceInfo, item: Item, name: string): ChangeTarget[] {
    const kind = CHANGE_KIND[info.id]
    const id = itemId(info, item)
    if (!kind || id === null) return []
    if (kind === 'apiKeys') {
        return [
            { kind, action: 'rotate', target: id, title: `Rotate ${name}`, body: pretty(TEMPLATES.rotate) },
            { kind, action: 'version', target: id, title: `Issue additional key for ${fieldText(item.cust_app)}`, body: pretty({ ...TEMPLATES.key, cust_app: item.cust_app ?? '' }) },
            { kind, action: 'delete', target: id, title: `Delete ${name}`, body: '{}' },
        ]
    }
    const edit: ChangeTarget = { kind, action: 'update', target: id, title: `Edit ${name}`, body: pretty(item) }
    const remove: ChangeTarget = { kind, action: 'delete', target: id, title: `Delete ${name}`, body: '{}' }
    switch (kind) {
        case 'dictionaries': {
            const metadata = Object.fromEntries(Object.entries(item).filter(([k]) => k !== 'id'))
            return [{ ...edit, body: JSON.stringify({ metadata, file: '' }) }, remove]
        }
        case 'dataProfiles':
        case 'filteringProfiles':
            return [edit]
        default:
            return [edit, remove]
    }
}

/** A short list for a table cell: the first three names and how many more. */
function listText(names: string[]): string {
    const shown = names.filter(Boolean)
    if (shown.length === 0) return '—'
    return shown.length <= 3 ? shown.join(', ') : `${shown.slice(0, 3).join(', ')} +${shown.length - 3}`
}

/** The custom topics a security profile's policy names, by one walk of its JSON (C31). */
export function topicRefs(profile: Item): { id: string | null; name: string | null }[] {
    const found = new Map<string, { id: string | null; name: string | null }>()
    const walk = (value: unknown, depth: number) => {
        if (depth > 12 || value === null || typeof value !== 'object') return
        if (Array.isArray(value)) {
            for (const v of value) walk(v, depth + 1)
            return
        }
        const o = value as Record<string, unknown>
        const id = typeof o.topic_id === 'string' ? o.topic_id : null
        const name = typeof o.topic_name === 'string' ? o.topic_name : null
        if (id || name) found.set(id ?? `name:${name}`, { id, name })
        for (const v of Object.values(o)) walk(v, depth + 1)
    }
    walk(profile.policy ?? profile, 0)
    return [...found.values()]
}

interface TopicRelations {
    usedBy: (topicId: string | null, topicName: string) => string[]
    complete: boolean
    failure: string | null
}

/** The profile ↔ topic relationship inventory: one read of the profiles (first 100), walked once (C31). */
function useTopicRelations(resource: ManagementResource, shown: Shown | null, context: ContextRef, dispatch: Dispatch, observe: ObserveManagement): TopicRelations | null {
    const [relations, setRelations] = useState<TopicRelations | null>(null)
    const readAt = shown?.readAt.getTime() ?? 0
    useEffect(() => {
        if (resource !== 'topics') return
        let live = true
        void readManagement(dispatch, observe, context, { kind: 'page', resource: 'profiles', offset: 0, limit: 100 }).then((outcome) => {
            if (!live) return
            if (outcome.kind === 'notAdmitted' || outcome.result.kind !== 'page') {
                setRelations({ usedBy: () => [], complete: false, failure: outcome.kind === 'notAdmitted' ? outcome.message : 'failed' })
                return
            }
            const page = outcome.result.page
            const profiles = JSON.parse(page.items) as Item[]
            const byId = new Map<string, string[]>()
            const byName = new Map<string, string[]>()
            for (const profile of profiles) {
                const name = typeof profile.profile_name === 'string' ? profile.profile_name : String(profile.profile_id ?? '')
                for (const t of topicRefs(profile)) {
                    if (t.id) byId.set(t.id, [...(byId.get(t.id) ?? []), name])
                    else if (t.name) byName.set(t.name, [...(byName.get(t.name) ?? []), name])
                }
            }
            setRelations({
                usedBy: (id, topicName) => [...new Set([...(id ? (byId.get(id) ?? []) : []), ...(byName.get(topicName) ?? [])])],
                complete: page.offset === 0 && page.complete,
                failure: null,
            })
        })
        return () => {
            live = false
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- re-derived with each topics read
    }, [resource, context, readAt])
    return relations
}

/**
 * One management inventory as R2App's table draws it (C39): the search
 * field and the inventory's create action above a role=table grid (name,
 * ID, the inventory's own relation column, a View action per row), the
 * footer with the row count and the read time, paging, and the detail panel
 * with exactly the admitted item actions. Every page and detail is one
 * explicit read through the tenant's management client (C08): nothing is
 * cached across tenants or credential generations, a failed refresh keeps
 * the previous rows marked with their age, and an inventory is called empty
 * or complete only when the service said so.
 */
export function InventoryTable({ snapshot, context, dispatch, observe, resource, arrival = null, admitted, labels, onReview, rotations, onFinishRotation }: Props) {
    const ids = useId()
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const info = RESOURCES.find((r) => r.id === resource) ?? (RESOURCES[0] as ResourceInfo)
    const [shown, setShown] = useState<Shown | null>(null)
    const [failure, setFailure] = useState<string | null>(null)
    const [loading, setLoading] = useState(false)
    const [filter, setFilter] = useState(arrival?.item ?? '')
    const [detail, setDetail] = useState<DetailState | null>(null)
    const arrivalItem = useRef<string | null>(arrival?.item ?? null)
    const seenArrival = useRef(arrival?.sequence ?? 0)
    useEffect(() => {
        if (!arrival || arrival.sequence === seenArrival.current) return
        seenArrival.current = arrival.sequence
        arrivalItem.current = arrival.item
        setFilter(arrival.item ?? '')
    }, [arrival])
    const token = useRef(0)
    const detailToken = useRef(0)
    const opened = useRef<Item | null>(null)

    async function load(offset: number, keepDetail = false) {
        const mine = ++token.current
        setLoading(true)
        const outcome = await readManagement(dispatch, observe, context, { kind: 'page', resource, offset, limit: PAGE_SIZE })
        if (mine !== token.current) return
        setLoading(false)
        if (outcome.kind === 'notAdmitted') {
            setFailure(outcome.message)
            return
        }
        const result = outcome.result
        if (result.kind === 'page' && result.page.resource === resource && result.page.offset === offset) {
            setShown({ page: result.page, items: JSON.parse(result.page.items) as Item[], readAt: new Date() })
            setFailure(null)
            if (keepDetail && opened.current) void openDetail(opened.current)
            else {
                detailToken.current += 1
                setDetail(null)
                opened.current = null
            }
        } else if (result.kind === 'failed') setFailure(MANAGEMENT_FAILURE_TEXT[result.failure])
        else setFailure(MANAGEMENT_FAILURE_TEXT.invalidResponse)
    }

    async function openDetail(item: Item) {
        const id = itemId(info, item)
        const name = itemName(info, item)
        if (id === null) return
        opened.current = item
        const mine = ++detailToken.current
        setDetail({ kind: 'loading', name })
        const outcome = await readManagement(dispatch, observe, context, { kind: 'detail', resource, id })
        if (mine !== detailToken.current) return
        if (outcome.kind === 'notAdmitted') setDetail({ kind: 'failed', name, message: outcome.message })
        else if (outcome.result.kind === 'detail' && outcome.result.detail.id === id) {
            const found = outcome.result.detail
            setDetail({ kind: 'shown', name, detail: found, item: found.item ? (JSON.parse(found.item) as Item) : null })
        } else if (outcome.result.kind === 'failed') setDetail({ kind: 'failed', name, message: MANAGEMENT_FAILURE_TEXT[outcome.result.failure] })
        else setDetail({ kind: 'failed', name, message: MANAGEMENT_FAILURE_TEXT.invalidResponse })
    }

    useEffect(() => {
        token.current += 1
        detailToken.current += 1
        setShown(null)
        setFailure(null)
        setDetail(null)
        setFilter(arrivalItem.current ?? '')
        arrivalItem.current = null
        setLoading(false)
        if (ready) void load(0)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- a new tenant, credential or inventory starts over
    }, [context, resource, ready])

    // The table follows what changed it (C31): a settled change of this kind, or a rotation moving on, re-reads the page.
    const kindHere = CHANGE_KIND[resource]
    const changedHere = [
        ...snapshot.changes.results.filter((r) => r.settled && r.kind === kindHere).map((r) => r.operation),
        ...(resource === 'apiKeys' ? rotations.map((r) => `${r.application}:${r.stage}:${r.newKey?.id ?? ''}`) : []),
    ].join('|')
    const lastChanged = useRef(changedHere)
    useEffect(() => {
        if (lastChanged.current === changedHere) return
        lastChanged.current = changedHere
        if (ready) void load(shown?.page.offset ?? 0, true)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- re-read on change only
    }, [changedHere])
    const relations = useTopicRelations(resource, shown, context, dispatch, observe)

    const label = (action: ChangeTarget['action']) => admitted.find((a) => a.kind === kindHere && a.action === action && a.row)?.label ?? labels?.[action] ?? ACTION_LABEL[action]
    const create = admitted.some((a) => a.kind === kindHere && a.action === 'create' && !a.row) ? CREATE[resource] : undefined
    const createLabel = admitted.find((a) => a.kind === kindHere && a.action === 'create' && !a.row)?.label ?? labels?.create ?? CREATE[resource]?.title
    const admittedIndex = (c: ChangeTarget) => admitted.findIndex((a) => a.kind === c.kind && a.action === c.action && a.row)

    if (!tenant) return null
    if (!ready) {
        return (
            <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }} data-slot='administration-not-configured'>
                {C.table.notReady(tenant.label)}
            </p>
        )
    }
    const needle = filter.trim().toLowerCase()
    const rows = (shown?.items ?? []).filter((item) => needle.length === 0 || itemName(info, item).toLowerCase().includes(needle) || (itemId(info, item) ?? '').toLowerCase().includes(needle))
    const page = shown?.page
    const empty = page !== undefined && !failure && page.count === 0 && page.offset === 0 && page.complete
    const extra: { heading: string; cell: (item: Item, id: string | null) => string } | null =
        resource === 'profiles'
            ? { heading: 'Topics', cell: (item) => listText(topicRefs(item).map((t) => t.name ?? t.id ?? '')) }
            : resource === 'topics'
              ? {
                    heading: 'Used by',
                    cell: (item, id) => {
                        if (!relations) return 'Reading profiles…'
                        if (relations.failure) return 'Not read'
                        const users = relations.usedBy(id, itemName(info, item))
                        return users.length > 0 ? listText(users) : relations.complete ? 'No profile' : 'None in the profiles read'
                    },
                }
              : resource === 'apiKeys'
                ? {
                      heading: 'State',
                      cell: (_item, id) => {
                          const old = rotations.find((r) => r.oldKey.id === id && r.stage !== 'complete')
                          if (old) return 'Replace after deployment'
                          const fresh = rotations.find((r) => r.newKey?.id === id && r.stage !== 'complete')
                          if (fresh) return `New key for ${fresh.application}`
                          return 'In use'
                      },
                  }
                : null
    const grid = extra ? 'minmax(0, 1.4fr) minmax(0, 1.6fr) minmax(0, 1.2fr) 90px' : 'minmax(0, 1.4fr) minmax(0, 2fr) 90px'
    const cell = 'min-w-0 truncate'
    const count = page ? (page.total !== null && page.total > page.count ? C.table.itemsOf(rows.length, page.total) : C.table.items(rows.length)) : ''
    return (
        <div className='flex min-w-0 flex-col' style={{ gap: 18 }} data-slot='administration'>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <label className='flex items-center rounded-[7px] text-[12px]' style={{ gap: 7, width: 280, maxWidth: '100%', height: 32, padding: '0 9px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                    <Glyph name='search' size={12} />
                    <input
                        id={`${ids}-filter`}
                        aria-label={C.table.search(info.label)}
                        placeholder={C.table.search(info.label)}
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        autoComplete='off'
                        spellCheck={false}
                        className='min-h-0 min-w-0 flex-grow border-0 bg-transparent p-0 text-[13px]'
                        style={{ border: 0, minHeight: 0, borderRadius: 0, background: 'transparent', color: 'var(--v2-ink)', height: 24 }}
                        data-slot='administration-filter'
                    />
                </label>
                <Btn tier='tertiary' disabled={loading} onClick={() => void load(page?.offset ?? 0)}>
                    {C.table.refresh}
                </Btn>
                {create ? (
                    <Btn tier='primary' className='ml-auto' onClick={() => onReview(create)}>
                        {createLabel}
                    </Btn>
                ) : null}
            </div>
            <div role='status' aria-live='polite' className='sr-only' data-slot='administration-progress'>
                {loading ? C.table.reading(info.label) : page ? `${count} · ${C.table.read(time(shown!.readAt))}` : ''}
            </div>
            {failure ? (
                <div role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-flag-bg)', color: 'var(--v2-ink)' }} data-slot='administration-failure'>
                    <p>{failure}</p>
                    {shown ? (
                        <p className='mt-1 text-[12px]' data-slot='administration-stale'>
                            {C.table.stale(time(shown.readAt))}
                        </p>
                    ) : null}
                </div>
            ) : null}
            <div className='overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)' }}>
                <div role='table' aria-label={info.label} data-slot='administration-table'>
                    <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: grid, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                        <span role='columnheader'>{C.table.name}</span>
                        <span role='columnheader'>{C.table.id}</span>
                        {extra ? <span role='columnheader'>{extra.heading}</span> : null}
                        <span role='columnheader'>
                            <span className='sr-only'>{C.table.actions}</span>
                        </span>
                    </div>
                    {rows.map((item, index) => {
                        const id = itemId(info, item)
                        const name = itemName(info, item)
                        return (
                            <div key={`${id ?? 'row'}-${index}`} role='row' className='grid items-center' style={{ gridTemplateColumns: grid, gap: 12, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)' }} data-slot='administration-row'>
                                <span role='cell' className={`${cell} font-mono text-[12px] font-semibold`}>
                                    {name}
                                </span>
                                <span role='cell' className={`${cell} font-mono text-[12px]`} style={{ color: 'var(--v2-soft)' }}>
                                    {id ?? '—'}
                                </span>
                                {extra ? (
                                    <span role='cell' className={`${cell} text-[12px]`} style={{ color: 'var(--v2-soft)' }} data-slot='administration-relation'>
                                        {extra.cell(item, id)}
                                    </span>
                                ) : null}
                                <span role='cell' className='flex justify-end'>
                                    {id !== null && info.detail ? (
                                        <Btn className='text-[12px] font-semibold' aria-label={C.table.details(name)} onClick={() => void openDetail(item)}>
                                            {C.table.view}
                                        </Btn>
                                    ) : null}
                                </span>
                            </div>
                        )
                    })}
                </div>
                {loading && !page ? (
                    <div className='text-center text-[13px]' style={{ padding: '24px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }}>
                        {C.table.reading(info.label)}
                    </div>
                ) : empty ? (
                    <div className='text-center text-[13px]' style={{ padding: '24px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }} data-slot='administration-empty'>
                        {C.table.empty(info.label)}
                    </div>
                ) : page && needle.length > 0 && rows.length === 0 ? (
                    <div className='text-center text-[13px]' style={{ padding: '24px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }} data-slot='administration-no-match'>
                        {C.table.noMatch(info.label, filter, page.offset > 0 || !page.complete)}
                    </div>
                ) : null}
                <div className='flex flex-wrap items-center text-[12px]' style={{ gap: 10, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }} data-slot='administration-foot'>
                    <span>{count}</span>
                    {page && !page.complete ? <span data-slot='administration-partial'>{C.table.partial(info.label)}</span> : null}
                    {page ? (
                        <span className='flex items-center' style={{ gap: 6 }} data-slot='administration-paging'>
                            <Btn tier='tertiary' className='h-7 text-[12px]' disabled={loading || page.offset === 0} onClick={() => void load(0)}>
                                {C.table.firstPage}
                            </Btn>
                            <Btn tier='tertiary' className='h-7 text-[12px]' disabled={loading || page.complete || page.nextOffset === null} onClick={() => page.nextOffset !== null && void load(page.nextOffset)}>
                                {C.table.nextPage}
                            </Btn>
                        </span>
                    ) : null}
                    <span className='ml-auto'>{shown ? C.table.read(time(shown.readAt)) : ''}</span>
                </div>
            </div>
            {detail ? (
                <DetailPanel
                    state={detail}
                    info={info}
                    onClose={() => {
                        // Closing retires the read in flight and the opened item (C39a attempt-01 R2): a late
                        // reply or a later refresh never reopens what was dismissed.
                        detailToken.current += 1
                        opened.current = null
                        setDetail(null)
                    }}
                    onChange={onReview}
                    label={label}
                    admitted={admittedIndex}
                    rotation={(item) => {
                        const id = itemId(info, item)
                        return rotations.find((r) => r.oldKey.id === id && r.stage !== 'complete') ?? null
                    }}
                    onFinishRotation={onFinishRotation}
                />
            ) : null}
        </div>
    )
}

function DetailPanel({
    state,
    info,
    onClose,
    onChange,
    label,
    rotation,
    onFinishRotation,
    admitted,
}: {
    state: DetailState
    info: ResourceInfo
    onClose: () => void
    onChange: (change: ChangeTarget) => void
    label: (action: ChangeTarget['action']) => string
    rotation: (item: Item) => RotationView | null
    onFinishRotation: (application: string) => void
    admitted: (change: ChangeTarget) => number
}) {
    const inRotation = state.kind === 'shown' && state.item ? rotation(state.item) : null
    const headingId = useId()
    const heading = useRef<HTMLHeadingElement>(null)
    useEffect(() => {
        heading.current?.focus()
    }, [state.kind])
    return (
        <section aria-labelledby={headingId} data-slot='administration-detail' className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: 18, borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
            <h2 id={headingId} ref={heading} tabIndex={-1} className='text-[15px] font-semibold outline-none'>
                {state.name}
            </h2>
            {state.kind === 'loading' ? (
                <p role='status' className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                    {C.table.readingDetails}
                </p>
            ) : state.kind === 'failed' ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {state.message}
                </p>
            ) : state.detail.found === 'absent' ? (
                <p className='text-[13px]' data-slot='detail-absent'>
                    {C.table.absent}
                </p>
            ) : state.detail.found === 'incomplete' ? (
                <p className='text-[13px]' data-slot='detail-incomplete'>
                    {C.table.incomplete(info.label)}
                </p>
            ) : (
                <>
                    <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='detail-fields'>
                        {Object.entries(state.item ?? {}).map(([key, value]) => (
                            <div key={key} className='contents'>
                                <dt className='font-mono text-[12px] break-all' style={{ color: 'var(--v2-soft)' }}>
                                    {key}
                                </dt>
                                <dd className='font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                            </div>
                        ))}
                    </dl>
                    {info.id === 'profiles' ? (
                        <p className='rounded-[8px] text-[12px]' style={{ padding: '10px 12px', background: 'var(--v2-inset)', color: 'var(--v2-soft)' }} data-slot='detail-local-preview'>
                            {C.table.localPreview}
                        </p>
                    ) : null}
                </>
            )}
            <div className='flex flex-wrap' style={{ gap: 8 }}>
                {inRotation ? (
                    <Btn tier='primary' data-slot='finish-rotation' onClick={() => onFinishRotation(inRotation.application)}>
                        {C.table.finishRotation}
                    </Btn>
                ) : null}
                {state.kind === 'shown' && state.item
                    ? itemChanges(info, state.item, state.name)
                          .filter((c) => admitted(c) >= 0)
                          .sort((a, b) => admitted(a) - admitted(b))
                          .map((c) => (
                              <Btn key={c.action} onClick={() => onChange(c)}>
                                  {label(c.action)}
                              </Btn>
                          ))
                    : null}
                <Btn tier='tertiary' onClick={onClose}>
                    {C.table.closeDetails}
                </Btn>
            </div>
        </section>
    )
}
