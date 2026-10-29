import { forwardRef, type ReactNode, useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { AdminProduct, AdminTab, DesktopClient, FeatureId, ManagementResource, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { AdministrationView } from '../../features/administration/administration-view'
import { CHANGE_NOUN, type ChangeTarget, verbOf } from '../../features/administration/runtime-changes'
import { GatewayView, type GatewayTab } from '../../features/gateway/gateway-view'
import { RedTeamView, type RedTeamSection, type RedTeamTab } from '../../features/red-team/red-team-view'
import { RuntimeView } from '../../features/runtime/runtime-view'
import type { Dispatch } from '../../features/tenants/dispatch'
import type { Act } from '../inbox/inbox'
import { type Arrival, useShellStore } from '../store'
import { SHELL_COPY } from '../copy'
import { Verdict } from '../verdict'
import { ADMIN_COPY as C } from './copy'
import { ReviewSheet } from './review-sheet'

export interface AdministrationProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    client: DesktopClient
    onAct: Act
    onNavigate: (feature: FeatureId) => void
    arrival: Arrival | null
}

const DLP: ManagementResource[] = ['dataPatterns', 'dictionaries', 'dataProfiles', 'filteringProfiles', 'dlpCatalog']
const DLP_LABEL: Record<string, string> = {
    dataPatterns: 'Data patterns',
    dictionaries: 'Dictionaries',
    dataProfiles: 'Data profiles',
    filteringProfiles: 'Data filtering profiles',
    dlpCatalog: 'DLP profiles (AIRS)',
}
const DLP_KIND: Partial<Record<ManagementResource, string>> = {
    dataPatterns: 'dataPatterns',
    dictionaries: 'dictionaries',
    dataProfiles: 'dataProfiles',
    filteringProfiles: 'filteringProfiles',
}
const GATEWAY_TAB: Record<string, GatewayTab> = {
    overview: 'usage',
    workspaces: 'workspace',
    routes: 'routes',
    providers: 'providers',
    guardrails: 'guardrails',
    integrations: 'integrations',
    logs: 'logs',
}
const RED_TEAM_TAB = (product: AdminProduct): RedTeamTab => (product.id === 'modelSkill' ? 'models' : 'redTeam')
/** Each read-only tab renders its own read surface (screens/administration-inventory.md). */
const SECTION: Record<string, Record<string, RedTeamSection>> = {
    redTeam: {
        overview: 'overview',
        targets: 'targets',
        scans: 'scans',
        'attack-results': 'attacks',
        reports: 'reports',
    },
    modelSkill: {
        overview: 'modelOverview',
        'model-scans': 'modelScans',
        'skill-scans': 'skillScans',
    },
}

interface Sheet {
    change: ChangeTarget | null
    rotation: string | null
    onDraft?: (body: string) => void
    note?: ReactNode
}

/**
 * Administration (C31, screens/reviews-administration.md): the accordion of
 * the products in `AdminInventory`, each tab rendered by its disposition.
 * Read and Change tabs render the retained views (the same reads and
 * reviewed changes), with the inventory's labels for exactly its actions;
 * Unavailable tabs and products show their gate and nothing else; Removed
 * tabs are not rendered. Every change opens the review sheet; a key rotation
 * follows the core's staged sequence there.
 */
export const Administration = forwardRef<HTMLHeadingElement, AdministrationProps>(function Administration(
    { snapshot, dispatch, client, onAct, onNavigate, arrival },
    ref
) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const context = snapshot.active?.context ?? null
    const products = snapshot.administration?.products ?? []
    const [open, setOpen] = useState<Record<string, boolean>>({
        runtimeSecurity: true,
    })
    const [tabOf, setTabOf] = useState<Record<string, string>>({
        runtimeSecurity: 'custom-topics',
        gateway: 'routes',
        redTeam: 'scans',
        modelSkill: 'skill-scans',
    })
    const [dlp, setDlp] = useState<ManagementResource>('dataPatterns')
    const [gatewayKeys, setGatewayKeys] = useState<GatewayTab>('serviceKeys')
    const [sheet, setSheet] = useState<Sheet | null>(null)
    const observe = useCallback((operation: string) => client.observeManagement(operation), [client])

    // A Harness proposal the operator opened into a review (C21) arrives
    // here: the sheet shows its exact draft under the native review, on the
    // product and tab of its kind; typed confirmation and native approval
    // still apply, and leaving the sheet discards the review.
    const review = snapshot.changes.review
    const proposed = snapshot.harness.conversation?.proposals.find((p) => p.state === 'inReview' && review !== null && p.review === review.review) ?? null
    const shownProposal = useRef<string | null>(null)
    useEffect(() => {
        if (!proposed || shownProposal.current === proposed.review) return
        shownProposal.current = proposed.review
        const d = proposed.draft
        for (const p of products) {
            const t = p.tabs.find((x) => x.disposition.kind === 'change' && x.actions.some((a) => a.kind === d.kind))
            if (!t) continue
            setOpen((o) => ({ ...o, [p.id]: true }))
            setTabOf((s) => ({ ...s, [p.id]: t.id }))
            if (DLP.includes(d.kind as ManagementResource)) setDlp(d.kind as ManagementResource)
            break
        }
        setSheet({
            change: {
                kind: d.kind,
                action: d.action,
                target: d.target,
                title: `Proposed by the Harness: ${verbOf(d.kind, d.action).toLowerCase()} ${CHANGE_NOUN[d.kind]}${d.target ? ` ${d.target}` : ''}`,
                body: d.body,
            },
            rotation: null,
        })
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per opened proposal review
    }, [proposed])

    // A Runtime case opened in the Playground from Test Cases (C33): Runtime
    // Security's Playground tab, filled with the case.
    const playground = useShellStore((s) => s.playground)
    useEffect(() => {
        if (!playground || playground.tenant !== tenant?.id) return
        const tab = products.find((p) => p.id === 'runtimeSecurity')?.tabs.find((t) => t.label === 'Playground')
        if (!tab) return
        setOpen((o) => ({ ...o, runtimeSecurity: true }))
        setTabOf((s) => ({ ...s, runtimeSecurity: tab.id }))
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per handed case
    }, [playground?.prefill.nonce, products.length])

    // The rail's product children open that product's section (C35).
    useEffect(() => {
        if (!arrival?.product) return
        setOpen((o) => ({ ...o, [arrival.product!]: true }))
    }, [arrival])
    // A contextual action (Inbox, Home) opens the product and tab it names (C30).
    useEffect(() => {
        if (!arrival || !arrival.resource) return
        const resource = arrival.resource
        const runtime = products.find((p) => p.id === 'runtimeSecurity')
        const tab = runtime?.tabs.find((t) => t.resource === resource) ?? (DLP.includes(resource) ? runtime?.tabs.find((t) => t.label === 'DLP') : undefined)
        if (!tab) return
        setOpen((o) => ({ ...o, runtimeSecurity: true }))
        setTabOf((s) => ({ ...s, runtimeSecurity: tab.id }))
        if (DLP.includes(resource)) setDlp(resource)
    }, [arrival, products])

    if (!tenant || !context) {
        return (
            <section aria-labelledby='administration-heading' data-slot='administration-v2'>
                <h1 ref={ref} id='administration-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {C.title}
                </h1>
                <p className='mt-2 text-[13px]'>{C.noTenant}</p>
            </section>
        )
    }

    const labelsOf = (tab: AdminTab, kind?: string) =>
        Object.fromEntries(tab.actions.filter((a) => !kind || a.kind === kind).map((a) => [a.action, a.label])) as Partial<Record<ChangeTarget['action'], string>>

    function body(product: AdminProduct, tab: AdminTab) {
        if (tab.disposition.kind === 'unavailable') {
            return (
                <div data-slot='admin-unavailable' className='rounded-[12px] border p-4 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                    <Verdict tone='info' word={C.unavailableTab} />
                    <p className='mt-2' style={{ color: 'var(--v2-soft)' }}>
                        {tab.disposition.reason}
                    </p>
                </div>
            )
        }
        const key = `${context}-${product.id}-${tab.id}`
        if (product.id === 'runtimeSecurity') {
            if (tab.label === 'Overview')
                return (
                    <RuntimeOverviewTab
                        snapshot={snapshot}
                        onRead={() =>
                            void onAct(SHELL_COPY.readOf(tenant!.label), {
                                kind: 'readTenant',
                                context: context!,
                            })
                        }
                    />
                )
            if (tab.label === 'Playground') {
                // A case handed over from Test Cases fills the form (C33); the operator runs it.
                const handed = playground && playground.tenant === tenant!.id ? playground : null
                return <RuntimeView key={key} snapshot={snapshot} dispatch={dispatch} onNavigate={onNavigate} prefill={handed?.prefill ?? null} caseId={handed?.caseId ?? null} embedded />
            }
            const resource = (tab.label === 'DLP' ? dlp : tab.resource) as ManagementResource | null
            if (!resource) return null
            return (
                <div className='flex flex-col gap-3'>
                    {tab.label === 'DLP' ? (
                        <div role='radiogroup' aria-label={C.dlpResources} className='flex flex-wrap gap-1' data-slot='dlp-resources'>
                            {DLP.map((r) => (
                                <button
                                    key={r}
                                    type='button'
                                    role='radio'
                                    aria-checked={dlp === r}
                                    onClick={() => setDlp(r)}
                                    className='h-8 rounded-[7px] px-3 text-[13px]'
                                    style={{
                                        background: dlp === r ? 'var(--v2-sel)' : 'transparent',
                                        color: dlp === r ? 'var(--v2-sel-ink)' : 'var(--v2-ink)',
                                    }}
                                >
                                    {DLP_LABEL[r]}
                                </button>
                            ))}
                        </div>
                    ) : null}
                    <AdministrationView
                        key={`${key}-${resource}`}
                        snapshot={snapshot}
                        dispatch={dispatch}
                        observe={observe}
                        onNavigate={onNavigate}
                        arrival={arrival && arrival.resource === resource ? arrival : null}
                        pinned={resource}
                        embedded
                        labels={labelsOf(tab, tab.label === 'DLP' ? DLP_KIND[resource] : undefined)}
                        admitted={tab.actions}
                        onReview={(change) => setSheet({ change, rotation: null })}
                        rotations={snapshot.rotations}
                        onFinishRotation={(application) => setSheet({ change: null, rotation: application })}
                    />
                </div>
            )
        }
        if (product.id === 'gateway') {
            if (tab.label === 'MCP servers') return <McpTab snapshot={snapshot} dispatch={dispatch} />
            const gtab = tab.label === 'Keys' ? gatewayKeys : GATEWAY_TAB[tab.id]
            return (
                <div className='flex flex-col gap-3'>
                    {tab.label === 'Keys' ? (
                        <div role='radiogroup' aria-label={C.keyKinds} className='flex gap-1'>
                            {(['serviceKeys', 'userKeys'] as const).map((k) => (
                                <button
                                    key={k}
                                    type='button'
                                    role='radio'
                                    aria-checked={gatewayKeys === k}
                                    onClick={() => setGatewayKeys(k)}
                                    className='h-8 rounded-[7px] px-3 text-[13px]'
                                    style={{
                                        background: gatewayKeys === k ? 'var(--v2-sel)' : 'transparent',
                                        color: gatewayKeys === k ? 'var(--v2-sel-ink)' : 'var(--v2-ink)',
                                    }}
                                >
                                    {k === 'serviceKeys' ? C.serviceKeys : C.userKeys}
                                </button>
                            ))}
                        </div>
                    ) : null}
                    <GatewayView
                        key={`${context}-gateway`}
                        snapshot={snapshot}
                        dispatch={dispatch}
                        observe={observe}
                        onNavigate={onNavigate}
                        tab={gtab}
                        embedded
                        actions={tab.actions}
                        onReview={(change, extras) => setSheet({ change, rotation: null, ...extras })}
                    />
                </div>
            )
        }
        return (
            <div className='flex flex-col gap-2'>
                <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                    {C.readOnly}
                </p>
                <RedTeamView
                    key={`${context}-${product.id}`}
                    snapshot={snapshot}
                    dispatch={dispatch}
                    observe={observe}
                    onNavigate={onNavigate}
                    tab={RED_TEAM_TAB(product)}
                    section={SECTION[product.id]?.[tab.id]}
                    embedded
                />
            </div>
        )
    }

    return (
        <section aria-labelledby='administration-heading' data-slot='administration-v2' className='flex min-w-0 flex-col gap-4'>
            <div>
                <h1 ref={ref} id='administration-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {C.title}
                </h1>
                <p className='mt-1 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                    {C.lead(tenant.label)}
                </p>
            </div>
            <ul className='flex flex-col gap-3' data-slot='admin-products'>
                {products.map((product) => {
                    const unavailable = product.availability.state === 'unavailable'
                    const expanded = !!open[product.id] && !unavailable
                    const tabs = product.tabs.filter((t) => t.disposition.kind !== 'removed')
                    const current = tabs.find((t) => t.id === tabOf[product.id]) ?? tabs[0]
                    const panel = `admin-${product.id}`
                    return (
                        <li
                            key={product.id}
                            data-slot='admin-product'
                            data-product={product.id}
                            className='min-w-0 rounded-[12px] border'
                            style={{
                                borderColor: 'var(--v2-rule)',
                                background: 'var(--v2-paper)',
                            }}
                        >
                            <h2 className='m-0'>
                                <button
                                    type='button'
                                    aria-expanded={unavailable ? undefined : expanded}
                                    aria-controls={unavailable ? undefined : panel}
                                    aria-disabled={unavailable || undefined}
                                    onClick={() => !unavailable && setOpen((o) => ({ ...o, [product.id]: !o[product.id] }))}
                                    className='flex w-full flex-wrap items-center gap-3 px-4 py-3 text-left text-[15px] font-semibold'
                                >
                                    <span className='flex-grow'>{product.label}</span>
                                    {unavailable ? (
                                        <Verdict tone='info' word={C.unavailableTab} />
                                    ) : product.readOnly ? (
                                        <span className='text-[12px] font-normal' style={{ color: 'var(--v2-soft)' }}>
                                            Read-only
                                        </span>
                                    ) : null}
                                </button>
                            </h2>
                            {unavailable && product.availability.state === 'unavailable' ? (
                                <p data-slot='admin-product-gate' className='px-4 pb-3 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                                    {product.availability.reason}
                                </p>
                            ) : null}
                            {expanded && current ? (
                                <div id={panel} className='flex min-w-0 flex-col gap-3 px-4 pb-4'>
                                    <div role='tablist' aria-label={C.tabs(product.label)} className='flex flex-wrap gap-1'>
                                        {tabs.map((t) => (
                                            <button
                                                key={t.id}
                                                type='button'
                                                role='tab'
                                                aria-selected={t.id === current.id}
                                                aria-controls={`${panel}-tab`}
                                                onClick={() => setTabOf((s) => ({ ...s, [product.id]: t.id }))}
                                                className='h-8 rounded-[7px] px-3 text-[13px]'
                                                style={{
                                                    background: t.id === current.id ? 'var(--v2-sel)' : 'transparent',
                                                    color:
                                                        t.id === current.id
                                                            ? 'var(--v2-sel-ink)'
                                                            : t.disposition.kind === 'unavailable'
                                                              ? 'var(--v2-faint)'
                                                              : 'var(--v2-ink)',
                                                }}
                                            >
                                                {t.label}
                                            </button>
                                        ))}
                                    </div>
                                    <div
                                        role='tabpanel'
                                        id={`${panel}-tab`}
                                        aria-label={current.label}
                                        data-slot='admin-tab'
                                        data-tab={current.id}
                                        data-disposition={current.disposition.kind}
                                        className='min-w-0'
                                    >
                                        {body(product, current)}
                                    </div>
                                </div>
                            ) : null}
                        </li>
                    )
                })}
            </ul>
            {/* The sheet is the topmost dialog (I8): rendered at the body, outside the page the modal stack hides. */}
            {sheet
                ? createPortal(
                      <ReviewSheet
                          snapshot={snapshot}
                          context={context}
                          dispatch={dispatch}
                          observe={observe}
                          change={sheet.change}
                          rotation={sheet.rotation}
                          onDraft={sheet.onDraft}
                          note={sheet.note}
                          onClose={() => setSheet(null)}
                      />,
                      document.body
                  )
                : null}
        </section>
    )
})

/** Runtime Overview: what the tenant's read established (C30), never invented figures. */
function RuntimeOverviewTab({ snapshot, onRead }: { snapshot: ShellSnapshot; onRead: () => void }) {
    const read = snapshot.overview.read
    const cell = (label: string, value: string) => (
        <div className='rounded-[10px] border px-3 py-2' style={{ borderColor: 'var(--v2-rule)' }}>
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {label}
            </p>
            <p className='text-[18px] font-semibold'>{value}</p>
        </div>
    )
    const n = (v: number | null | undefined) => (v === null || v === undefined ? C.overviewRuntime.notRead : v.toLocaleString())
    return (
        <div className='flex flex-col gap-3' data-slot='admin-runtime-overview'>
            <div className='grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))' }}>
                {cell(C.overviewRuntime.sessions, n(read?.summary.sessions))}
                {cell(C.overviewRuntime.violating, n(read?.summary.violating))}
                {cell(C.overviewRuntime.keys, read ? (read.keysFailure ? 'Unavailable' : read.keysComplete ? 'Complete' : 'Partial') : C.overviewRuntime.notRead)}
            </div>
            <div>
                <button type='button' onClick={onRead} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                    {C.overviewRuntime.readNow}
                </button>
            </div>
        </div>
    )
}

/** Gateway MCP servers: the chosen workspace's catalog (`GatewayRead::McpServers`, C22). */
function McpTab({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const mcp = snapshot.harness.mcp
    const context = snapshot.active?.context ?? null
    const workspace = snapshot.gateway.selected?.reference ?? null
    if (!workspace) return <p className='text-[13px]'>{C.noWorkspace}</p>
    if (!mcp || mcp.servers.length === 0) {
        return (
            <div className='flex flex-col gap-2 text-[13px]'>
                <p>{C.mcpNotRead}</p>
                <div>
                    <button
                        type='button'
                        onClick={() => context && void dispatch({ kind: 'listMcpServers', context, workspace })}
                        className='h-8 rounded-[7px] border px-3 text-[13px]'
                        style={{ borderColor: 'var(--v2-rule)' }}
                    >
                        {C.readMcp}
                    </button>
                </div>
            </div>
        )
    }
    return (
        <table className='w-full text-left text-[13px]' data-slot='admin-mcp'>
            <thead>
                <tr>
                    <th scope='col' className='py-1 pr-3 font-medium'>
                        Server
                    </th>
                    <th scope='col' className='py-1 pr-3 font-medium'>
                        Namespace
                    </th>
                    <th scope='col' className='py-1 font-medium'>
                        Status
                    </th>
                </tr>
            </thead>
            <tbody>
                {mcp.servers.map((s) => (
                    <tr key={s.id}>
                        <td className='py-1 pr-3'>{s.name}</td>
                        <td className='py-1 pr-3 font-mono text-[12px]'>{s.namespace}</td>
                        <td className='py-1'>{s.status}</td>
                    </tr>
                ))}
            </tbody>
        </table>
    )
}
