import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout, Input } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, Lock } from 'lucide-react'
import type {
    ContextRef,
    FeatureId,
    ManagementDetail,
    ManagementPage,
    ManagementResource,
    ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type ChangeTarget, ChangeResults, ChangeWorkbench, TEMPLATES, isGatewayKind, pretty } from './runtime-changes'
import type { Dispatch } from '../tenants/dispatch'
import {
    MANAGEMENT_FAILURE_TEXT,
    type ObserveManagement,
    RESOURCES,
    type ResourceInfo,
    fieldText,
    itemId,
    itemName,
    readManagement,
} from './management-read'

/** Rows per page (the native side accepts 1..=100). */
export const PAGE_SIZE = 25

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
}

type Item = Record<string, unknown>

/** The last page the service returned, kept while a refresh fails. */
interface Shown {
    page: ManagementPage
    items: Item[]
    readAt: Date
}

type DetailState =
    | { kind: 'loading'; name: string }
    | { kind: 'shown'; name: string; detail: ManagementDetail; item: Item | null }
    | { kind: 'failed'; name: string; message: string }

const time = (date: Date) => date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })

function rangeText(page: ManagementPage): string {
    if (page.count === 0) return page.offset === 0 ? 'No items' : `No items from ${page.offset + 1}`
    const first = page.offset + 1
    const last = page.offset + page.count
    if (page.total !== null) return `Items ${first}–${last} of ${page.total}`
    // Without a service total, an incomplete inventory only proves a minimum.
    return page.complete ? `Items ${first}–${last} of ${last}` : `Items ${first}–${last} of ${last}+`
}

/**
 * Read-only management inventories for the active tenant (C08). Every page
 * and detail is one explicit read through the tenant's separate management
 * client; nothing is cached across tenants or credential generations, a
 * failed refresh keeps the previous rows visibly marked with their age, and
 * an inventory is called empty or complete only when the service said so.
 */
export const AdministrationView = forwardRef<HTMLHeadingElement, Props>(function AdministrationView(
    { snapshot, dispatch, observe, onNavigate },
    ref
) {
    const ids = useId()
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const context: ContextRef | null = snapshot.active?.context ?? null
    const ready = tenant?.management.state === 'ready'
    const [resource, setResource] = useState<ManagementResource>('profiles')
    const [shown, setShown] = useState<Shown | null>(null)
    const [failure, setFailure] = useState<string | null>(null)
    const [loading, setLoading] = useState(false)
    const [filter, setFilter] = useState('')
    const [detail, setDetail] = useState<DetailState | null>(null)
    const [change, setChange] = useState<ChangeTarget | null>(null)
    // Each page and detail read carries a token; a reply for a superseded
    // tenant, credential, inventory or newer read is dropped.
    const token = useRef(0)
    const detailToken = useRef(0)
    const info = RESOURCES.find((r) => r.id === resource) ?? (RESOURCES[0] as ResourceInfo)

    async function load(offset: number) {
        if (!context) return
        const mine = ++token.current
        setLoading(true)
        const outcome = await readManagement(dispatch, observe, context, {
            kind: 'page',
            resource,
            offset,
            limit: PAGE_SIZE,
        })
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
            detailToken.current += 1
            setDetail(null)
        } else if (result.kind === 'failed') {
            setFailure(MANAGEMENT_FAILURE_TEXT[result.failure])
        } else {
            setFailure(MANAGEMENT_FAILURE_TEXT.invalidResponse)
        }
    }

    async function openDetail(item: Item) {
        const id = itemId(info, item)
        const name = itemName(info, item)
        if (!context || id === null) return
        const mine = ++detailToken.current
        setDetail({ kind: 'loading', name })
        const outcome = await readManagement(dispatch, observe, context, { kind: 'detail', resource, id })
        if (mine !== detailToken.current) return
        if (outcome.kind === 'notAdmitted') setDetail({ kind: 'failed', name, message: outcome.message })
        else if (outcome.result.kind === 'detail' && outcome.result.detail.id === id) {
            const found = outcome.result.detail
            setDetail({ kind: 'shown', name, detail: found, item: found.item ? (JSON.parse(found.item) as Item) : null })
        } else if (outcome.result.kind === 'failed') {
            setDetail({ kind: 'failed', name, message: MANAGEMENT_FAILURE_TEXT[outcome.result.failure] })
        } else setDetail({ kind: 'failed', name, message: MANAGEMENT_FAILURE_TEXT.invalidResponse })
    }

    useEffect(() => {
        // A new tenant, management credential or inventory starts from the
        // first page with nothing carried over.
        token.current += 1
        detailToken.current += 1
        setShown(null)
        setFailure(null)
        setDetail(null)
        setFilter('')
        setLoading(false)
        if (ready) void load(0)
    }, [context, resource, ready])

    const heading = <PageHeading ref={ref} id='administration-heading' kicker='Administration' title='Tenant inventories' />

    if (!tenant || !context) {
        return (
            <section aria-labelledby='administration-heading' data-slot='administration'>
                {heading}
                <Callout tone='default' icon={Info} data-slot='administration-no-tenant'>
                    <p>Add a tenant in Settings to read its inventories.</p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            </section>
        )
    }

    if (!ready) {
        return (
            <section aria-labelledby='administration-heading' data-slot='administration'>
                {heading}
                <Callout tone='default' icon={Lock} data-slot='administration-not-configured'>
                    <p>
                        {tenant.label} has no ready management client. Inventories need a separate OAuth client;
                        scanning keeps working without one.
                    </p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            </section>
        )
    }

    const needle = filter.trim().toLowerCase()
    const rows = (shown?.items ?? []).filter(
        (item) =>
            needle.length === 0 ||
            itemName(info, item).toLowerCase().includes(needle) ||
            (itemId(info, item) ?? '').toLowerCase().includes(needle)
    )
    const page = shown?.page
    const empty = page !== undefined && !failure && page.count === 0 && page.offset === 0 && page.complete

    return (
        <section aria-labelledby='administration-heading' data-slot='administration' className='flex flex-col gap-5'>
            {heading}
            <p className='text-academy-ink-soft -mt-4 text-[13px]' data-slot='administration-tenant'>
                Read-only inventories of {tenant.label}, read with its management client. Nothing here changes the
                tenant.
            </p>
            <div className='flex flex-col gap-3' data-slot='administration-resources'>
                {(['AI Runtime Security', 'Data Loss Prevention'] as const).map((group) => (
                    <div key={group} role='group' aria-labelledby={`${ids}-${group.replace(/ /g, '-')}`} className='flex flex-col gap-1'>
                        <span id={`${ids}-${group.replace(/ /g, '-')}`} className='text-academy-ink-soft text-[12px] font-medium'>
                            {group}
                        </span>
                        <div className='flex flex-wrap gap-2'>
                            {RESOURCES.filter((r) => r.group === group).map((r) => (
                                <Button
                                    key={r.id}
                                    type='button'
                                    tier={r.id === resource ? 'secondary' : 'tertiary'}
                                    size='sm'
                                    className={r.id === resource ? 'bg-academy-paper-deep font-semibold' : undefined}
                                    aria-current={r.id === resource ? 'true' : undefined}
                                    onClick={() => setResource(r.id)}
                                >
                                    {r.label}
                                </Button>
                            ))}
                        </div>
                    </div>
                ))}
            </div>
            <div className='flex flex-wrap items-end gap-3'>
                <label className='flex min-w-48 flex-1 flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-filter`}>
                    Filter this page
                    <Input
                        id={`${ids}-filter`}
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        autoComplete='off'
                        spellCheck={false}
                        data-slot='administration-filter'
                    />
                </label>
                <Button type='button' tier='secondary' disabled={loading} onClick={() => void load(page?.offset ?? 0)}>
                    Refresh
                </Button>
                {CREATE[resource] ? (
                    <Button type='button' tier='secondary' onClick={() => setChange(CREATE[resource] ?? null)}>
                        {CREATE[resource]?.title}
                    </Button>
                ) : null}
            </div>

            {READ_ONLY[resource] ? (
                <p className='text-academy-ink-faint text-[12px]' data-slot='administration-read-only'>
                    {READ_ONLY[resource]}
                </p>
            ) : null}
            <div role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='administration-progress'>
                {loading ? `Reading ${info.label.toLowerCase()}…` : page ? `${rangeText(page)} · read at ${time(shown.readAt)}` : ''}
            </div>

            {failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='administration-failure'>
                    <p>{failure}</p>
                    {shown ? (
                        <p className='mt-1 text-[12px]' data-slot='administration-stale'>
                            Showing rows read at {time(shown.readAt)}. They may be out of date.
                        </p>
                    ) : null}
                </Callout>
            ) : null}

            {empty ? (
                <Callout tone='default' icon={Info} data-slot='administration-empty'>
                    The service reports no {info.label.toLowerCase()} in this tenant.
                </Callout>
            ) : null}

            {page && page.count > 0 ? (
                <div className='flex flex-col gap-2'>
                    {!page.complete ? (
                        <p className='text-academy-ink-faint text-[12px]' data-slot='administration-partial'>
                            More {info.label.toLowerCase()} follow. Rows not yet read are not shown and are not assumed
                            absent.
                        </p>
                    ) : null}
                    <table className='w-full border-collapse text-left text-[13px]' data-slot='administration-table'>
                        <caption className='sr-only'>{`${info.label}, ${rangeText(page)}`}</caption>
                        <thead>
                            <tr className='border-academy-rule border-b'>
                                <th scope='col' className='py-2 pr-3 font-medium'>
                                    Name
                                </th>
                                <th scope='col' className='py-2 pr-3 font-medium'>
                                    ID
                                </th>
                                <th scope='col' className='py-2 font-medium'>
                                    <span className='sr-only'>Actions</span>
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            {rows.map((item, index) => {
                                const id = itemId(info, item)
                                const name = itemName(info, item)
                                return (
                                    <tr key={`${id ?? 'row'}-${index}`} className='border-academy-rule border-b' data-slot='administration-row'>
                                        <td className='text-academy-ink py-2 pr-3 break-all'>{name}</td>
                                        <td className='text-academy-ink-soft py-2 pr-3 font-mono text-[12px] break-all'>
                                            {id ?? '—'}
                                        </td>
                                        <td className='py-2 text-right'>
                                            {id !== null && info.detail ? (
                                                <Button
                                                    type='button'
                                                    tier='secondary'
                                                    aria-label={`Details for ${name}`}
                                                    onClick={() => void openDetail(item)}
                                                >
                                                    Details
                                                </Button>
                                            ) : null}
                                        </td>
                                    </tr>
                                )
                            })}
                        </tbody>
                    </table>
                    {needle.length > 0 && rows.length === 0 ? (
                        <p className='text-academy-ink-faint text-[12px]'>No rows on this page match the filter.</p>
                    ) : null}
                </div>
            ) : null}

            {page ? (
                <div className='flex flex-wrap gap-2' data-slot='administration-paging'>
                    <Button type='button' tier='secondary' disabled={loading || page.offset === 0} onClick={() => void load(0)}>
                        First page
                    </Button>
                    <Button
                        type='button'
                        tier='secondary'
                        disabled={loading || page.complete || page.nextOffset === null}
                        onClick={() => page.nextOffset !== null && void load(page.nextOffset)}
                    >
                        Next page
                    </Button>
                </div>
            ) : null}

            {detail ? (
                <DetailPanel state={detail} info={info} onClose={() => setDetail(null)} onChange={setChange} />
            ) : null}
            {change ? (
                <ChangeWorkbench
                    key={`${change.kind}-${change.action}-${change.target}`}
                    change={change}
                    review={snapshot.changes.review}
                    staged={snapshot.changes.staged}
                    context={context}
                    dispatch={dispatch}
                    onClose={() => setChange(null)}
                />
            ) : null}
            <ChangeResults results={snapshot.changes.results.filter((r) => !isGatewayKind(r.kind))} dispatch={dispatch} />
        </section>
    )
})

/** The changes v1 offers to start from an inventory. */
const CREATE: Partial<Record<ManagementResource, ChangeTarget>> = {
    profiles: { kind: 'profiles', action: 'create', target: '', title: 'New security profile', body: pretty(TEMPLATES.profile) },
    topics: { kind: 'topics', action: 'create', target: '', title: 'New custom topic', body: pretty(TEMPLATES.topic) },
    apiKeys: { kind: 'apiKeys', action: 'create', target: '', title: 'Create API key', body: pretty(TEMPLATES.key) },
    dataPatterns: { kind: 'dataPatterns', action: 'create', target: '', title: 'New data pattern', body: pretty(TEMPLATES.pattern) },
    dataProfiles: { kind: 'dataProfiles', action: 'create', target: '', title: 'New data profile', body: pretty(TEMPLATES.dataProfile) },
    dictionaries: { kind: 'dictionaries', action: 'create', target: '', title: 'New dictionary', body: JSON.stringify(TEMPLATES.dictionary) },
}

/** Inventories v1 shows read-only. */
const READ_ONLY: Partial<Record<ManagementResource, string>> = {
    deploymentProfiles: 'Read only. Deployment profiles are used when creating API keys; they are not changed here.',
    dlpCatalog: 'Read only. The AIRS DLP profile catalog is not changed here.',
}

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

/** The changes v1 offers for one item. */
function itemChanges(info: ResourceInfo, item: Item, name: string): ChangeTarget[] {
    const kind = CHANGE_KIND[info.id]
    const id = itemId(info, item)
    if (!kind || id === null) return []
    if (kind === 'apiKeys') {
        return [
            { kind, action: 'rotate', target: id, title: `Rotate ${name}`, body: pretty(TEMPLATES.rotate) },
            {
                kind,
                action: 'version',
                target: id,
                title: `Add a key for ${fieldText(item.cust_app)}`,
                body: pretty({ ...TEMPLATES.key, cust_app: item.cust_app ?? '' }),
            },
            { kind, action: 'delete', target: id, title: `Delete ${name}`, body: '{}' },
        ]
    }
    const edit: ChangeTarget = { kind, action: 'update', target: id, title: `Edit ${name}`, body: pretty(item) }
    const remove: ChangeTarget = { kind, action: 'delete', target: id, title: `Delete ${name}`, body: '{}' }
    switch (kind) {
        case 'dictionaries': {
            // v1: a replace re-sends the whole keyword list with the metadata.
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

const ACTION_LABEL: Record<ChangeTarget['action'], string> = {
    create: 'Create…',
    update: 'Edit…',
    delete: 'Delete…',
    version: 'Add key version…',
    rotate: 'Rotate…',
    models: 'Models…',
}

function DetailPanel({
    state,
    info,
    onClose,
    onChange,
}: {
    state: DetailState
    info: ResourceInfo
    onClose: () => void
    onChange: (change: ChangeTarget) => void
}) {
    const headingId = useId()
    const heading = useRef<HTMLHeadingElement>(null)
    useEffect(() => {
        heading.current?.focus()
    }, [state.kind])
    return (
        <section
            aria-labelledby={headingId}
            data-slot='administration-detail'
            className='border-academy-rule flex flex-col gap-3 rounded-[12px] border p-5'
        >
            <h2 id={headingId} ref={heading} tabIndex={-1} className='text-academy-ink text-[15px] font-semibold outline-none'>
                {state.name}
            </h2>
            {state.kind === 'loading' ? (
                <p role='status' className='text-academy-ink-soft text-[13px]'>
                    Reading details…
                </p>
            ) : state.kind === 'failed' ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {state.message}
                </p>
            ) : state.detail.found === 'absent' ? (
                <p className='text-[13px]' data-slot='detail-absent'>
                    The service’s complete inventory no longer contains this item.
                </p>
            ) : state.detail.found === 'incomplete' ? (
                <p className='text-[13px]' data-slot='detail-incomplete'>
                    This item was not within the first 1,000 {info.label.toLowerCase()}. The inventory is larger, so
                    this does not mean it is absent.
                </p>
            ) : (
                <>
                    <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='detail-fields'>
                        {Object.entries(state.item ?? {}).map(([key, value]) => (
                            <div key={key} className='contents'>
                                <dt className='text-academy-ink-soft font-mono text-[12px] break-all'>{key}</dt>
                                <dd className='text-academy-ink font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                            </div>
                        ))}
                    </dl>
                    {info.id === 'profiles' ? (
                        <Callout tone='default' icon={Info} data-slot='detail-local-preview'>
                            Directional settings and customized prompt-injection modes were local previews in the
                            previous desktop. They are not read from or sent to the service here.
                        </Callout>
                    ) : null}
                </>
            )}
            <div className='flex flex-wrap gap-2'>
                {state.kind === 'shown' && state.item
                    ? itemChanges(info, state.item, state.name).map((c) => (
                          <Button key={c.action} type='button' tier='secondary' onClick={() => onChange(c)}>
                              {ACTION_LABEL[c.action]}
                          </Button>
                      ))
                    : null}
                <Button type='button' tier='tertiary' onClick={onClose}>
                    Close details
                </Button>
            </div>
        </section>
    )
}
