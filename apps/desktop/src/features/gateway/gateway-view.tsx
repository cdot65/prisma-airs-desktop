import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, RefreshCw } from 'lucide-react'
import type {
    FeatureId,
    GatewayDetail,
    GatewayList,
    GatewayModels,
    GatewayPrerequisites,
    GatewayRead,
    GatewayResource,
    ManagementResult,
    PlaneAccess,
    ScopeBinding,
    ShellSnapshot,
    WorkspaceRef,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { fieldText, MANAGEMENT_FAILURE_TEXT, type ObserveManagement, readManagement } from '../administration/management-read'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
}

type Tab = Exclude<GatewayResource, 'workspaces'> | 'prerequisites'

const TABS: readonly [Tab, string][] = [
    ['providers', 'Providers'],
    ['routes', 'Routes'],
    ['guardrails', 'Guardrails'],
    ['serviceKeys', 'Service keys'],
    ['userKeys', 'User keys'],
    ['integrations', 'Integrations'],
    ['prerequisites', 'Inference prerequisites'],
]

const LABEL: Record<GatewayResource, string> = {
    workspaces: 'workspaces',
    providers: 'providers',
    integrations: 'integrations',
    routes: 'routes',
    guardrails: 'guardrails',
    serviceKeys: 'service keys',
    userKeys: 'user keys',
}

/** A read held by the view. */
interface Held<T> {
    read: T | null
    loading: boolean
    failure: string | null
}

const empty = <T,>(): Held<T> => ({ read: null, loading: false, failure: null })

const SCOPE_TEXT: Record<ScopeBinding, string> = {
    bound: 'The workspace’s IAM scope lists this workspace.',
    unbound: 'The workspace’s IAM scope exists but does not list this workspace.',
    absent: 'The service has no IAM scope with this name.',
    denied: 'This management client may not read IAM scopes.',
    failed: 'The IAM scope could not be read.',
}

const PLANE_TEXT: Record<PlaneAccess, string> = {
    granted: 'Readable',
    denied: 'Not allowed for this management client',
    failed: 'Could not be read',
}

function resultFailure(result: ManagementResult | string): string {
    if (typeof result === 'string') return result
    return result.kind === 'failed' ? MANAGEMENT_FAILURE_TEXT[result.failure] : MANAGEMENT_FAILURE_TEXT.invalidResponse
}

function gatewayResult(result: ManagementResult | string) {
    return typeof result !== 'string' && result.kind === 'gateway' ? result.gateway : null
}

const rowsOf = (list: GatewayList): Record<string, unknown>[] => JSON.parse(list.rows) as Record<string, unknown>[]

const nameOf = (row: Record<string, unknown>) =>
    typeof row.name === 'string' && row.name.length > 0 ? row.name : typeof row.slug === 'string' && row.slug.length > 0 ? row.slug : String(row.id)

/** How much of an inventory was read: never "none" from a partial one. */
function coverage(list: GatewayList, what: string): string {
    if (list.complete) return list.count === 0 ? `The service reports no ${what}.` : `${list.count.toLocaleString()} ${what} · complete`
    return list.total !== null
        ? `${list.count.toLocaleString()} of ${list.total.toLocaleString()}+ ${what} read. More exist; they are not shown and not assumed absent.`
        : `${list.count.toLocaleString()} ${what} read. The service did not say whether more exist.`
}

/**
 * AI Gateway workspace and configuration reads (C16). The tenant's
 * workspaces are listed and one is chosen; within it the providers, routes,
 * guardrails and keys are read, with the tenant-wide integrations and their
 * models, and what inference from the workspace would need. Everything is
 * read through the tenant's management client and shown as allow-listed
 * projections: key values, provider keys and credential-bearing settings
 * stay in Rust. Nothing here creates, changes, provisions or sends
 * inference. Keyed by context in the shell; a different workspace starts
 * with nothing held, and a reply for one chosen before is never shown.
 */
export const GatewayView = forwardRef<HTMLHeadingElement, Props>(function GatewayView({ snapshot, dispatch, observe, onNavigate }, ref) {
    const ids = useId()
    const context = snapshot.active?.context ?? null
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const gateway = snapshot.gateway
    const selected = gateway.selected
    const reference = selected?.reference ?? null
    const [listing, setListing] = useState<{ loading: boolean; failure: string | null }>({ loading: false, failure: null })
    const [choosing, setChoosing] = useState<string | null>(null)
    const [tab, setTab] = useState<Tab>('providers')
    const [lists, setLists] = useState<Partial<Record<GatewayResource, Held<GatewayList>>>>({})
    const [prerequisites, setPrerequisites] = useState<Held<GatewayPrerequisites>>(empty)
    // A reply for a superseded read (or another workspace) is dropped.
    const tokens = useRef<Record<string, number>>({})
    const generation = useRef(0)

    async function read(gatewayRead: GatewayRead, scoped: boolean): Promise<ManagementResult | string> {
        if (!context) return 'No tenant is active.'
        if (scoped && !reference) return 'Choose a workspace first.'
        const outcome = await readManagement(dispatch, observe, context, {
            kind: 'gateway',
            workspace: scoped ? (reference as WorkspaceRef) : null,
            read: gatewayRead,
        })
        return outcome.kind === 'notAdmitted' ? outcome.message : outcome.result
    }

    async function listWorkspaces() {
        setListing({ loading: true, failure: null })
        const result = await read({ kind: 'inventory', resource: 'workspaces' }, false)
        const g = gatewayResult(result)
        setListing({ loading: false, failure: g?.kind === 'list' ? null : resultFailure(result) })
    }

    async function choose(workspace: string) {
        if (!context) return
        setChoosing(null)
        const outcome = await dispatch({ kind: 'selectGatewayWorkspace', context, workspace })
        setChoosing(outcome.kind === 'admitted' ? null : outcomeMessage(outcome))
    }

    async function readList(resource: GatewayResource) {
        const gen = generation.current
        const mine = (tokens.current[resource] = (tokens.current[resource] ?? 0) + 1)
        setLists((l) => ({ ...l, [resource]: { ...(l[resource] ?? empty<GatewayList>()), loading: true } }))
        const scoped = resource !== 'integrations'
        const result = await read({ kind: 'inventory', resource }, scoped)
        if (mine !== tokens.current[resource] || gen !== generation.current) return
        const g = gatewayResult(result)
        setLists((l) => ({
            ...l,
            [resource]:
                g?.kind === 'list'
                    ? { read: g.list, loading: false, failure: null }
                    : { ...(l[resource] ?? empty<GatewayList>()), loading: false, failure: resultFailure(result) },
        }))
    }

    async function readPrerequisites() {
        const gen = generation.current
        const mine = (tokens.current.prerequisites = (tokens.current.prerequisites ?? 0) + 1)
        setPrerequisites((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'prerequisites' }, true)
        if (mine !== tokens.current.prerequisites || gen !== generation.current) return
        const g = gatewayResult(result)
        setPrerequisites((h) =>
            g?.kind === 'prerequisites' ? { read: g.prerequisites, loading: false, failure: null } : { ...h, loading: false, failure: resultFailure(result) }
        )
    }

    // The workspaces are listed when the page opens with none held.
    useEffect(() => {
        if (ready && gateway.workspaces === null && !listing.loading && !listing.failure) void listWorkspaces()
    }, [ready, gateway.workspaces === null])

    // A different workspace starts with nothing held.
    useEffect(() => {
        generation.current += 1
        setLists({})
        setPrerequisites(empty)
    }, [reference])

    // Each tab is read when first shown with nothing held.
    useEffect(() => {
        if (!ready || !reference) return
        if (tab === 'prerequisites') {
            if (!prerequisites.read && !prerequisites.loading && !prerequisites.failure) void readPrerequisites()
            return
        }
        const held = lists[tab]
        if (!held) void readList(tab)
    }, [tab, ready, reference, lists, prerequisites])

    const heading = <PageHeading ref={ref} id='gateway-heading' kicker='AI Gateway' title='Gateway workspaces and configuration' />

    if (!tenant || !context) {
        return (
            <section aria-labelledby='gateway-heading' className='flex flex-col gap-4' data-slot='gateway'>
                {heading}
                <p className='text-[14px]'>Add a tenant in Settings to read its AI Gateway workspaces.</p>
                <Button type='button' tier='secondary' className='self-start' onClick={() => onNavigate('settings')}>
                    Open Settings
                </Button>
            </section>
        )
    }

    const workspaces = gateway.workspaces
    return (
        <section aria-labelledby='gateway-heading' className='flex flex-col gap-5' data-slot='gateway'>
            {heading}
            <p className='text-academy-ink-soft max-w-3xl text-[14px]'>
                Read-only view of {tenant.label}’s AI Gateway through its management client. Key values, provider keys and
                credential-bearing settings stay in the desktop’s native core and are never shown. Nothing here creates,
                changes or provisions Gateway resources, and no inference is sent.
            </p>
            {!ready ? (
                <Callout tone='default' icon={Info} data-slot='gateway-not-configured'>
                    AI Gateway reads use the tenant’s management client. Configure one in Settings to see the workspaces.
                </Callout>
            ) : (
                <>
                    <fieldset className='flex flex-col gap-2' data-slot='gateway-workspaces'>
                        <legend className='text-[15px] font-semibold'>Workspace</legend>
                        <div className='flex flex-wrap items-center gap-3'>
                            <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='gateway-workspaces-status'>
                                {listing.loading
                                    ? 'Reading workspaces…'
                                    : workspaces === null
                                      ? 'Workspaces not read yet'
                                      : workspaces.length === 0
                                        ? gateway.complete
                                            ? 'The service reports no active workspaces.'
                                            : 'No workspaces were read. The service did not say none exist.'
                                        : `${workspaces.length} workspace${workspaces.length === 1 ? '' : 's'}${gateway.complete ? '' : ' read · more exist and are not shown'}`}
                            </p>
                            <Button type='button' tier='secondary' size='sm' aria-disabled={listing.loading} onClick={() => !listing.loading && void listWorkspaces()}>
                                <RefreshCw aria-hidden className='size-4' /> Refresh workspaces
                            </Button>
                        </div>
                        {listing.failure ? (
                            <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='gateway-workspaces-failure'>
                                The workspaces could not be read: {listing.failure}
                            </Callout>
                        ) : null}
                        {gateway.rememberedMissing ? (
                            <Callout tone='default' icon={Info} data-slot='gateway-remembered-missing'>
                                The workspace last chosen for this tenant is no longer listed. Choose another.
                            </Callout>
                        ) : gateway.remembered && !selected && workspaces !== null ? (
                            <p className='text-academy-ink-faint text-[12px]' data-slot='gateway-remembered-unconfirmed'>
                                The workspace last chosen for this tenant was not in the workspaces read. It is not chosen for you.
                            </p>
                        ) : null}
                        {workspaces && workspaces.length > 0 ? (
                            <div role='radiogroup' aria-label='Gateway workspace' className='flex flex-col gap-1'>
                                {workspaces.map((w) => (
                                    <label key={w.id} className='flex items-center gap-2 text-[14px]'>
                                        <input
                                            type='radio'
                                            name={`${ids}-workspace`}
                                            value={w.id}
                                            checked={selected?.workspace.id === w.id}
                                            onChange={() => void choose(w.id)}
                                        />
                                        <span>{w.name}</span>
                                        <span className='text-academy-ink-faint font-mono text-[12px]'>{w.slug}</span>
                                    </label>
                                ))}
                            </div>
                        ) : null}
                        {choosing ? (
                            <p role='alert' className='text-academy-accent-warn text-[13px]'>
                                {choosing}
                            </p>
                        ) : null}
                        {selected?.restored ? (
                            <p className='text-academy-ink-faint text-[12px]' data-slot='gateway-restored'>
                                Restored the workspace last chosen for this tenant: it is still listed.
                            </p>
                        ) : null}
                    </fieldset>
                    {selected ? (
                        <>
                            <div role='tablist' aria-label={`Configuration of ${selected.workspace.name}`} className='flex flex-wrap gap-1'>
                                {TABS.map(([id, label]) => (
                                    <Button
                                        key={id}
                                        type='button'
                                        role='tab'
                                        id={`${ids}-tab-${id}`}
                                        aria-selected={tab === id}
                                        aria-controls={`${ids}-panel`}
                                        tier={tab === id ? 'secondary' : 'tertiary'}
                                        size='sm'
                                        onClick={() => setTab(id)}
                                    >
                                        {label}
                                    </Button>
                                ))}
                            </div>
                            <div role='tabpanel' id={`${ids}-panel`} aria-labelledby={`${ids}-tab-${tab}`} className='flex flex-col gap-4'>
                                {tab === 'prerequisites' ? (
                                    <PrerequisitesPanel held={prerequisites} onRefresh={() => void readPrerequisites()} />
                                ) : (
                                    <InventoryPanel
                                        key={`${reference}-${tab}`}
                                        resource={tab}
                                        held={lists[tab] ?? empty<GatewayList>()}
                                        onRefresh={() => void readList(tab)}
                                        read={read}
                                    />
                                )}
                            </div>
                        </>
                    ) : workspaces && workspaces.length > 0 ? (
                        <p className='text-[13px]'>Choose a workspace to read its configuration.</p>
                    ) : null}
                    <p className='text-academy-ink-faint text-[12px]' data-slot='gateway-not-here'>
                        Identity administration (users, roles and IAM scopes beyond the workspace’s binding) is not
                        available in the desktop.
                    </p>
                </>
            )}
        </section>
    )
})

type DetailState =
    | { kind: 'loading'; name: string }
    | { kind: 'failed'; name: string; message: string }
    | { kind: 'shown'; name: string; detail: GatewayDetail }

function InventoryPanel({
    resource,
    held,
    onRefresh,
    read,
}: {
    resource: GatewayResource
    held: Held<GatewayList>
    onRefresh: () => void
    read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>
}) {
    const what = LABEL[resource]
    const [detail, setDetail] = useState<DetailState | null>(null)
    const opener = useRef<HTMLButtonElement | null>(null)
    const token = useRef(0)
    const list = held.read
    const rows = list ? rowsOf(list) : []

    async function open(row: Record<string, unknown>, button: HTMLButtonElement) {
        opener.current = button
        const name = nameOf(row)
        const mine = ++token.current
        setDetail({ kind: 'loading', name })
        const result = await read({ kind: 'detail', resource, id: String(row.id) }, resource !== 'integrations')
        if (mine !== token.current) return
        const g = gatewayResult(result)
        setDetail(g?.kind === 'detail' ? { kind: 'shown', name, detail: g.detail } : { kind: 'failed', name, message: resultFailure(result) })
    }

    function close() {
        token.current += 1
        setDetail(null)
        opener.current?.focus()
    }

    return (
        <div className='flex flex-col gap-3' data-slot='gateway-inventory'>
            <div className='flex flex-wrap items-center gap-3'>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='gateway-inventory-status'>
                    {held.loading ? `Reading ${what}…` : list ? coverage(list, what) : 'Not read yet'}
                </p>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    <RefreshCw aria-hidden className='size-4' /> {`Refresh ${what}`}
                </Button>
            </div>
            {resource === 'integrations' ? (
                <p className='text-academy-ink-faint text-[12px]'>Integrations are tenant-wide; the workspaces granted them share them.</p>
            ) : null}
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='gateway-inventory-failure'>
                    <p>The last read failed: {held.failure}</p>
                    {list ? <p className='mt-1 text-[12px]'>Showing the earlier read. It may be out of date.</p> : null}
                </Callout>
            ) : null}
            {rows.length > 0 ? (
                <table className='w-full border-collapse text-left text-[13px]' data-slot='gateway-table'>
                    <caption className='sr-only'>{`${what.charAt(0).toUpperCase()}${what.slice(1)} read from the service`}</caption>
                    <thead>
                        <tr className='border-academy-rule border-b'>
                            <th scope='col' className='py-2 pr-3 font-medium'>
                                Name
                            </th>
                            <th scope='col' className='py-2 pr-3 font-medium'>
                                Status
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
                        {rows.map((row) => {
                            const name = nameOf(row)
                            return (
                                <tr key={String(row.id)} className='border-academy-rule border-b' data-slot='gateway-row'>
                                    <td className='py-2 pr-3 break-all'>{name}</td>
                                    <td className='text-academy-ink-soft py-2 pr-3'>{fieldText(row.status)}</td>
                                    <td className='text-academy-ink-soft py-2 pr-3 font-mono text-[12px] break-all'>{String(row.id)}</td>
                                    <td className='py-2 text-right'>
                                        <Button type='button' tier='secondary' aria-label={`Details for ${name}`} onClick={(e) => void open(row, e.currentTarget)}>
                                            Details
                                        </Button>
                                    </td>
                                </tr>
                            )
                        })}
                    </tbody>
                </table>
            ) : null}
            {detail ? <DetailPanel state={detail} resource={resource} onClose={close} read={read} /> : null}
        </div>
    )
}

/** Fields shown in a detail: nested ones as JSON text; protected values
 * are never in the projection, only where they are. */
function DetailPanel({
    state,
    resource,
    onClose,
    read,
}: {
    state: DetailState
    resource: GatewayResource
    onClose: () => void
    read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>
}) {
    const headingId = useId()
    const heading = useRef<HTMLHeadingElement>(null)
    const [models, setModels] = useState<Held<GatewayModels>>(empty)
    useEffect(() => {
        heading.current?.focus()
    }, [state.kind])
    const item: Record<string, unknown> | null =
        state.kind === 'shown' && state.detail.item ? (JSON.parse(state.detail.item) as Record<string, unknown>) : null

    async function readModels(id: string) {
        setModels((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'integrationModels', id }, false)
        const g = gatewayResult(result)
        setModels((h) => (g?.kind === 'models' ? { read: g.models, loading: false, failure: null } : { ...h, loading: false, failure: resultFailure(result) }))
    }

    return (
        <section aria-labelledby={headingId} data-slot='gateway-detail' className='border-academy-rule flex flex-col gap-3 rounded-[12px] border p-5'>
            <h2 id={headingId} ref={heading} tabIndex={-1} className='text-[15px] font-semibold outline-none'>
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
                <p className='text-[13px]' data-slot='gateway-detail-absent'>
                    The workspace’s complete inventory does not contain this item, so it is not shown.
                </p>
            ) : state.detail.found === 'incomplete' ? (
                <p className='text-[13px]' data-slot='gateway-detail-incomplete'>
                    This item was not in the part of the workspace’s inventory that could be read. It is not shown, and this
                    does not mean it is absent.
                </p>
            ) : (
                <>
                    {item?.credential_bearing === true ? (
                        <Callout tone='default' icon={Info} data-slot='gateway-credential-bearing'>
                            This configuration carries credentials. Their values are not shown.
                        </Callout>
                    ) : null}
                    <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='gateway-detail-fields'>
                        {Object.entries(item ?? {})
                            .filter(([key]) => key !== 'credential_bearing')
                            .map(([key, value]) => (
                                <div key={key} className='contents'>
                                    <dt className='text-academy-ink-soft font-mono text-[12px] break-all'>{key}</dt>
                                    <dd className='font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                                </div>
                            ))}
                    </dl>
                    {resource === 'integrations' ? (
                        <div className='flex flex-col gap-2' data-slot='gateway-models'>
                            <Button
                                type='button'
                                tier='secondary'
                                size='sm'
                                className='self-start'
                                aria-disabled={models.loading}
                                onClick={() => !models.loading && state.kind === 'shown' && void readModels(state.detail.id)}
                            >
                                {models.read ? 'Refresh models' : 'Read models'}
                            </Button>
                            <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                                {models.loading
                                    ? 'Reading models…'
                                    : models.failure
                                      ? `The models could not be read: ${models.failure}`
                                      : models.read
                                        ? `${models.read.count} model${models.read.count === 1 ? '' : 's'}${models.read.complete ? '' : ' read · more exist'}${models.read.allowAll ? ' · all models allowed' : ''}`
                                        : ''}
                            </p>
                            {models.read ? (
                                <ul className='text-[13px]'>
                                    {(JSON.parse(models.read.models) as Record<string, unknown>[]).map((m) => (
                                        <li key={String(m.slug)}>{`${fieldText(m.name ?? m.slug)} · ${m.enabled === true ? 'enabled' : 'not enabled'}`}</li>
                                    ))}
                                </ul>
                            ) : null}
                        </div>
                    ) : null}
                </>
            )}
            <Button type='button' tier='tertiary' className='self-start' onClick={onClose}>
                Close details
            </Button>
        </section>
    )
}

function sourceText(list: GatewayList | null, failure: string | null, what: string): string {
    if (failure) return `Unavailable: ${failure}`
    return list ? coverage(list, what) : 'Not read'
}

function PrerequisitesPanel({ held, onRefresh }: { held: Held<GatewayPrerequisites>; onRefresh: () => void }) {
    const p = held.read
    const failure = (f: GatewayPrerequisites['routesFailure']) => (f ? MANAGEMENT_FAILURE_TEXT[f] : null)
    return (
        <div className='flex flex-col gap-3' data-slot='gateway-prerequisites'>
            <div className='flex flex-wrap items-center gap-3'>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    {held.loading ? 'Reading prerequisites…' : p ? 'Read from the workspace and its IAM scope.' : 'Not read yet'}
                </p>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    <RefreshCw aria-hidden className='size-4' /> Refresh prerequisites
                </Button>
            </div>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    The prerequisites could not be read: {held.failure}
                </Callout>
            ) : null}
            {p ? (
                <>
                    <dl className='grid grid-cols-[minmax(10rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='gateway-prerequisite-sources'>
                        <dt className='text-academy-ink-soft'>Routes</dt>
                        <dd>{sourceText(p.routes, failure(p.routesFailure), 'routes')}</dd>
                        <dt className='text-academy-ink-soft'>Providers</dt>
                        <dd>{sourceText(p.providers, failure(p.providersFailure), 'providers')}</dd>
                        <dt className='text-academy-ink-soft'>Integrations</dt>
                        <dd>{sourceText(p.integrations, failure(p.integrationsFailure), 'integrations')}</dd>
                        <dt className='text-academy-ink-soft'>Inference keys</dt>
                        <dd>
                            {p.serviceKeys
                                ? `${p.inferenceKeys} of the service keys read may call inference (${sourceText(p.serviceKeys, null, 'service keys')})`
                                : `Unavailable: ${failure(p.serviceKeysFailure) ?? ''}`}
                        </dd>
                        <dt className='text-academy-ink-soft'>IAM scope</dt>
                        <dd data-slot='gateway-scope'>{`${p.scopeName || 'None named'} · ${SCOPE_TEXT[p.scope]}`}</dd>
                        <dt className='text-academy-ink-soft'>Data plane</dt>
                        <dd>{PLANE_TEXT[p.dataPlane]}</dd>
                        <dt className='text-academy-ink-soft'>Admin plane</dt>
                        <dd>{PLANE_TEXT[p.adminPlane]}</dd>
                        <dt className='text-academy-ink-soft'>IAM plane</dt>
                        <dd>{PLANE_TEXT[p.iamPlane]}</dd>
                    </dl>
                    <Callout tone='default' icon={Info} data-slot='gateway-inference-credential'>
                        No Gateway inference credential is configured in the desktop yet, and the management client is never
                        used in its place. Inference from this workspace is not available here.
                    </Callout>
                </>
            ) : null}
        </div>
    )
}
