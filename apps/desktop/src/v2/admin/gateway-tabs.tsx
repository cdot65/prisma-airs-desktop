import { type FormEvent, useEffect, useId, useRef, useState } from 'react'
import type {
    AdminAction,
    ChangeAction,
    ChangeKind,
    ContextRef,
    FeatureId,
    GatewayDetail,
    GatewayDraft,
    GatewayDraftInput,
    GatewayList,
    GatewayLocalView,
    GatewayLogs,
    GatewayModels,
    GatewayRead,
    GatewayRecord,
    GatewayResource,
    ListedWorkspace,
    ManagementResult,
    ObserveRead,
    ShellSnapshot,
    WorkspaceRef,
} from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, fieldText, readManagement } from '../../features/administration/management-read'
import { CHANGE_NOUN, type ChangeTarget, ProvisioningSteps, isGatewayKind, pretty, verbOf } from '../../features/administration/runtime-changes'
import { type Dispatch, outcomeMessage, type DispatchOutcome } from '../../features/tenants/dispatch'
import { clockOf } from '../copy'
import { Btn, Field } from '../controls'
import { Glyph } from '../icons'
import { ADMIN_COPY as C } from './copy'

/** A change the Gateway tabs open in the review sheet, with how its draft is kept on this Mac (C17). */
export interface GatewayReviewExtras {
    /** Keeps the edited text on this Mac; the sheet reports the save's outcome and the core's word on the draft. */
    onDraft?: (body: string) => void | Promise<DispatchOutcome>
}

interface Held<T> {
    read: T | null
    loading: boolean
    failure: string | null
}
const empty = <T,>(): Held<T> => ({ read: null, loading: false, failure: null })

const LABEL: Record<GatewayResource, string> = { workspaces: 'workspaces', providers: 'providers', integrations: 'integrations', routes: 'routes', guardrails: 'guardrails', serviceKeys: 'service keys', userKeys: 'user keys' }
const KIND: Record<GatewayResource, ChangeKind> = { workspaces: 'gatewayWorkspaces', providers: 'gatewayProviders', routes: 'gatewayRoutes', guardrails: 'gatewayGuardrails', serviceKeys: 'gatewayServiceKeys', userKeys: 'gatewayUserKeys', integrations: 'gatewayIntegrations' }
const RESOURCE_OF = Object.fromEntries(Object.entries(KIND).map(([r, k]) => [k, r])) as Record<ChangeKind, GatewayResource>
const NOUN: Record<GatewayResource, string> = { workspaces: 'workspace', providers: 'provider', routes: 'route', guardrails: 'guardrail', serviceKeys: 'service key', userKeys: 'user key', integrations: 'integration' }
/** Editor templates (v1 field names). Credentials are never part of one. */
const CREATE_TEMPLATE: Record<GatewayResource, Record<string, unknown>> = {
    workspaces: { name: '', scope_name: '', existing_scope: false, description: '' },
    providers: { name: '', slug: '', ai_provider_id: '', integration_id: '' },
    routes: { name: '', config: { strategy: { mode: 'fallback' }, targets: [{ provider: '@provider-slug' }] } },
    guardrails: { name: '', checks: [], actions: { deny: true } },
    serviceKeys: { name: '', description: '', scopes: ['completions.write'], type: 'service' },
    userKeys: { name: '', scopes: ['completions.write'], type: 'user', user_id: '' },
    integrations: { name: '', slug: '', ai_provider_id: '', description: '' },
}
/** Kinds whose changes apply inside the chosen workspace. */
const inWorkspace = (kind: ChangeKind) => kind !== 'gatewayWorkspaces' && kind !== 'gatewayIntegrations'

type Opened = ChangeTarget & { home?: ListedWorkspace | null }

/** What the operator chose to change in the Gateway. */
function gatewayChange(resource: GatewayResource, action: ChangeAction, target: string, name: string, workspace: WorkspaceRef | null, body?: Record<string, unknown>): ChangeTarget {
    const noun = NOUN[resource]
    const title =
        action === 'create'
            ? resource === 'workspaces'
                ? 'Provision a workspace'
                : `New ${noun}`
            : action === 'delete'
              ? `${resource === 'workspaces' ? 'Archive' : 'Delete'} ${noun} ${name}`
              : action === 'rotate'
                ? `Rotate ${noun} ${name}`
                : action === 'models'
                  ? `Models of ${name}`
                  : `Edit ${noun} ${name}`
    const scoped = resource !== 'workspaces' && resource !== 'integrations'
    const change: ChangeTarget = { kind: KIND[resource], action, target, title, body: action === 'delete' ? '{}' : pretty(body ?? (action === 'create' ? CREATE_TEMPLATE[resource] : { name })) }
    if (scoped && workspace) change.workspace = workspace
    return change
}

function resultFailure(result: ManagementResult | string): string {
    if (typeof result === 'string') return result
    return result.kind === 'failed' ? MANAGEMENT_FAILURE_TEXT[result.failure] : MANAGEMENT_FAILURE_TEXT.invalidResponse
}
const gatewayResult = (result: ManagementResult | string) => (typeof result !== 'string' && result.kind === 'gateway' ? result.gateway : null)
const rowsOf = (list: GatewayList): Record<string, unknown>[] => JSON.parse(list.rows) as Record<string, unknown>[]
const nameOf = (row: Record<string, unknown>) => (typeof row.name === 'string' && row.name.length > 0 ? row.name : typeof row.slug === 'string' && row.slug.length > 0 ? row.slug : String(row.id))

/** How much of an inventory was read: never "none" from a partial one. */
function coverage(list: GatewayList, what: string): string {
    if (list.complete) return list.count === 0 ? `The service reports no ${what}.` : `${list.count.toLocaleString()} ${what} · complete`
    return list.total !== null
        ? `${list.count.toLocaleString()} of ${list.total.toLocaleString()}+ ${what} read. More exist; they are not shown and not assumed absent.`
        : `${list.count.toLocaleString()} ${what} read. The service did not say whether more exist.`
}

const admittedLabel = (actions: AdminAction[], kind: ChangeKind, action: ChangeAction) => actions.find((a) => a.kind === kind && a.action === action)?.label
/** The inventory's admitted row actions for an item of `resource`. */
const itemActions = (resource: GatewayResource, admitted: AdminAction[]): [ChangeAction, string][] => admitted.filter((a) => a.kind === KIND[resource] && a.row).map((a) => [a.action, a.label])

const KEPT_TIME = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
const keptAt = (iso: string) => {
    const date = new Date(iso)
    return Number.isNaN(date.getTime()) ? iso : KEPT_TIME.format(date)
}
function recordText(r: GatewayRecord): string {
    const when = `when it settled (${keptAt(r.settledAt)})`
    const known = r.outcome === 'unknown' ? `Its outcome was not known ${when}.` : r.provisioning ? `Some steps completed and others did not ${when}.` : `It was applied, but reading it back did not confirm the reviewed change ${when}.`
    return `${known} The service has not been read since: refresh before trying again. Nothing here is sent unless you review it again.`
}
function keptLabel(k: GatewayDraft | GatewayRecord): string {
    const name = 'name' in k && k.name ? k.name : k.action === 'create' ? '' : k.target
    return `${verbOf(k.kind, k.action)} ${CHANGE_NOUN[k.kind]}${name ? ` ${name}` : ''}${k.workspace ? ` in ${k.workspace.name}` : ''}`
}
const INFERENCE_FAILURE_TEXT: Record<string, string> = {
    blocked: 'A Gateway guardrail blocked the request or its response.',
    unauthorized: 'The Gateway did not accept the inference key. Store the key again.',
    denied: 'The inference key may not use this model or route.',
    notFound: 'The Gateway did not find this model, route or endpoint.',
    rejected: 'The Gateway rejected the request.',
    rateLimited: 'The Gateway is limiting requests. Wait, then run again.',
    unavailable: 'The Gateway or its provider failed.',
    timeout: 'The run took too long and was stopped.',
    network: 'The Gateway could not be reached, or the connection ended.',
    destination: 'The Gateway tried to send the run somewhere else. Nothing was followed.',
    invalidResponse: 'The Gateway sent something that is not a valid streamed answer. The run ended there.',
    modelFailed: 'The model failed while answering. The Gateway reported the response as failed.',
    credentialLocked: 'The Keychain is locked. Unlock it and run again.',
    credentialDenied: 'macOS did not allow access to the inference key.',
    credentialMissing: 'The inference key is missing. Store it again.',
    configuration: 'The run could not be set up with this endpoint and key.',
}
const fileFailureText = (failure: string) => (failure === 'invalidResponse' ? 'The Gateway’s answer did not describe that file.' : (INFERENCE_FAILURE_TEXT[failure] ?? failure))
function sizeText(bytes: number): string {
    if (bytes < 1024) return `${bytes} ${bytes === 1 ? 'byte' : 'bytes'}`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
}
export const DRAFT_PROBLEM_TEXT: Record<string, string> = {
    credential: 'This text names a credential field, so it is not kept on this Mac. Credentials are never entered here.',
    incomplete: 'Only complete JSON is kept on this Mac, so these edits are not kept yet: the last complete text is. Finish the document to keep it.',
    tooLarge: 'This text is too large to keep on this Mac; the last text that fit is kept.',
}

const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
const cardStyle = { borderColor: 'var(--v2-rule)' }
const grid = 'minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 1.6fr) 90px'

export type GatewayTabId = 'overview' | 'workspaces' | 'routes' | 'providers' | 'guardrails' | 'keys' | 'integrations' | 'logs' | 'mcp-servers'

interface Props {
    snapshot: ShellSnapshot
    context: ContextRef
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
    tab: GatewayTabId
    /** The tab's admitted actions (`AdminInventory`). */
    actions: AdminAction[]
    onReview: (change: ChangeTarget, extras: GatewayReviewExtras) => void
    onRead: () => void
}

/**
 * AI Gateway's tabs as R2App's product card draws them (C39b), over the C16–
 * C18 reads: the workspaces listed and one chosen, within it the routes,
 * providers, guardrails, keys and the tenant-wide integrations as tables
 * with a View per row and the inventory's admitted actions in the detail,
 * the workspace's own settings, the request logs, the chosen workspace's MCP
 * catalog, and the Overview from the tenant's read. Everything is read
 * through the management client as allow-listed projections; key values and
 * credential-bearing settings stay in Rust. One instance serves one tenant
 * and credential generation; a different workspace starts with nothing held.
 */
export function GatewayProduct({ snapshot, context, dispatch, observe, onNavigate, tab, actions, onReview, onRead }: Props) {
    const ids = useId()
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const gateway = snapshot.gateway
    const selected = gateway.selected
    const reference = selected?.reference ?? null
    const [listing, setListing] = useState<{ loading: boolean; failure: string | null }>({ loading: false, failure: null })
    const [choosing, setChoosing] = useState<string | null>(null)
    const [lists, setLists] = useState<Partial<Record<GatewayResource, Held<GatewayList>>>>({})
    const [space, setSpace] = useState<Held<GatewayDetail>>(empty)
    const [keyKind, setKeyKind] = useState<'serviceKeys' | 'userKeys'>('serviceKeys')
    const local = gateway.local
    const settledSeen = useRef<Set<string> | null>(null)
    const tokens = useRef<Record<string, number>>({})
    const generation = useRef(0)

    async function read(gatewayRead: GatewayRead, scoped: boolean): Promise<ManagementResult | string> {
        if (scoped && !reference) return 'Choose a workspace first.'
        const outcome = await readManagement(dispatch, observe, context, { kind: 'gateway', workspace: scoped ? (reference as WorkspaceRef) : null, read: gatewayRead })
        return outcome.kind === 'notAdmitted' ? outcome.message : outcome.result
    }
    async function listWorkspaces() {
        setListing({ loading: true, failure: null })
        const result = await read({ kind: 'inventory', resource: 'workspaces' }, false)
        const g = gatewayResult(result)
        setListing({ loading: false, failure: g?.kind === 'list' ? null : resultFailure(result) })
    }
    async function choose(workspace: string) {
        setChoosing(null)
        const outcome = await dispatch({ kind: 'selectGatewayWorkspace', context, workspace })
        setChoosing(outcome.kind === 'admitted' ? null : outcomeMessage(outcome))
    }
    async function readList(resource: GatewayResource) {
        const gen = generation.current
        const mine = (tokens.current[resource] = (tokens.current[resource] ?? 0) + 1)
        setLists((l) => ({ ...l, [resource]: { ...(l[resource] ?? empty<GatewayList>()), loading: true } }))
        const result = await read({ kind: 'inventory', resource }, resource !== 'integrations')
        if (mine !== tokens.current[resource] || gen !== generation.current) return
        const g = gatewayResult(result)
        setLists((l) => ({ ...l, [resource]: g?.kind === 'list' ? { read: g.list, loading: false, failure: null } : { ...(l[resource] ?? empty<GatewayList>()), loading: false, failure: resultFailure(result) } }))
    }
    async function readWorkspace() {
        if (!selected) return
        const gen = generation.current
        const mine = (tokens.current.workspace = (tokens.current.workspace ?? 0) + 1)
        setSpace((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'detail', resource: 'workspaces', id: selected.workspace.id }, false)
        if (mine !== tokens.current.workspace || gen !== generation.current) return
        const g = gatewayResult(result)
        setSpace((h) => (g?.kind === 'detail' ? { read: g.detail, loading: false, failure: null } : { ...h, loading: false, failure: resultFailure(result) }))
    }
    /** Keeps the edited text on this Mac (never reviewed or sent). */
    /** Keeps the edited text on this Mac (never reviewed or sent); the sheet shows what the core made of it. */
    function keepDraft(target: ChangeTarget, body: string) {
        const draft: GatewayDraftInput = { kind: target.kind, action: target.action, target: target.target, body }
        if (target.draftId) draft.id = target.draftId
        else if (target.workspace) draft.workspace = target.workspace
        return dispatch({ kind: 'saveGatewayDraft', context, draft })
    }
    function start(next: Opened) {
        // A change opened from this Mac is reviewed only in its own workspace, with the reference chosen now.
        const change: ChangeTarget =
            next.home !== undefined && inWorkspace(next.kind)
                ? next.home && selected?.workspace.id === next.home.id && reference
                    ? { ...next, workspace: reference }
                    : { ...next, workspace: undefined, reviewBlocked: `This draft is for workspace ${next.home?.name ?? 'unknown'}. Choose that workspace above to review it.` }
                : next
        onReview(change, { onDraft: change.action === 'delete' ? undefined : (body) => keepDraft(change, body) })
    }
    function openKept(kept: GatewayDraft | GatewayRecord, draftId: string | undefined) {
        const resource = RESOURCE_OF[kept.kind]
        const name = 'name' in kept && kept.name ? kept.name : kept.target
        const base = gatewayChange(resource, kept.action, kept.target, name, null)
        start({ ...base, body: kept.body, draftId, home: kept.workspace })
    }

    // Gateway work kept on this Mac is read when the product opens (offline too).
    useEffect(() => {
        if (local.state === 'idle') void dispatch({ kind: 'openGatewayLocal', context })
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per idle store
    }, [context, local.state === 'idle'])
    // The workspaces are listed when the product opens with none held.
    useEffect(() => {
        if (ready && gateway.workspaces === null && !listing.loading && !listing.failure) void listWorkspaces()
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per missing list
    }, [ready, gateway.workspaces === null])
    // A different workspace starts with nothing held.
    useEffect(() => {
        generation.current += 1
        setLists({})
        setSpace(empty)
    }, [reference])
    // A settled change refreshes what it changed.
    const gatewayResults = snapshot.changes.results.filter((r) => isGatewayKind(r.kind))
    const settledKey = gatewayResults.map((r) => `${r.operation}:${r.settled}`).join(',')
    useEffect(() => {
        const settled = gatewayResults.filter((r) => r.settled)
        if (settledSeen.current === null) {
            settledSeen.current = new Set(settled.map((r) => r.operation))
            return
        }
        for (const r of settled) {
            if (settledSeen.current.has(r.operation)) continue
            settledSeen.current.add(r.operation)
            const resource = RESOURCE_OF[r.kind]
            if (resource === 'workspaces') {
                void listWorkspaces()
                setSpace(empty)
            } else setLists((l) => ({ ...l, [resource]: undefined }))
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- on each settled change
    }, [settledKey])
    // Each inventory tab is read when first shown with nothing held.
    const resource: GatewayResource | null = tab === 'keys' ? keyKind : tab === 'routes' || tab === 'providers' || tab === 'guardrails' || tab === 'integrations' ? tab : null
    useEffect(() => {
        if (!ready) return
        if (tab === 'workspaces') {
            if (reference && !space.read && !space.loading && !space.failure) void readWorkspace()
            return
        }
        if (!resource) return
        if (resource !== 'integrations' && !reference) return
        if (!lists[resource]) void readList(resource)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- per shown tab
    }, [tab, resource, ready, reference, lists, space])

    if (!tenant) return null
    if (!ready) {
        return (
            <p className='text-[13px]' style={soft} data-slot='gateway-not-configured'>
                AI Gateway reads use the tenant’s management client. Configure one in Settings to see the workspaces.
            </p>
        )
    }
    const workspaces = gateway.workspaces
    const chooser = (
        <fieldset className='flex flex-col' style={{ gap: 8 }} data-slot='gateway-workspaces'>
            <legend className='text-[13px] font-semibold'>Workspace</legend>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <p role='status' aria-live='polite' className='text-[12px]' style={soft} data-slot='gateway-workspaces-status'>
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
                <Btn tier='tertiary' aria-disabled={listing.loading} onClick={() => !listing.loading && void listWorkspaces()}>
                    Refresh workspaces
                </Btn>
                {tab === 'workspaces' && admittedLabel(actions, 'gatewayWorkspaces', 'create') ? (
                    <Btn tier='primary' className='ml-auto' onClick={() => start(gatewayChange('workspaces', 'create', '', '', null))}>
                        {admittedLabel(actions, 'gatewayWorkspaces', 'create')}
                    </Btn>
                ) : null}
            </div>
            {listing.failure ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }} data-slot='gateway-workspaces-failure'>
                    The workspaces could not be read: {listing.failure}
                </p>
            ) : null}
            {gateway.rememberedMissing ? (
                <p className='text-[12px]' style={faint} data-slot='gateway-remembered-missing'>
                    The workspace last chosen for this tenant is no longer listed. Choose another.
                </p>
            ) : gateway.remembered && !selected && workspaces !== null ? (
                <p className='text-[12px]' style={faint} data-slot='gateway-remembered-unconfirmed'>
                    The workspace last chosen for this tenant was not in the workspaces read. It is not chosen for you.
                </p>
            ) : null}
            {workspaces && workspaces.length > 0 ? (
                <div role='radiogroup' aria-label='Gateway workspace' className='flex flex-col' style={{ gap: 4 }}>
                    {workspaces.map((w) => (
                        <label key={w.id} className='flex items-center text-[13px]' style={{ gap: 8 }}>
                            <input type='radio' name={`${ids}-workspace`} value={w.id} aria-label={w.name} checked={selected?.workspace.id === w.id} onChange={() => void choose(w.id)} className='size-4 min-h-0' />
                            <span>{w.name}</span>
                            <span className='font-mono text-[12px]' style={faint}>
                                {w.slug}
                            </span>
                        </label>
                    ))}
                </div>
            ) : null}
            {choosing ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {choosing}
                </p>
            ) : null}
            {selected?.restored ? (
                <p className='text-[12px]' style={faint} data-slot='gateway-restored'>
                    Restored the workspace last chosen for this tenant: it is still listed.
                </p>
            ) : null}
        </fieldset>
    )
    const needsWorkspace = (
        <>
            {chooser}
            {workspaces && workspaces.length > 0 ? (
                <p className='text-[13px]' style={soft}>
                    Choose a workspace to read its configuration.
                </p>
            ) : null}
        </>
    )
    if (tab === 'overview') return <GatewayOverview snapshot={snapshot} onRead={onRead} chooser={selected ? null : chooser} />
    if (tab === 'workspaces') {
        return (
            <div className='flex flex-col' style={{ gap: 18 }}>
                {chooser}
                {selected ? <WorkspacePanel name={selected.workspace.name} held={space} onRefresh={() => void readWorkspace()} actions={actions} onChange={(action) => start(gatewayChange('workspaces', action, selected.workspace.id, selected.workspace.name, null))} /> : null}
                {selected && tenant ? <WorkspaceKey key={`${tenant.id}:${selected.workspace.id}`} tenant={tenant} selected={selected} context={context} dispatch={dispatch} /> : null}
                <LocalWork local={local} context={context} dispatch={dispatch} onOpen={openKept} />
                <p className='text-[12px]' style={faint} data-slot='gateway-not-here'>
                    Identity administration (users, roles and IAM scopes beyond the workspace’s binding) is not available in the desktop.
                </p>
            </div>
        )
    }
    if (tab === 'logs') return selected ? <LogsPanel key={reference ?? ''} workspace={selected.workspace.name} read={read} /> : needsWorkspace
    if (tab === 'mcp-servers') return selected ? <McpServers snapshot={snapshot} dispatch={dispatch} context={context} /> : needsWorkspace
    if (!resource) return null
    if (resource !== 'integrations' && !selected) return needsWorkspace
    return (
        <div className='flex flex-col' style={{ gap: 14 }}>
            {tab === 'keys' ? (
                <div role='radiogroup' aria-label={C.keyKinds} className='flex self-start rounded-[8px]' style={{ gap: 2, padding: 2, background: 'var(--v2-inset)' }}>
                    {(['serviceKeys', 'userKeys'] as const).map((k) => (
                        <button
                            key={k}
                            type='button'
                            role='radio'
                            aria-checked={keyKind === k}
                            onClick={() => setKeyKind(k)}
                            className='h-7 min-h-0 rounded-[6px] px-3 text-[12px] font-semibold'
                            style={{ background: keyKind === k ? 'var(--v2-paper)' : 'transparent', boxShadow: keyKind === k ? '0 1px 2px rgba(0,0,0,0.12)' : 'none', color: 'var(--v2-ink)' }}
                        >
                            {k === 'serviceKeys' ? C.serviceKeys : C.userKeys}
                        </button>
                    ))}
                </div>
            ) : null}
            <InventoryPanel
                key={`${reference ?? ''}-${resource}`}
                resource={resource}
                held={lists[resource] ?? empty<GatewayList>()}
                onRefresh={() => void readList(resource)}
                read={read}
                actions={actions}
                onChange={(action, target, name, body) => start(gatewayChange(resource, action, target, name, reference as WorkspaceRef | null, body))}
            />
        </div>
    )
}

/** The Overview: the Gateway day of the tenant's read (C36's figures), never invented. */
function GatewayOverview({ snapshot, onRead, chooser }: { snapshot: ShellSnapshot; onRead: () => void; chooser: React.ReactNode }) {
    const g = snapshot.overview.read?.gateway ?? null
    const readAt = snapshot.overview.read?.readAt ?? null
    const n = (v: number | null | undefined) => (v === null || v === undefined ? '—' : v.toLocaleString())
    const dollars = (cents: number | null | undefined) => (cents === null || cents === undefined ? '—' : (cents / 100).toLocaleString(undefined, { style: 'currency', currency: 'USD' }))
    const rate = g && g.requests !== null && g.errors !== null && g.requests > 0 ? `${((g.errors / g.requests) * 100).toFixed(1)}%` : '—'
    const sub = !g ? (chooser ? 'Choose a workspace' : 'Not read yet') : g.usageFailure ? `Usage not read: ${MANAGEMENT_FAILURE_TEXT[g.usageFailure]}` : g.partial ? 'Part of the day only' : readAt ? `From the read at ${clockOf(readAt)}` : ''
    const tiles = [
        { label: 'Requests · today', value: n(g?.requests), sub },
        { label: 'Error rate', value: rate, sub: g && g.errors !== null ? `${n(g.errors)} errors` : 'Errors unknown', hot: g !== null && g.errors !== null && g.requests !== null && g.requests > 0 && g.errors / g.requests > 0.05 },
        { label: 'Cost · today', value: dollars(g?.cost), sub: g?.limitsFailure ? `Limits not read: ${MANAGEMENT_FAILURE_TEXT[g.limitsFailure]}` : g?.policies !== null && g?.policies !== undefined ? `${g.policies} limit polic${g.policies === 1 ? 'y' : 'ies'}` : 'Limit policies unknown' },
    ]
    return (
        <div className='flex flex-col' style={{ gap: 16 }} data-slot='admin-gateway-overview'>
            {chooser}
            {g ? (
                <p className='text-[12px]' style={soft}>{`Workspace ${g.workspaceName}`}</p>
            ) : null}
            <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 150px), 1fr))', gap: 16 }}>
                {tiles.map((t) => (
                    <div key={t.label} className='rounded-[12px]' style={{ padding: '16px 18px', background: 'var(--v2-inset)' }} data-slot='admin-kpi'>
                        <div className='text-[12px]' style={faint}>
                            {t.label}
                        </div>
                        <div className='text-[22px] font-bold tabular-nums' style={{ color: t.hot ? 'var(--v2-block)' : 'var(--v2-ink)' }}>
                            {t.value}
                        </div>
                        <div className='text-[12px] font-semibold' style={soft}>
                            {t.sub}
                        </div>
                    </div>
                ))}
            </div>
            <div>
                <Btn onClick={onRead}>{C.overviewRuntime.readNow}</Btn>
            </div>
        </div>
    )
}

type DetailState = { kind: 'loading'; id: string; name: string } | { kind: 'failed'; id: string; name: string; message: string } | { kind: 'shown'; id: string; name: string; detail: GatewayDetail }
type StartChange = (action: ChangeAction, target: string, name: string, body?: Record<string, unknown>) => void

function InventoryPanel({ resource, held, onRefresh, read, onChange, actions }: { resource: GatewayResource; held: Held<GatewayList>; onRefresh: () => void; read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>; onChange: StartChange; actions: AdminAction[] }) {
    const create = actions.find((a) => a.kind === KIND[resource] && a.action === 'create' && !a.row)?.label
    const what = LABEL[resource]
    const [detail, setDetail] = useState<DetailState | null>(null)
    const [filter, setFilter] = useState('')
    const token = useRef(0)
    const list = held.read
    const needle = filter.trim().toLowerCase()
    const rows = (list ? rowsOf(list) : []).filter((row) => needle.length === 0 || nameOf(row).toLowerCase().includes(needle) || String(row.id).toLowerCase().includes(needle))
    async function open(row: Record<string, unknown>) {
        const name = nameOf(row)
        const id = String(row.id)
        const mine = ++token.current
        setDetail({ kind: 'loading', id, name })
        const result = await read({ kind: 'detail', resource, id }, resource !== 'integrations')
        if (mine !== token.current) return
        const g = gatewayResult(result)
        setDetail(g?.kind === 'detail' ? { kind: 'shown', id, name, detail: g.detail } : { kind: 'failed', id, name, message: resultFailure(result) })
    }
    function close() {
        token.current += 1
        setDetail(null)
    }
    return (
        <div className='flex min-w-0 flex-col' style={{ gap: 14 }} data-slot='gateway-inventory'>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <label className='flex items-center rounded-[7px] text-[12px]' style={{ gap: 7, width: 280, maxWidth: '100%', height: 32, padding: '0 9px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                    <Glyph name='search' size={12} />
                    <input aria-label={C.table.search(what)} placeholder={C.table.search(what)} value={filter} onChange={(e) => setFilter(e.target.value)} autoComplete='off' spellCheck={false} className='min-h-0 min-w-0 flex-grow border-0 bg-transparent p-0 text-[13px]' style={{ border: 0, minHeight: 0, borderRadius: 0, background: 'transparent', color: 'var(--v2-ink)', height: 24 }} />
                </label>
                <Btn tier='tertiary' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    {`Refresh ${what}`}
                </Btn>
                {create ? (
                    <Btn tier='primary' className='ml-auto' onClick={() => onChange('create', '', '')}>
                        {create}
                    </Btn>
                ) : null}
            </div>
            <p role='status' aria-live='polite' className='sr-only' data-slot='gateway-inventory-status'>
                {held.loading ? `Reading ${what}…` : list ? coverage(list, what) : 'Not read yet'}
            </p>
            {resource === 'integrations' ? (
                <p className='text-[12px]' style={faint}>
                    Integrations are tenant-wide; the workspaces granted them share them.
                </p>
            ) : null}
            {held.failure ? (
                <div role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-flag-bg)' }} data-slot='gateway-inventory-failure'>
                    <p>The last read failed: {held.failure}</p>
                    {list ? <p className='mt-1 text-[12px]'>Showing the earlier read. It may be out of date.</p> : null}
                </div>
            ) : null}
            <div className='overflow-hidden rounded-[12px] border' style={cardStyle}>
                <div role='table' aria-label={`${what.charAt(0).toUpperCase()}${what.slice(1)}`} data-slot='gateway-table'>
                    <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: grid, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                        <span role='columnheader'>Name</span>
                        <span role='columnheader'>Status</span>
                        <span role='columnheader'>ID</span>
                        <span role='columnheader'>
                            <span className='sr-only'>Actions</span>
                        </span>
                    </div>
                    {rows.map((row) => {
                        const name = nameOf(row)
                        return (
                            <div key={String(row.id)} role='row' className='grid items-center' style={{ gridTemplateColumns: grid, gap: 12, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)' }} data-slot='gateway-row'>
                                <span role='cell' className='min-w-0 truncate font-mono text-[12px] font-semibold' data-label='Name'>
                                    {name}
                                </span>
                                <span role='cell' className='min-w-0 truncate text-[12px]' style={soft} data-label='Status'>
                                    {fieldText(row.status)}
                                </span>
                                <span role='cell' className='min-w-0 truncate font-mono text-[12px]' style={soft} data-label='ID'>
                                    {String(row.id)}
                                </span>
                                <span role='cell' className='flex justify-end'>
                                    <Btn className='text-[12px] font-semibold' aria-label={`Details for ${name}`} onClick={() => void open(row)}>
                                        {C.table.view}
                                    </Btn>
                                </span>
                            </div>
                        )
                    })}
                </div>
                {held.loading && !list ? (
                    <div className='text-center text-[13px]' style={{ padding: '24px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }}>{`Reading ${what}…`}</div>
                ) : list && rows.length === 0 ? (
                    <div className='text-center text-[13px]' style={{ padding: '24px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }}>
                        {needle ? C.table.noMatch(what, filter) : coverage(list, what)}
                    </div>
                ) : null}
                <div className='flex flex-wrap items-center text-[12px]' style={{ gap: 10, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }} data-slot='gateway-foot'>
                    <span>{list ? coverage(list, what) : 'Not read yet'}</span>
                </div>
            </div>
            {detail ? <DetailPanel key={detail.id} state={detail} resource={resource} onClose={close} read={read} onChange={onChange} actions={actions} /> : null}
        </div>
    )
}

/** Fields shown in a detail: nested ones as JSON text; protected values are never in the projection, only where they are. */
function DetailPanel({ state, resource, onClose, read, onChange, actions }: { state: DetailState; resource: GatewayResource; onClose: () => void; read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>; onChange: StartChange; actions: AdminAction[] }) {
    const headingId = useId()
    const heading = useRef<HTMLHeadingElement>(null)
    const [models, setModels] = useState<Held<GatewayModels>>(empty)
    const shownId = useRef<string | null>(state.id)
    shownId.current = state.id
    useEffect(() => () => void (shownId.current = null), [])
    useEffect(() => {
        heading.current?.focus()
    }, [state.kind])
    const item: Record<string, unknown> | null = state.kind === 'shown' && state.detail.item ? (JSON.parse(state.detail.item) as Record<string, unknown>) : null
    async function readModels(id: string) {
        setModels((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'integrationModels', id }, false)
        if (shownId.current !== id) return
        const g = gatewayResult(result)
        setModels((h) => (g?.kind === 'models' && g.models.integration === id ? { read: g.models, loading: false, failure: null } : { ...h, loading: false, failure: resultFailure(result) }))
    }
    return (
        <section aria-labelledby={headingId} data-slot='gateway-detail' className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: 18, ...cardStyle, background: 'var(--v2-paper)' }}>
            <h2 id={headingId} ref={heading} tabIndex={-1} className='text-[15px] font-semibold outline-none'>
                {state.name}
            </h2>
            {state.kind === 'loading' ? (
                <p role='status' className='text-[13px]' style={soft}>
                    Reading details…
                </p>
            ) : state.kind === 'failed' ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {state.message}
                </p>
            ) : state.detail.found === 'absent' ? (
                <p className='text-[13px]' data-slot='gateway-detail-absent'>
                    The workspace’s complete inventory does not contain this item, so it is not shown.
                </p>
            ) : state.detail.found === 'incomplete' ? (
                <p className='text-[13px]' data-slot='gateway-detail-incomplete'>
                    This item was not in the part of the workspace’s inventory that could be read. It is not shown, and this does not mean it is absent.
                </p>
            ) : (
                <>
                    {item?.credential_bearing === true ? (
                        <p className='rounded-[8px] text-[12px]' style={{ padding: '10px 12px', background: 'var(--v2-inset)', ...soft }} data-slot='gateway-credential-bearing'>
                            This configuration carries credentials. Their values are not shown.
                        </p>
                    ) : null}
                    <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='gateway-detail-fields'>
                        {Object.entries(item ?? {})
                            .filter(([key]) => key !== 'credential_bearing')
                            .map(([key, value]) => (
                                <div key={key} className='contents'>
                                    <dt className='font-mono text-[12px] break-all' style={soft}>
                                        {key}
                                    </dt>
                                    <dd className='font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                                </div>
                            ))}
                    </dl>
                    {resource === 'integrations' ? (
                        <div className='flex flex-col' style={{ gap: 8 }} data-slot='gateway-models'>
                            <Btn tier='tertiary' className='self-start' aria-disabled={models.loading} onClick={() => !models.loading && state.kind === 'shown' && void readModels(state.detail.id)}>
                                {models.read ? 'Refresh models' : 'Read models'}
                            </Btn>
                            <p role='status' aria-live='polite' className='text-[13px]' style={soft}>
                                {models.loading ? 'Reading models…' : models.failure ? `The models could not be read: ${models.failure}` : models.read ? `${models.read.count} model${models.read.count === 1 ? '' : 's'}${models.read.complete ? '' : ' read · more exist'}${models.read.allowAll ? ' · all models allowed' : ''}` : ''}
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
            {state.kind === 'shown' && state.detail.found === 'found' ? (
                <div className='flex flex-wrap' style={{ gap: 8 }} data-slot='gateway-item-actions'>
                    {itemActions(resource, actions).map(([action, label]) => (
                        <Btn
                            key={action}
                            aria-label={`${label.replace('…', '')} ${state.name}`}
                            onClick={() =>
                                onChange(
                                    action,
                                    state.id,
                                    state.name,
                                    action === 'rotate'
                                        ? { key_transition_period_ms: 3600000 }
                                        : action === 'models'
                                          ? { models: models.read ? (JSON.parse(models.read.models) as Record<string, unknown>[]).map((m) => ({ slug: m.slug, enabled: m.enabled })) : [] }
                                          : undefined
                                )
                            }
                        >
                            {label}
                        </Btn>
                    ))}
                </div>
            ) : null}
            <Btn tier='tertiary' className='self-start' onClick={onClose}>
                {C.table.closeDetails}
            </Btn>
        </section>
    )
}

/** The chosen workspace's own detail: settings, defaults and embedded limits. */
function WorkspacePanel({ name, held, onRefresh, onChange, actions }: { name: string; held: Held<GatewayDetail>; onRefresh: () => void; onChange: (action: ChangeAction) => void; actions: AdminAction[] }) {
    const d = held.read
    const item: Record<string, unknown> | null = d?.item ? (JSON.parse(d.item) as Record<string, unknown>) : null
    return (
        <div className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: 18, ...cardStyle, background: 'var(--v2-paper)' }} data-slot='gateway-workspace-detail'>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <p role='status' aria-live='polite' className='text-[13px]' style={soft}>
                    {held.loading ? 'Reading the workspace…' : item ? `Settings of ${name} as read from the service.` : 'Not read yet'}
                </p>
                <Btn tier='tertiary' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    Refresh workspace
                </Btn>
            </div>
            {held.failure ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }} data-slot='gateway-workspace-failure'>
                    The workspace could not be read: {held.failure}
                </p>
            ) : null}
            {d && d.found !== 'found' ? <p className='text-[13px]'>The service did not return this workspace. Refresh the workspaces.</p> : null}
            {item ? (
                <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='gateway-workspace-fields'>
                    {Object.entries(item).map(([key, value]) => (
                        <div key={key} className='contents'>
                            <dt className='font-mono text-[12px] break-all' style={soft}>
                                {key}
                            </dt>
                            <dd className='font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                        </div>
                    ))}
                </dl>
            ) : null}
            <div className='flex flex-wrap' style={{ gap: 8 }}>
                {admittedLabel(actions, 'gatewayWorkspaces', 'update') ? <Btn onClick={() => onChange('update')}>{admittedLabel(actions, 'gatewayWorkspaces', 'update')}</Btn> : null}
                {admittedLabel(actions, 'gatewayWorkspaces', 'delete') ? <Btn onClick={() => onChange('delete')}>{admittedLabel(actions, 'gatewayWorkspaces', 'delete')}</Btn> : null}
            </div>
        </div>
    )
}

/**
 * Gateway work kept on this Mac (C17, T23): unreviewed drafts and changes that
 * settled uncertain. Readable offline; none carries a review or Apply
 * authority, so reconnecting sends nothing. A Playground draft is listed and
 * discarded here; it is edited in the Playground.
 */
/**
 * The chosen workspace's inference key (C18) on the one presentation (C44): its Keychain state from the tenant's
 * snapshot, the endpoint and key fields, and Save or Replace (configureInferenceCredential, keychain) or Remove
 * (removeInferenceCredential). The key is read back only as a state; the field is emptied as soon as it is sent.
 */
function WorkspaceKey({ tenant, selected, context, dispatch }: { tenant: ShellSnapshot['tenants'][number]; selected: NonNullable<ShellSnapshot['gateway']['selected']>; context: ContextRef; dispatch: Dispatch }) {
    const K = C.workspaceKey
    const ids = useId()
    const keyRef = useRef<HTMLInputElement>(null)
    const [endpoint, setEndpoint] = useState('')
    const [errors, setErrors] = useState<{ endpoint: string; key: string }>({ endpoint: '', key: '' })
    const [busy, setBusy] = useState(false)
    const [message, setMessage] = useState<string | null>(null)
    const key = tenant.inference
    const here = key.target !== null && key.target.workspace.id === selected.workspace.id
    const ready = key.state === 'ready' && here
    async function save(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const field = keyRef.current
        if (!field) return
        const secret = field.value
        field.value = ''
        setMessage(null)
        if (!/^https:\/\/\S+$/.test(endpoint.trim())) return setErrors({ endpoint: K.errors.endpoint, key: '' })
        if (!secret) return setErrors({ endpoint: '', key: K.errors.key })
        setErrors({ endpoint: '', key: '' })
        setBusy(true)
        try {
            const outcome = await dispatch({ kind: 'configureInferenceCredential', context, mode: 'keychain', workspace: selected.reference, endpoint: endpoint.trim(), secret })
            setMessage(outcome.kind === 'admitted' ? K.saved : (outcomeMessage(outcome) ?? 'That was not done.'))
            if (outcome.kind === 'admitted') setEndpoint('')
        } finally {
            setBusy(false)
        }
    }
    async function remove() {
        setMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch({ kind: 'removeInferenceCredential', context })
            setMessage(outcome.kind === 'admitted' ? K.removed : (outcomeMessage(outcome) ?? 'That was not done.'))
        } finally {
            setBusy(false)
        }
    }
    return (
        <form onSubmit={(e) => void save(e)} aria-labelledby={`${ids}-title`} className='flex flex-col rounded-[10px] border p-3' style={{ gap: 10, ...cardStyle }} data-slot='gateway-workspace-key'>
            <div className='flex flex-wrap items-baseline' style={{ gap: 10 }}>
                <h4 id={`${ids}-title`} className='text-[13px] font-bold'>
                    {K.title}
                </h4>
                <span className='text-[12px]' style={ready ? { color: 'var(--v2-ok)' } : faint} data-slot='gateway-workspace-key-state'>
                    {ready ? `${K.state.ready}${key.target?.endpoint ? ` · ${key.target.endpoint}` : ''}` : K.state[key.state] ?? K.state.missing}
                </span>
            </div>
            <p className='text-[12px]' style={faint}>
                {K.hint}
            </p>
            {key.state === 'ready' && key.target && !here ? (
                <p className='text-[12px]' style={faint}>
                    {K.otherWorkspace(key.target.workspace.name)}
                </p>
            ) : null}
            <label className='flex flex-col text-[12px]' style={{ gap: 4 }}>
                <span>{K.endpoint}</span>
                <Field aria-label={K.endpoint} value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder='https://' autoComplete='off' spellCheck={false} aria-invalid={errors.endpoint ? true : undefined} aria-describedby={`${ids}-endpoint-hint`} />
                <span id={`${ids}-endpoint-hint`} style={faint}>
                    {errors.endpoint || K.endpointHint}
                </span>
            </label>
            <label className='flex flex-col text-[12px]' style={{ gap: 4 }}>
                <span>{K.key}</span>
                <input
                    ref={keyRef}
                    type='password'
                    aria-label={K.key}
                    autoComplete='off'
                    aria-invalid={errors.key ? true : undefined}
                    className='h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px]'
                    style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}
                />
                {errors.key ? (
                    <span role='alert' style={{ color: 'var(--v2-flag)' }}>
                        {errors.key}
                    </span>
                ) : null}
            </label>
            <div className='flex flex-wrap' style={{ gap: 8 }}>
                <Btn tier='primary' type='submit' disabled={busy}>
                    {ready ? K.replace : K.save}
                </Btn>
                {key.state === 'ready' ? (
                    <Btn tier='tertiary' type='button' disabled={busy} onClick={() => void remove()}>
                        {K.remove}
                    </Btn>
                ) : null}
            </div>
            {message ? (
                <p role='status' className='text-[12px]'>
                    {message}
                </p>
            ) : null}
        </form>
    )
}

function LocalWork({ local, context, dispatch, onOpen }: { local: GatewayLocalView; context: ContextRef; dispatch: Dispatch; onOpen: (kept: GatewayDraft | GatewayRecord, draftId: string | undefined) => void }) {
    const ids = useId()
    const [message, setMessage] = useState<string | null>(null)
    if (local.state !== 'failed' && local.drafts.length === 0 && local.records.length === 0 && local.runs.length === 0 && local.files.length === 0 && local.playground.length === 0 && !local.unsaved) return null
    async function discard(id: string) {
        const outcome = await dispatch({ kind: 'discardGatewayLocal', context, id })
        setMessage(outcome.kind === 'admitted' ? null : outcomeMessage(outcome))
    }
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col' style={{ gap: 10 }} data-slot='gateway-local'>
            <h3 id={`${ids}-heading`} className='text-[13px] font-semibold'>
                Kept on this Mac
            </h3>
            <p className='text-[12px]' style={soft}>
                Drafts and uncertain results are kept here for this tenant, even offline. None of them is a review: nothing is sent until you review and apply it again.
            </p>
            {local.state === 'failed' ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }} data-slot='gateway-local-failed'>
                    {local.failure === 'unreadable' ? 'The Gateway work kept on this Mac could not be read. It was left as it is.' : 'The Gateway work kept on this Mac is unavailable.'}
                </p>
            ) : null}
            {local.unsaved ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }} data-slot='gateway-local-unsaved'>
                    The last change could not be saved on this Mac.
                </p>
            ) : null}
            {local.records.length > 0 ? (
                <ul aria-label='Uncertain changes' className='flex flex-col' style={{ gap: 10 }}>
                    {local.records.map((r) => (
                        <li key={r.id} className='flex flex-col rounded-[12px] border' style={{ gap: 8, padding: 14, ...cardStyle }} data-slot='gateway-local-record'>
                            <p className='text-[13px] font-medium'>{keptLabel(r)}</p>
                            <p className='text-[13px]'>{recordText(r)}</p>
                            {r.provisioning ? <ProvisioningSteps p={r.provisioning} /> : null}
                            <div className='flex flex-wrap' style={{ gap: 8 }}>
                                <Btn aria-label={`Open to retry: ${keptLabel(r)}`} onClick={() => onOpen(r, undefined)}>
                                    Open to retry…
                                </Btn>
                                <Btn tier='tertiary' aria-label={`Dismiss record: ${keptLabel(r)}`} onClick={() => void discard(r.id)}>
                                    Dismiss
                                </Btn>
                            </div>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.runs.length > 0 ? (
                <ul aria-label='Playground runs without a known result' className='flex flex-col' style={{ gap: 8 }}>
                    {local.runs.map((r) => (
                        <li key={r.id} className='flex flex-col text-[13px]' style={{ gap: 4 }} data-slot='gateway-local-run'>
                            <p>{`Playground run in ${r.workspace.name} (${r.model}) ${r.end === 'stopped' ? 'was stopped' : 'failed'} after it was sent${r.answered ? ' and answered' : ''}: the Gateway may have run it and charged for it.${r.failure ? ` ${INFERENCE_FAILURE_TEXT[r.failure] ?? r.failure}` : ''} Trace ${r.trace} · ${keptAt(r.settledAt)}. Its output is not kept.`}</p>
                            <Btn tier='tertiary' className='self-start' aria-label={`Dismiss run: ${r.trace}`} onClick={() => void discard(r.id)}>
                                Dismiss
                            </Btn>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.files.length > 0 ? (
                <ul aria-label='Gateway file changes without a known result' className='flex flex-col' style={{ gap: 8 }}>
                    {local.files.map((r) => (
                        <li key={r.id} className='flex flex-col text-[13px]' style={{ gap: 4 }} data-slot='gateway-local-file'>
                            <p>{`${r.action === 'upload' ? 'Upload' : 'Delete'} of ${r.filename ?? r.file ?? 'a file'}${r.bytes !== null ? ` (${sizeText(r.bytes)})` : ''} in ${r.workspace.name} was sent without a known result: the file ${r.action === 'upload' ? 'may have been stored' : 'may have been deleted'}.${r.failure ? ` ${fileFailureText(r.failure)}` : ''} Trace ${r.trace} · ${keptAt(r.settledAt)}. It is never sent again; read the workspace's files to see what exists.`}</p>
                            <Btn tier='tertiary' className='self-start' aria-label={`Dismiss file record: ${r.trace}`} onClick={() => void discard(r.id)}>
                                Dismiss
                            </Btn>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.playground.length > 0 ? (
                <ul aria-label='Playground drafts' className='flex flex-col' style={{ gap: 8 }}>
                    {local.playground.map((d) => (
                        <li key={d.id} className='flex flex-col text-[13px]' style={{ gap: 4 }} data-slot='gateway-local-playground'>
                            <p>{`Playground draft for ${d.workspace.name} · ${d.model || 'no model yet'} · saved ${keptAt(d.savedAt)}. It opens in the Playground when ${d.workspace.name} is chosen; nothing is sent until you run it there.`}</p>
                            <p className='line-clamp-2 whitespace-pre-wrap' style={soft}>
                                {d.prompt}
                            </p>
                            <Btn tier='tertiary' className='self-start' aria-label={`Discard Playground draft for ${d.workspace.name}`} onClick={() => void discard(d.id)}>
                                Discard
                            </Btn>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.drafts.length > 0 ? (
                <ul aria-label='Drafts' className='flex flex-col' style={{ gap: 8 }}>
                    {local.drafts.map((d) => (
                        <li key={d.id} className='flex flex-wrap items-center text-[13px]' style={{ gap: 10 }} data-slot='gateway-local-draft'>
                            <span>
                                Draft: {keptLabel(d)} · saved {keptAt(d.savedAt)}
                            </span>
                            <Btn aria-label={`Open draft: ${keptLabel(d)}`} onClick={() => onOpen(d, d.id)}>
                                Open…
                            </Btn>
                            <Btn tier='tertiary' aria-label={`Discard draft: ${keptLabel(d)}`} onClick={() => void discard(d.id)}>
                                Discard
                            </Btn>
                        </li>
                    ))}
                </ul>
            ) : null}
            {message ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {message}
                </p>
            ) : null}
        </section>
    )
}

// ---- Request logs (C18) ----

interface LogQuery {
    days: number
    trace: string | null
    status: number | null
}
const LOG_PAGE = 25
const LOG_PAGES = 40
const WINDOWS = [1, 7, 30] as const
const WINDOW_TEXT: Record<number, string> = { 1: 'Last day', 7: 'Last 7 days', 30: 'Last 30 days' }
const sameQuery = (page: GatewayLogs, q: LogQuery) => page.days === q.days && page.trace === q.trace && page.status === q.status
const queryText = (q: LogQuery) => `${(WINDOW_TEXT[q.days] ?? `${q.days} days`).toLowerCase()}${q.trace !== null ? `, trace ${q.trace}` : ''}${q.status !== null ? `, status ${q.status}` : ''}`
const number = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 1 })
const dollars = (cents: number) => (cents / 100).toLocaleString(undefined, { style: 'currency', currency: 'USD' })

function LogsPanel({ workspace, read }: { workspace: string; read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string> }) {
    const ids = useId()
    const [days, setDays] = useState<number>(7)
    const [trace, setTrace] = useState('')
    const [status, setStatus] = useState('')
    const [pages, setPages] = useState<GatewayLogs[]>([])
    const [held, setHeld] = useState<{ loading: boolean; failure: string | null }>({ loading: false, failure: null })
    const [invalid, setInvalid] = useState<string | null>(null)
    const token = useRef(0)
    function query(): LogQuery | null {
        const t = trace.trim()
        const s = status.trim()
        if (t && !/^[A-Za-z0-9_.:-]{1,128}$/.test(t)) {
            setInvalid('A trace ID is 1-128 letters, digits, “_”, “.”, “:” or “-”.')
            return null
        }
        if (s && !(/^\d{3}$/.test(s) && Number(s) >= 100 && Number(s) <= 599)) {
            setInvalid('A status is a three-digit HTTP status from 100 to 599.')
            return null
        }
        setInvalid(null)
        return { days, trace: t || null, status: s ? Number(s) : null }
    }
    async function load(page: number, q: LogQuery) {
        const mine = ++token.current
        setHeld({ loading: true, failure: null })
        const what: ObserveRead = { kind: 'logs', days: q.days, page }
        if (q.trace !== null) what.trace = q.trace
        if (q.status !== null) what.status = q.status
        const result = await read({ kind: 'observe', read: what }, true)
        if (mine !== token.current) return
        const o = typeof result !== 'string' && result.kind === 'gateway' && result.gateway.kind === 'observed' ? result.gateway.observed : null
        if (o?.kind === 'logs' && o.logs.page === page && sameQuery(o.logs, q)) {
            setPages((p) => (page === 0 ? [o.logs] : p.length === page && p.every((x) => sameQuery(x, q)) ? [...p, o.logs] : p))
            setHeld({ loading: false, failure: null })
        } else setHeld({ loading: false, failure: o ? 'The service answered for other filters.' : resultFailure(result) })
    }
    useEffect(() => {
        const q = query()
        if (q) void load(0, q)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once on open
    }, [])
    const rows = pages.flatMap((p) => p.rows)
    const last = pages.at(-1)
    const logGrid = 'minmax(0, 1.2fr) minmax(0, 1.4fr) minmax(0, 0.8fr) minmax(0, 0.6fr) minmax(0, 0.6fr) minmax(0, 1fr)'
    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col' style={{ gap: 12 }} data-slot='gateway-logs'>
            <h3 id={`${ids}-h`} className='text-[13px] font-semibold'>
                Request logs of {workspace}
            </h3>
            <form
                className='flex flex-wrap items-end'
                style={{ gap: 8 }}
                onSubmit={(e) => {
                    e.preventDefault()
                    const q = query()
                    if (q) void load(0, q)
                }}
            >
                <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                    Window
                    <select value={days} onChange={(e) => setDays(Number(e.target.value))} className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px]' style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                        {WINDOWS.map((w) => (
                            <option key={w} value={w}>
                                {WINDOW_TEXT[w]}
                            </option>
                        ))}
                    </select>
                </label>
                <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                    Trace ID
                    <Field value={trace} onChange={(e) => setTrace(e.target.value)} className='font-mono' spellCheck={false} />
                </label>
                <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                    Status
                    <Field value={status} onChange={(e) => setStatus(e.target.value)} inputMode='numeric' style={{ width: 80 }} />
                </label>
                <Btn type='submit'>Read logs</Btn>
            </form>
            {invalid ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {invalid}
                </p>
            ) : null}
            <p role='status' aria-live='polite' className='text-[12px]' style={soft} data-slot='gateway-logs-status'>
                {held.loading
                    ? 'Reading logs…'
                    : last
                      ? `${rows.length.toLocaleString()} request${rows.length === 1 ? '' : 's'} shown${last.total !== null ? ` of ${last.total.toLocaleString()}` : ''} for ${queryText(last)}${last.partial ? ' · part of the window only (the service’s quota was exceeded)' : ''}${last.capped ? ` · only the first ${(LOG_PAGES * LOG_PAGE).toLocaleString()} are read here: more exist${last.total === null ? ' or may exist' : ''}` : last.more || rows.length === 0 || last.partial ? '' : ' · all read'}`
                      : ''}
            </p>
            {held.failure ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    The logs could not be read: {held.failure}
                </p>
            ) : null}
            {last && rows.length === 0 && !held.loading ? <p className='text-[13px]'>{last.partial ? 'No request was returned for the part of the window read.' : 'No request matches.'}</p> : null}
            {rows.length > 0 ? (
                <div className='overflow-hidden rounded-[12px] border' style={cardStyle}>
                    <div role='table' aria-label='Requests, newest first' data-slot='gateway-log-rows'>
                        <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: logGrid, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                            {['Time', 'Model', 'Status', 'Units', 'Cost', 'Trace'].map((h) => (
                                <span key={h} role='columnheader'>
                                    {h}
                                </span>
                            ))}
                        </div>
                        {rows.map((r) => (
                            <div key={r.id} role='row' className='grid text-[12px]' style={{ gridTemplateColumns: logGrid, gap: 12, padding: '10px 18px', borderTop: '1px solid var(--v2-rule)' }}>
                                <span role='cell' className='min-w-0 truncate' data-label='Time'>{r.createdAt}</span>
                                <span role='cell' className='min-w-0 truncate font-mono' data-label='Model'>{r.provider ? `${r.provider}/${r.model}` : r.model}</span>
                                <span role='cell' data-label='Status'>{r.status === 446 ? '446 (blocked by a guardrail)' : r.status}</span>
                                <span role='cell' data-label='Units'>{r.units === null ? '—' : number(r.units)}</span>
                                <span role='cell' data-label='Cost'>{r.cost === null ? '—' : dollars(r.cost)}</span>
                                <span role='cell' className='min-w-0 truncate font-mono' data-label='Trace'>{r.traceId ?? '—'}</span>
                            </div>
                        ))}
                    </div>
                </div>
            ) : null}
            {last?.more ? (
                <Btn tier='tertiary' className='self-start' aria-disabled={held.loading} onClick={() => !held.loading && void load(last.page + 1, { days: last.days, trace: last.trace, status: last.status })}>
                    Read more logs
                </Btn>
            ) : null}
        </section>
    )
}

/** The chosen workspace's MCP catalog (`GatewayRead::McpServers`, C22), as the Harness reads it. */
function McpServers({ snapshot, dispatch, context }: { snapshot: ShellSnapshot; dispatch: Dispatch; context: ContextRef }) {
    const mcp = snapshot.harness.mcp
    const workspace = snapshot.gateway.selected?.reference ?? null
    if (!workspace) return <p className='text-[13px]'>{C.noWorkspace}</p>
    const here = mcp !== null && mcp.workspace === snapshot.gateway.selected?.workspace.id
    const mcpGrid = 'minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 1fr)'
    return (
        <div className='flex flex-col' style={{ gap: 12 }} data-slot='admin-mcp'>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <p role='status' className='text-[12px]' style={soft}>
                    {!here ? C.mcpNotRead : mcp.state === 'loading' ? 'Reading the workspace’s MCP servers…' : mcp.failure ? `The MCP servers could not be read: ${MANAGEMENT_FAILURE_TEXT[mcp.failure]}` : `${mcp.servers.length} server${mcp.servers.length === 1 ? '' : 's'}${mcp.complete ? '' : ' · only the first 500 were read'}`}
                </p>
                <Btn tier='tertiary' onClick={() => void dispatch({ kind: 'listMcpServers', context, workspace })}>
                    {C.readMcp}
                </Btn>
            </div>
            {here && mcp.servers.length > 0 ? (
                <div className='overflow-hidden rounded-[12px] border' style={cardStyle}>
                    <div role='table' aria-label='MCP servers'>
                        <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: mcpGrid, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                            <span role='columnheader'>Server</span>
                            <span role='columnheader'>Namespace</span>
                            <span role='columnheader'>Status</span>
                        </div>
                        {mcp.servers.map((s) => (
                            <div key={s.id} role='row' className='grid text-[12px]' style={{ gridTemplateColumns: mcpGrid, gap: 12, padding: '10px 18px', borderTop: '1px solid var(--v2-rule)' }}>
                                <span role='cell' className='min-w-0 truncate font-semibold' data-label='Server'>{s.name}</span>
                                <span role='cell' className='min-w-0 truncate font-mono' data-label='Namespace'>{s.namespace}</span>
                                <span role='cell' className='min-w-0 truncate' data-label='Status'>{s.status}</span>
                            </div>
                        ))}
                    </div>
                </div>
            ) : null}
        </div>
    )
}
