import { forwardRef, type ReactNode, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, RefreshCw } from 'lucide-react'
import type {
    AdminAction,
    ChangeAction,
    ChangeKind,
    ContextRef,
    FeatureId,
    GatewayDetail,
    GatewayDraft,
    GatewayDraftInput,
    GatewayDraftProblem,
    GatewayList,
    GatewayLocalView,
    GatewayModels,
    GatewayPrerequisites,
    GatewayRead,
    GatewayRecord,
    GatewayResource,
    ListedWorkspace,
    ManagementResult,
    PlaneAccess,
    ScopeBinding,
    ShellSnapshot,
    WorkspaceRef,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { fieldText, MANAGEMENT_FAILURE_TEXT, type ObserveManagement, readManagement } from '../administration/management-read'
import {
    CHANGE_NOUN,
    type ChangeTarget,
    ChangeResults,
    ChangeWorkbench,
    isGatewayKind,
    pretty,
    ProvisioningSteps,
    verbOf,
} from '../administration/runtime-changes'
import { LimitsPanel, LogsPanel, UsagePanel } from './gateway-observe'
import { fileFailureText, FilesPanel, sizeText } from './gateway-files'
import { INFERENCE_FAILURE_TEXT, inferenceKey, KeptDraftEditor, PlaygroundPanel } from './playground'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
    /** v2 (C31): the tab the shell's Administration accordion selected; the strip and heading are then the shell's. */
    tab?: GatewayTab
    embedded?: boolean
    /** v2 (C31): the Administration tab's admitted actions and labels (AdminInventory); legacy actions when absent. */
    actions?: AdminAction[]
    /** v2 (C31): every change opens the shell's review sheet (typed confirmation, secret disposal on exit). */
    onReview?: (change: ChangeTarget, extras: GatewayReviewExtras) => void
}

/** What a Gateway change carries into the shell's review sheet: the local draft hook and its note. */
export interface GatewayReviewExtras {
    onDraft?: (body: string) => void
    note?: ReactNode
}

export type GatewayTab = Exclude<GatewayResource, 'workspaces'> | 'workspace' | 'prerequisites' | 'playground' | 'files' | 'usage' | 'logs' | 'limits'
type Tab = GatewayTab

const TABS: readonly [Tab, string][] = [
    ['workspace', 'Workspace'],
    ['providers', 'Providers'],
    ['routes', 'Routes'],
    ['guardrails', 'Guardrails'],
    ['serviceKeys', 'Service keys'],
    ['userKeys', 'User keys'],
    ['integrations', 'Integrations'],
    ['prerequisites', 'Inference prerequisites'],
    ['playground', 'Playground'],
    ['files', 'Files'],
    ['usage', 'Usage'],
    ['logs', 'Logs'],
    ['limits', 'Limits'],
]

/** Tabs whose panels read for themselves (C18). */
const OBSERVED: readonly Tab[] = ['usage', 'logs', 'limits']

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

/** The change kind of each Gateway inventory (C17). */
const KIND: Record<GatewayResource, ChangeKind> = {
    workspaces: 'gatewayWorkspaces',
    providers: 'gatewayProviders',
    routes: 'gatewayRoutes',
    guardrails: 'gatewayGuardrails',
    serviceKeys: 'gatewayServiceKeys',
    userKeys: 'gatewayUserKeys',
    integrations: 'gatewayIntegrations',
}

const RESOURCE_OF = Object.fromEntries(Object.entries(KIND).map(([r, k]) => [k, r])) as Record<ChangeKind, GatewayResource>

const NOUN: Record<GatewayResource, string> = {
    workspaces: 'workspace',
    providers: 'provider',
    routes: 'route',
    guardrails: 'guardrail',
    serviceKeys: 'service key',
    userKeys: 'user key',
    integrations: 'integration',
}

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

/** A change being edited; one opened from this Mac keeps its workspace. */
type Opened = ChangeTarget & { home?: ListedWorkspace | null }

/** Kinds whose changes apply inside the chosen workspace. */
const inWorkspace = (kind: ChangeKind) => kind !== 'gatewayWorkspaces' && kind !== 'gatewayIntegrations'

/** What the operator chose to change in the Gateway. */
function gatewayChange(
    resource: GatewayResource,
    action: ChangeAction,
    target: string,
    name: string,
    workspace: WorkspaceRef | null,
    body?: Record<string, unknown>
): ChangeTarget {
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
    const change: ChangeTarget = {
        kind: KIND[resource],
        action,
        target,
        title,
        body: action === 'delete' ? '{}' : pretty(body ?? (action === 'create' ? CREATE_TEMPLATE[resource] : { name })),
    }
    if (scoped && workspace) change.workspace = workspace
    return change
}

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
export const GatewayView = forwardRef<HTMLHeadingElement, Props>(function GatewayView({ snapshot, dispatch, observe, onNavigate, tab: controlledTab, embedded = false, actions, onReview }, ref) {
    const ids = useId()
    const context = snapshot.active?.context ?? null
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const gateway = snapshot.gateway
    const selected = gateway.selected
    const reference = selected?.reference ?? null
    const [listing, setListing] = useState<{ loading: boolean; failure: string | null }>({ loading: false, failure: null })
    const [choosing, setChoosing] = useState<string | null>(null)
    const [tab, setTab] = useState<Tab>(controlledTab ?? 'providers')
    useEffect(() => {
        if (controlledTab) setTab(controlledTab)
    }, [controlledTab])
    const [lists, setLists] = useState<Partial<Record<GatewayResource, Held<GatewayList>>>>({})
    const [prerequisites, setPrerequisites] = useState<Held<GatewayPrerequisites>>(empty)
    const [space, setSpace] = useState<Held<GatewayDetail>>(empty)
    const [change, setChange] = useState<Opened | null>(null)
    /** The trace the Logs tab opens with ("Find in logs"). */
    const [logsTrace, setLogsTrace] = useState<string | undefined>(undefined)
    const local = gateway.local
    const draftNote = (
        <p className='text-academy-ink-faint text-[12px]' data-slot='gateway-draft-note'>
            {(local.problem && DRAFT_PROBLEM_TEXT[local.problem]) ??
                'Your edits are kept on this Mac as a draft (never a review). Nothing is sent until you review and apply it.'}
        </p>
    )
    const opener = useRef<HTMLElement | null>(null)
    const settledSeen = useRef<Set<string> | null>(null)
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

    /** The chosen workspace's own detail: defaults and embedded limits. */
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

    function startChange(next: Opened, from: HTMLElement | null) {
        opener.current = from
        if (onReview) {
            // v2: the shell's review sheet, with this view's draft behaviour.
            onReview(next, {
                onDraft: next.action === 'delete' ? undefined : (body) => keepDraft(next, body),
                note: next.action === 'delete' ? null : draftNote,
            })
            return
        }
        setChange(next)
    }

    /** Keeps the edited text on this Mac (never reviewed or sent). */
    function keepDraft(target: ChangeTarget, body: string) {
        if (!context) return
        const draft: GatewayDraftInput = { kind: target.kind, action: target.action, target: target.target, body }
        if (target.draftId) draft.id = target.draftId
        else if (target.workspace) draft.workspace = target.workspace
        void dispatch({ kind: 'saveGatewayDraft', context, draft })
    }

    /** Opens kept text: a draft keeps editing itself; a record's text starts a new change. */
    function openKept(kept: GatewayDraft | GatewayRecord, draftId: string | undefined, from: HTMLElement) {
        const resource = RESOURCE_OF[kept.kind]
        const name = 'name' in kept && kept.name ? kept.name : kept.target
        const base = gatewayChange(resource, kept.action, kept.target, name, null)
        startChange({ ...base, body: kept.body, draftId, home: kept.workspace }, from)
    }

    function closeChange() {
        setChange(null)
        opener.current?.focus()
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

    // Gateway work kept on this Mac is read when the page opens (offline too).
    useEffect(() => {
        if (context && local.state === 'idle') void dispatch({ kind: 'openGatewayLocal', context })
    }, [context, local.state === 'idle'])

    // The workspaces are listed when the page opens with none held.
    useEffect(() => {
        if (ready && gateway.workspaces === null && !listing.loading && !listing.failure) void listWorkspaces()
    }, [ready, gateway.workspaces === null])

    // A different workspace starts with nothing held; a change prepared in
    // the one before is ended natively and closed here.
    useEffect(() => {
        generation.current += 1
        setLists({})
        setPrerequisites(empty)
        setSpace(empty)
        setChange((c) => (c?.workspace ? null : c))
    }, [reference])

    // A settled change refreshes what it changed.
    const gatewayResults = snapshot.changes.results.filter((r) => isGatewayKind(r.kind))
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
    }, [gatewayResults.map((r) => `${r.operation}:${r.settled}`).join(',')])

    // Each tab is read when first shown with nothing held.
    useEffect(() => {
        if (!ready || !reference) return
        if (tab === 'prerequisites') {
            if (!prerequisites.read && !prerequisites.loading && !prerequisites.failure) void readPrerequisites()
            return
        }
        if (tab === 'workspace') {
            if (!space.read && !space.loading && !space.failure) void readWorkspace()
            return
        }
        if (OBSERVED.includes(tab)) return
        // The Playground offers the workspace's routes.
        const resource = (tab === 'playground' ? 'routes' : tab) as GatewayResource
        const held = lists[resource]
        if (!held) void readList(resource)
    }, [tab, ready, reference, lists, prerequisites, space])

    const heading = embedded ? null : <PageHeading ref={ref} id='gateway-heading' kicker='AI Gateway' title='Gateway workspaces and configuration' />

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
    // A change opened from this Mac is reviewed only in its own workspace,
    // with the reference chosen now.
    const opened: ChangeTarget | null =
        change && change.home !== undefined && inWorkspace(change.kind)
            ? change.home && selected?.workspace.id === change.home.id && reference
                ? { ...change, workspace: reference }
                : {
                      ...change,
                      workspace: undefined,
                      reviewBlocked: `This draft is for workspace ${change.home?.name ?? 'unknown'}. Choose that workspace above to review it.`,
                  }
            : change
    return (
        <section aria-labelledby='gateway-heading' className='flex flex-col gap-5' data-slot='gateway'>
            {heading}
            <p className='text-academy-ink-soft max-w-3xl text-[14px]'>
                {tenant.label}’s AI Gateway through its management client. Key values, provider keys and credential-bearing
                settings stay in the desktop’s native core and are never shown or entered here. Every change is reviewed
                against what the service holds and applied only after you confirm it in the desktop’s dialog. Inference is
                sent only from the Playground, with the chosen workspace’s own inference key.
            </p>
            <LocalWork local={local} context={context} dispatch={dispatch} onOpen={openKept} />
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
                            {/* v2: creating a workspace belongs to the Workspaces tab only. */}
                            {!controlledTab || controlledTab === 'workspace' ? (
                                <Button
                                    type='button'
                                    tier='secondary'
                                    size='sm'
                                    onClick={(e) => startChange(gatewayChange('workspaces', 'create', '', '', null), e.currentTarget)}
                                >
                                    {admittedLabel(actions, 'gatewayWorkspaces', 'create') ?? 'Provision workspace…'}
                                </Button>
                            ) : null}
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
                            <div role='tablist' aria-label={`Configuration of ${selected.workspace.name}`} className='flex flex-wrap gap-1' hidden={!!controlledTab}>
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
                                {tab === 'playground' && context && reference ? (
                                    <PlaygroundPanel
                                        key={reference}
                                        snapshot={snapshot}
                                        dispatch={dispatch}
                                        context={context}
                                        reference={reference as WorkspaceRef}
                                        workspace={selected.workspace}
                                        routes={lists.routes?.read ?? null}
                                        onLogs={(trace) => {
                                            setLogsTrace(trace)
                                            setTab('logs')
                                        }}
                                    />
                                ) : tab === 'files' && context && reference ? (
                                    <FilesPanel
                                        key={reference}
                                        snapshot={snapshot}
                                        dispatch={dispatch}
                                        context={context}
                                        reference={reference as WorkspaceRef}
                                        workspace={selected.workspace}
                                        onLogs={(trace) => {
                                            setLogsTrace(trace)
                                            setTab('logs')
                                        }}
                                    />
                                ) : tab === 'usage' ? (
                                    <UsagePanel key={reference} workspace={selected.workspace.name} read={read} />
                                ) : tab === 'logs' ? (
                                    <LogsPanel key={`${reference}-${logsTrace ?? ''}`} workspace={selected.workspace.name} read={read} trace={logsTrace} />
                                ) : tab === 'limits' ? (
                                    <LimitsPanel key={reference} workspace={selected.workspace.name} read={read} />
                                ) : tab === 'prerequisites' ? (
                                    <PrerequisitesPanel
                                        held={prerequisites}
                                        onRefresh={() => void readPrerequisites()}
                                        snapshot={snapshot}
                                        workspace={selected.workspace}
                                        onPlayground={() => setTab('playground')}
                                    />
                                ) : tab === 'workspace' ? (
                                    <WorkspacePanel
                                        name={selected.workspace.name}
                                        held={space}
                                        onRefresh={() => void readWorkspace()}
                                        actions={actions}
                                        onChange={(action, from) =>
                                            startChange(gatewayChange('workspaces', action, selected.workspace.id, selected.workspace.name, null), from)
                                        }
                                    />
                                ) : (
                                    <InventoryPanel
                                        key={`${reference}-${tab}`}
                                        resource={tab as Exclude<GatewayResource, 'workspaces'>}
                                        held={lists[tab as GatewayResource] ?? empty<GatewayList>()}
                                        onRefresh={() => void readList(tab as GatewayResource)}
                                        read={read}
                                        actions={actions}
                                        onChange={(action, target, name, from, body) =>
                                            startChange(gatewayChange(tab as GatewayResource, action, target, name, reference, body), from)
                                        }
                                    />
                                )}
                            </div>
                        </>
                    ) : workspaces && workspaces.length > 0 ? (
                        <p className='text-[13px]'>Choose a workspace to read its configuration.</p>
                    ) : null}
                    {opened ? (
                        <ChangeWorkbench
                            key={`${opened.kind}-${opened.action}-${opened.target}-${opened.draftId ?? ''}`}
                            change={opened}
                            review={snapshot.changes.review}
                            context={context}
                            dispatch={dispatch}
                            onClose={closeChange}
                            onDraft={opened.action === 'delete' ? undefined : (body) => keepDraft(opened, body)}
                            note={opened.action === 'delete' ? null : draftNote}
                        />
                    ) : null}
                    <ChangeResults heading='Gateway changes' results={gatewayResults} dispatch={dispatch} />
                    <p className='text-academy-ink-faint text-[12px]' data-slot='gateway-not-here'>
                        Identity administration (users, roles and IAM scopes beyond the workspace’s binding) is not
                        available in the desktop.
                    </p>
                </>
            )}
        </section>
    )
})

/** Why the editor's text is not what is kept on this Mac. */
const DRAFT_PROBLEM_TEXT: Partial<Record<GatewayDraftProblem, string>> = {
    credential:
        'This text names a credential field, so it is not kept on this Mac. Credentials are never entered here.',
    incomplete:
        'Only complete JSON is kept on this Mac, so these edits are not kept yet: the last complete text is. Finish the document to keep it.',
    tooLarge: 'This text is too large to keep on this Mac; the last text that fit is kept.',
}

const KEPT_TIME = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
const keptAt = (iso: string) => {
    const date = new Date(iso)
    return Number.isNaN(date.getTime()) ? iso : KEPT_TIME.format(date)
}

/** What an uncertain change's record says: what was known when it settled. */
function recordText(r: GatewayRecord): string {
    const when = `when it settled (${keptAt(r.settledAt)})`
    const known =
        r.outcome === 'unknown'
            ? `Its outcome was not known ${when}.`
            : r.provisioning
              ? `Some steps completed and others did not ${when}.`
              : `It was applied, but reading it back did not confirm the reviewed change ${when}.`
    return `${known} The service has not been read since: refresh before trying again. Nothing here is sent unless you review it again.`
}

/** A kept change's one-line label. */
function keptLabel(k: GatewayDraft | GatewayRecord): string {
    const name = 'name' in k && k.name ? k.name : k.action === 'create' ? '' : k.target
    return `${verbOf(k.kind, k.action)} ${CHANGE_NOUN[k.kind]}${name ? ` ${name}` : ''}${k.workspace ? ` in ${k.workspace.name}` : ''}`
}

/**
 * Gateway work kept on this Mac (C17, T23): unreviewed drafts and changes
 * that settled uncertain. Readable and editable offline; none carries a
 * review or Apply authority, so reconnecting sends nothing.
 */
function LocalWork({
    local,
    context,
    dispatch,
    onOpen,
}: {
    local: GatewayLocalView
    context: ContextRef
    dispatch: Dispatch
    onOpen: (kept: GatewayDraft | GatewayRecord, draftId: string | undefined, from: HTMLElement) => void
}) {
    const ids = useId()
    const [message, setMessage] = useState<string | null>(null)
    const [editing, setEditing] = useState<string | null>(null)
    const items = useRef(new Map<string, HTMLLIElement>())
    if (
        local.state !== 'failed' &&
        local.drafts.length === 0 &&
        local.records.length === 0 &&
        local.runs.length === 0 &&
        local.files.length === 0 &&
        local.playground.length === 0 &&
        !local.unsaved
    )
        return null

    async function discard(id: string) {
        const outcome = await dispatch({ kind: 'discardGatewayLocal', context, id })
        setMessage(outcome.kind === 'admitted' ? null : outcomeMessage(outcome))
    }

    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='gateway-local'>
            <h2 id={`${ids}-heading`} className='text-academy-ink text-[15px] font-semibold'>
                Kept on this Mac
            </h2>
            <p className='text-academy-ink-soft text-[13px]'>
                Drafts and uncertain results are kept here for this tenant, even offline. None of them is a review: nothing
                is sent until you review and apply it again.
            </p>
            {local.state === 'failed' ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='gateway-local-failed'>
                    {local.failure === 'unreadable'
                        ? 'The Gateway work kept on this Mac could not be read. It was left as it is.'
                        : 'The Gateway work kept on this Mac is unavailable.'}
                </Callout>
            ) : null}
            {local.unsaved ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]' data-slot='gateway-local-unsaved'>
                    The last change could not be saved on this Mac.
                </p>
            ) : null}
            {local.records.length > 0 ? (
                <ul aria-label='Uncertain changes' className='flex flex-col gap-3'>
                    {local.records.map((r) => (
                        <li key={r.id} className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-4' data-slot='gateway-local-record'>
                            <p className='text-academy-ink text-[13px] font-medium'>{keptLabel(r)}</p>
                            <p className='text-[13px]'>{recordText(r)}</p>
                            {r.provisioning ? <ProvisioningSteps p={r.provisioning} /> : null}
                            <div className='flex flex-wrap gap-2'>
                                <Button type='button' tier='secondary' size='sm' aria-label={`Open to retry: ${keptLabel(r)}`} onClick={(e) => onOpen(r, undefined, e.currentTarget)}>
                                    Open to retry…
                                </Button>
                                <Button type='button' tier='tertiary' size='sm' aria-label={`Dismiss record: ${keptLabel(r)}`} onClick={() => void discard(r.id)}>
                                    Dismiss
                                </Button>
                            </div>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.runs.length > 0 ? (
                <ul aria-label='Playground runs without a known result' className='flex flex-col gap-2'>
                    {local.runs.map((r) => (
                        <li key={r.id} className='flex flex-col gap-1 text-[13px]' data-slot='gateway-local-run'>
                            <p>
                                {`Playground run in ${r.workspace.name} (${r.model}) ${r.end === 'stopped' ? 'was stopped' : 'failed'} after it was sent${
                                    r.answered ? ' and answered' : ''
                                }: the Gateway may have run it and charged for it.${r.failure ? ` ${INFERENCE_FAILURE_TEXT[r.failure]}` : ''} Trace ${r.trace} · ${keptAt(
                                    r.settledAt
                                )}. Its output is not kept.`}
                            </p>
                            <Button
                                type='button'
                                tier='tertiary'
                                size='sm'
                                className='self-start'
                                aria-label={`Dismiss run: ${r.trace}`}
                                onClick={() => void discard(r.id)}
                            >
                                Dismiss
                            </Button>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.files.length > 0 ? (
                <ul aria-label='Gateway file changes without a known result' className='flex flex-col gap-2'>
                    {local.files.map((r) => (
                        <li key={r.id} className='flex flex-col gap-1 text-[13px]' data-slot='gateway-local-file'>
                            <p>
                                {`${r.action === 'upload' ? 'Upload' : 'Delete'} of ${r.filename ?? r.file ?? 'a file'}${r.bytes !== null ? ` (${sizeText(r.bytes)})` : ''} in ${
                                    r.workspace.name
                                } was sent without a known result: the file ${r.action === 'upload' ? 'may have been stored' : 'may have been deleted'}.${
                                    r.failure ? ` ${fileFailureText(r.failure)}` : ''
                                } Trace ${r.trace} · ${keptAt(r.settledAt)}. It is never sent again; read the workspace's files to see what exists.`}
                            </p>
                            <Button
                                type='button'
                                tier='tertiary'
                                size='sm'
                                className='self-start'
                                aria-label={`Dismiss file record: ${r.trace}`}
                                onClick={() => void discard(r.id)}
                            >
                                Dismiss
                            </Button>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.playground.length > 0 ? (
                <ul aria-label='Playground drafts' className='flex flex-col gap-2'>
                    {local.playground.map((d) => (
                        <li
                            key={d.id}
                            ref={(li) => {
                                if (li) items.current.set(d.id, li)
                                else items.current.delete(d.id)
                            }}
                            className='flex flex-col gap-1 text-[13px]'
                            data-slot='gateway-local-playground'
                        >
                            <p>
                                {`Playground draft for ${d.workspace.name} · ${d.model || 'no model yet'} · saved ${keptAt(d.savedAt)}. It opens in the Playground when ${d.workspace.name} is chosen; nothing is sent until you press Run.`}
                            </p>
                            {editing === d.id && local.state === 'ready' ? (
                                <KeptDraftEditor
                                    draft={d}
                                    context={context}
                                    dispatch={dispatch}
                                    onDone={() => {
                                        setEditing(null)
                                        requestAnimationFrame(() =>
                                            items.current.get(d.id)?.querySelector<HTMLButtonElement>('button[aria-label^="Edit Playground draft"]')?.focus()
                                        )
                                    }}
                                />
                            ) : (
                                <p className='text-academy-ink-soft line-clamp-2 whitespace-pre-wrap'>{d.prompt}</p>
                            )}
                            <div className='flex gap-2'>
                                {editing !== d.id && local.state === 'ready' ? (
                                    <Button
                                        type='button'
                                        tier='secondary'
                                        size='sm'
                                        aria-label={`Edit Playground draft for ${d.workspace.name}`}
                                        onClick={() => setEditing(d.id)}
                                    >
                                        Edit
                                    </Button>
                                ) : null}
                                <Button
                                    type='button'
                                    tier='tertiary'
                                    size='sm'
                                    aria-label={`Discard Playground draft for ${d.workspace.name}`}
                                    onClick={() => void discard(d.id)}
                                >
                                    Discard
                                </Button>
                            </div>
                        </li>
                    ))}
                </ul>
            ) : null}
            {local.drafts.length > 0 ? (
                <ul aria-label='Drafts' className='flex flex-col gap-2'>
                    {local.drafts.map((d) => (
                        <li key={d.id} className='flex flex-wrap items-center gap-3 text-[13px]' data-slot='gateway-local-draft'>
                            <span>
                                Draft: {keptLabel(d)} · saved {keptAt(d.savedAt)}
                            </span>
                            <Button type='button' tier='secondary' size='sm' aria-label={`Open draft: ${keptLabel(d)}`} onClick={(e) => onOpen(d, d.id, e.currentTarget)}>
                                Open…
                            </Button>
                            <Button type='button' tier='tertiary' size='sm' aria-label={`Discard draft: ${keptLabel(d)}`} onClick={() => void discard(d.id)}>
                                Discard
                            </Button>
                        </li>
                    ))}
                </ul>
            ) : null}
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </section>
    )
}

type DetailState =
    | { kind: 'loading'; id: string; name: string }
    | { kind: 'failed'; id: string; name: string; message: string }
    | { kind: 'shown'; id: string; name: string; detail: GatewayDetail }

type StartChange = (action: ChangeAction, target: string, name: string, from: HTMLElement, body?: Record<string, unknown>) => void

/** v2 (C31): the inventory's label for an admitted (kind, action), if admitted. */
const admittedLabel = (actions: AdminAction[] | undefined, kind: ChangeKind, action: ChangeAction) => actions?.find((a) => a.kind === kind && a.action === action)?.label

/** The changes offered for an item of `resource`: the inventory's admitted
 * row actions in v2 (C31), else the v1 set. */
function itemActions(resource: GatewayResource, admitted?: AdminAction[]): [ChangeAction, string][] {
    if (admitted) return admitted.filter((a) => a.kind === KIND[resource] && a.row).map((a) => [a.action, a.label])
    const actions: [ChangeAction, string][] = [['update', 'Edit…']]
    if (resource === 'serviceKeys' || resource === 'userKeys') actions.push(['rotate', 'Rotate…'])
    if (resource === 'integrations') actions.push(['models', 'Models…'])
    actions.push(['delete', 'Delete…'])
    return actions
}

function InventoryPanel({
    resource,
    held,
    onRefresh,
    read,
    onChange,
    actions,
}: {
    resource: GatewayResource
    held: Held<GatewayList>
    onRefresh: () => void
    read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>
    onChange: StartChange
    actions?: AdminAction[]
}) {
    const create = actions ? actions.find((a) => a.kind === KIND[resource] && a.action === 'create' && !a.row)?.label : `New ${NOUN[resource]}…`
    const what = LABEL[resource]
    const [detail, setDetail] = useState<DetailState | null>(null)
    const opener = useRef<HTMLButtonElement | null>(null)
    const token = useRef(0)
    const list = held.read
    const rows = list ? rowsOf(list) : []

    async function open(row: Record<string, unknown>, button: HTMLButtonElement) {
        opener.current = button
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
                {create ? (
                    <Button type='button' tier='secondary' size='sm' onClick={(e) => onChange('create', '', '', e.currentTarget)}>
                        {create}
                    </Button>
                ) : null}
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
            {detail ? (
                // Keyed by item: another item never shows this one's models,
                // loading state or failure.
                <DetailPanel key={detail.id} state={detail} resource={resource} onClose={close} read={read} onChange={onChange} actions={actions} />
            ) : null}
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
    onChange,
    actions,
}: {
    state: DetailState
    resource: GatewayResource
    onClose: () => void
    read: (r: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>
    onChange: StartChange
    actions?: AdminAction[]
}) {
    const headingId = useId()
    const heading = useRef<HTMLHeadingElement>(null)
    const [models, setModels] = useState<Held<GatewayModels>>(empty)
    // The integration this panel shows; a models reply for any other (or
    // after the panel closed) is dropped.
    const shownId = useRef<string | null>(state.id)
    shownId.current = state.id
    useEffect(
        () => () => {
            shownId.current = null
        },
        []
    )
    useEffect(() => {
        heading.current?.focus()
    }, [state.kind])
    const item: Record<string, unknown> | null =
        state.kind === 'shown' && state.detail.item ? (JSON.parse(state.detail.item) as Record<string, unknown>) : null

    async function readModels(id: string) {
        setModels((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'integrationModels', id }, false)
        if (shownId.current !== id) return
        const g = gatewayResult(result)
        setModels((h) =>
            g?.kind === 'models' && g.models.integration === id
                ? { read: g.models, loading: false, failure: null }
                : { ...h, loading: false, failure: resultFailure(result) }
        )
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
            {state.kind === 'shown' && state.detail.found === 'found' ? (
                <div className='flex flex-wrap gap-2' data-slot='gateway-item-actions'>
                    {itemActions(resource, actions).map(([action, label]) => (
                        <Button
                            key={action}
                            type='button'
                            tier='secondary'
                            size='sm'
                            aria-label={`${label.replace('…', '')} ${state.name}`}
                            onClick={(e) =>
                                onChange(
                                    action,
                                    state.id,
                                    state.name,
                                    e.currentTarget,
                                    action === 'rotate'
                                        ? { key_transition_period_ms: 3600000 }
                                        : action === 'models'
                                          ? {
                                                models: models.read
                                                    ? (JSON.parse(models.read.models) as Record<string, unknown>[]).map((m) => ({ slug: m.slug, enabled: m.enabled }))
                                                    : [],
                                            }
                                          : undefined
                                )
                            }
                        >
                            {label}
                        </Button>
                    ))}
                </div>
            ) : null}
            <Button type='button' tier='tertiary' className='self-start' onClick={onClose}>
                Close details
            </Button>
        </section>
    )
}

/** The chosen workspace's own detail: settings, defaults and embedded
 * limits (limit policies are separate and not read here). */
function WorkspacePanel({
    name,
    held,
    onRefresh,
    onChange,
    actions,
}: {
    name: string
    held: Held<GatewayDetail>
    onRefresh: () => void
    onChange: (action: ChangeAction, from: HTMLElement) => void
    actions?: AdminAction[]
}) {
    const d = held.read
    const item: Record<string, unknown> | null = d?.item ? (JSON.parse(d.item) as Record<string, unknown>) : null
    return (
        <div className='flex flex-col gap-3' data-slot='gateway-workspace-detail'>
            <div className='flex flex-wrap items-center gap-3'>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    {held.loading ? 'Reading the workspace…' : item ? `Settings of ${name} as read from the service.` : 'Not read yet'}
                </p>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    <RefreshCw aria-hidden className='size-4' /> Refresh workspace
                </Button>
            </div>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='gateway-workspace-failure'>
                    The workspace could not be read: {held.failure}
                </Callout>
            ) : null}
            {d && d.found !== 'found' ? (
                <p className='text-[13px]'>The service did not return this workspace. Refresh the workspaces.</p>
            ) : null}
            {item ? (
                <dl className='grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='gateway-workspace-fields'>
                    {Object.entries(item).map(([key, value]) => (
                        <div key={key} className='contents'>
                            <dt className='text-academy-ink-soft font-mono text-[12px] break-all'>{key}</dt>
                            <dd className='font-mono text-[12px] break-all whitespace-pre-wrap'>{fieldText(value)}</dd>
                        </div>
                    ))}
                </dl>
            ) : null}
            <div className='flex flex-wrap gap-2'>
                {!actions || admittedLabel(actions, 'gatewayWorkspaces', 'update') ? (
                    <Button type='button' tier='secondary' size='sm' onClick={(e) => onChange('update', e.currentTarget)}>
                        {admittedLabel(actions, 'gatewayWorkspaces', 'update') ?? 'Edit workspace…'}
                    </Button>
                ) : null}
                {!actions || admittedLabel(actions, 'gatewayWorkspaces', 'delete') ? (
                    <Button type='button' tier='secondary' size='sm' onClick={(e) => onChange('delete', e.currentTarget)}>
                        {admittedLabel(actions, 'gatewayWorkspaces', 'delete') ?? 'Archive workspace…'}
                    </Button>
                ) : null}
            </div>
        </div>
    )
}

function sourceText(list: GatewayList | null, failure: string | null, what: string): string {
    if (failure) return `Unavailable: ${failure}`
    return list ? coverage(list, what) : 'Not read'
}

/** Where this workspace stands for inference from the desktop, from the same key state the Playground uses. */
function inferenceText(snapshot: ShellSnapshot, workspace: ListedWorkspace): string {
    const { key, target, here } = inferenceKey(snapshot, workspace)
    if (here && target)
        return `An inference key for ${workspace.name} is stored (${key?.mode === 'session' ? 'this session only' : 'Keychain'}) and is sent only to ${target.endpoint}. Run prompts from the Playground tab.`
    if (key?.state === 'ready' && target)
        return `The stored inference key is for workspace ${target.workspace.name}. To run inference from ${workspace.name}, store a key for it in the Playground tab (it replaces that one).`
    if (key?.state === 'storing') return 'An inference key is being stored.'
    return `No inference key is stored for ${workspace.name}. To run inference from here, store the workspace's runtime endpoint and an API key with completions access in the Playground tab.`
}

interface PrerequisitesProps {
    held: Held<GatewayPrerequisites>
    onRefresh: () => void
    snapshot: ShellSnapshot
    workspace: ListedWorkspace
    onPlayground: () => void
}

function PrerequisitesPanel({ held, onRefresh, snapshot, workspace, onPlayground }: PrerequisitesProps) {
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
                        <div className='flex flex-col items-start gap-2'>
                            <p>{`${inferenceText(snapshot, workspace)} The management client is never used in its place.`}</p>
                            <Button type='button' tier='secondary' size='sm' onClick={onPlayground}>
                                Open Playground
                            </Button>
                        </div>
                    </Callout>
                </>
            ) : null}
        </div>
    )
}
