import { forwardRef, type ReactNode, useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { AdminProduct, AdminTab, DesktopClient, FeatureId, ManagementResource, ProductReadState, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { CHANGE_NOUN, type ChangeTarget, verbOf } from '../../modules/administration/runtime-changes'
import type { Dispatch } from '../../modules/tenants/dispatch'
import type { Act } from '../inbox/inbox'
import { type Arrival, useShellStore } from '../store'
import { SHELL_COPY } from '../copy'
import { Verdict } from '../verdict'
import { PageIntro } from '../page-intro'
import { ADMIN_COPY as C } from './copy'
import { ReviewSheet } from './review-sheet'
import { InventoryTable } from './inventory-table'
import { Playground } from './playground'
import { type PillTone, ProductCard } from './product-card'
import { RuntimeOverview } from './runtime-overview'
import { GatewayProduct, type GatewayTabId } from './gateway-tabs'
import { ObservationProduct } from './observation-tabs'
import type { GlyphName } from '../icons'
import { Btn } from '../controls'

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
    _ref
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

    // An arrival is consumed once, when its target is in the inventory (C35 attempt-02 R1):
    // a fresh snapshot never re-applies it over what the operator chose since. The rail's
    // product children and the tenant menu's "Change Gateway workspace…" name a product
    // (and a tab); a contextual action (Inbox, Home) names a Runtime inventory (C30).
    const consumedArrival = useRef<number | null>(null)
    useEffect(() => {
        if (!arrival || consumedArrival.current === arrival.sequence) return
        if (arrival.product) {
            const product = products.find((p) => p.id === arrival.product)
            if (!product) return
            const tab = arrival.tab
                ? product.tabs.find((t) => t.id === arrival.tab && t.disposition.kind !== 'removed')
                : arrival.resource
                  ? product.tabs.find((t) => t.resource === arrival.resource) ?? (product.id === 'runtimeSecurity' && DLP.includes(arrival.resource) ? product.tabs.find((t) => t.label === 'DLP') : undefined)
                  : undefined
            // Product and resource are one arrival: do not discard a Runtime
            // inventory/filter when retaining its product, or fall back into
            // another product while its inventory has not arrived yet.
            if ((arrival.tab || arrival.resource) && !tab) return
            consumedArrival.current = arrival.sequence
            setOpen((o) => ({ ...o, [product.id]: true }))
            if (tab) setTabOf((s) => ({ ...s, [product.id]: tab.id }))
            if (arrival.resource && product.id === 'runtimeSecurity' && DLP.includes(arrival.resource)) setDlp(arrival.resource)
            return
        }
        if (!arrival.resource) {
            consumedArrival.current = arrival.sequence
            return
        }
        const resource = arrival.resource
        const runtime = products.find((p) => p.id === 'runtimeSecurity')
        const tab = runtime?.tabs.find((t) => t.resource === resource) ?? (DLP.includes(resource) ? runtime?.tabs.find((t) => t.label === 'DLP') : undefined)
        if (!tab) return
        consumedArrival.current = arrival.sequence
        setOpen((o) => ({ ...o, runtimeSecurity: true }))
        setTabOf((s) => ({ ...s, runtimeSecurity: tab.id }))
        if (DLP.includes(resource)) setDlp(resource)
    }, [arrival, products])

    if (!tenant || !context) {
        return (
            <section aria-label={C.title} data-slot='administration-v2'>
                <PageIntro description={C.noTenant} />
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
                    <RuntimeOverview
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
                return <Playground key={key} snapshot={snapshot} dispatch={dispatch} onNavigate={onNavigate} prefill={handed?.prefill ?? null} caseId={handed?.caseId ?? null} />
            }
            const resource = (tab.label === 'DLP' ? dlp : tab.resource) as ManagementResource | null
            if (!resource) return null
            return (
                <div className='flex flex-col gap-3'>
                    {tab.label === 'DLP' ? (
                        <div role='radiogroup' aria-label={C.dlpResources} className='flex flex-wrap rounded-[8px] self-start' style={{ gap: 2, padding: 2, background: 'var(--v2-inset)' }} data-slot='dlp-resources'>
                            {DLP.map((r) => (
                                <button
                                    key={r}
                                    type='button'
                                    role='radio'
                                    aria-checked={dlp === r}
                                    onClick={() => setDlp(r)}
                                    className='h-7 min-h-0 rounded-[6px] px-3 text-[12px] font-semibold'
                                    style={{
                                        background: dlp === r ? 'var(--v2-paper)' : 'transparent',
                                        boxShadow: dlp === r ? '0 1px 2px rgba(0,0,0,0.12)' : 'none',
                                        color: 'var(--v2-ink)',
                                    }}
                                >
                                    {DLP_LABEL[r]}
                                </button>
                            ))}
                        </div>
                    ) : null}
                    <InventoryTable
                        key={`${key}-${resource}`}
                        snapshot={snapshot}
                        context={context!}
                        dispatch={dispatch}
                        observe={observe}
                        resource={resource}
                        arrival={arrival && arrival.resource === resource ? arrival : null}
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
            return (
                <GatewayProduct
                    key={`${context}-gateway`}
                    snapshot={snapshot}
                    context={context!}
                    dispatch={dispatch}
                    observe={observe}
                    onNavigate={onNavigate}
                    tab={tab.id as GatewayTabId}
                    actions={tab.actions}
                    onReview={(change, extras) => setSheet({ change, rotation: null, ...extras })}
                    onRead={() => void onAct(SHELL_COPY.readOf(tenant!.label), { kind: 'readTenant', context: context! })}
                />
            )
        }
        return <ObservationProduct key={`${context}-${product.id}`} snapshot={snapshot} context={context!} dispatch={dispatch} observe={observe} product={product.id as 'redTeam' | 'modelSkill'} tab={tab.id} />
    }

    const PILL_WORD: Record<ProductReadState | 'unavailable', { tone: PillTone; glyph: GlyphName }> = {
        ok: { tone: 'ok', glyph: 'check' },
        incomplete: { tone: 'flag', glyph: 'alert' },
        needsRole: { tone: 'flag', glyph: 'alert' },
        failed: { tone: 'flag', glyph: 'alert' },
        pending: { tone: 'faint', glyph: 'clock' },
        notRead: { tone: 'faint', glyph: 'clock' },
        unavailable: { tone: 'faint', glyph: 'ban' },
    }
    const reads = snapshot.inbox.read.products
    const overview = snapshot.overview.read
    const chrome = (product: AdminProduct) => {
        const read = reads.find((r) => r.product === product.id)
        const state: ProductReadState | 'unavailable' = product.availability.state === 'unavailable' ? 'unavailable' : (read?.state ?? 'notRead')
        const alerts = snapshot.inbox.items.filter((e) => e.product === product.id && e.needsAction && e.hold === null).length
        const value =
            product.id === 'runtimeSecurity'
                ? overview?.summary.detectionEvents ?? null
                : product.id === 'gateway'
                  ? overview?.gateway?.requests ?? null
                  : null
        return {
            pill: { word: C.status[state] ?? state, ...PILL_WORD[state] },
            stat: { value: value === null ? '—' : value.toLocaleString(), label: C.stat[product.id] ?? '', hot: product.id === 'runtimeSecurity' && value !== null && value > 0 },
            alerts,
        }
    }
    const available = products.filter((p) => p.availability.state === 'available')
    const setAll = (value: boolean) => setOpen(Object.fromEntries(available.map((p) => [p.id, value])))
    return (
        <section aria-label={C.title} data-slot='administration-v2' className='flex min-w-0 flex-col' style={{ gap: 14 }}>
            <PageIntro
                description={<p data-slot='admin-sub'>{C.lead(tenant.label)}</p>}
                actions={<>
                    <Btn onClick={() => setAll(true)}>{C.expandAll}</Btn>
                    <Btn onClick={() => setAll(false)}>{C.collapseAll}</Btn>
                </>}
            />
            <ul className='flex flex-col' style={{ gap: 14 }} data-slot='admin-products'>
                {products.map((product) => {
                    const tabs = product.tabs.filter((t) => t.disposition.kind !== 'removed')
                    const current = tabs.find((t) => t.id === tabOf[product.id]) ?? tabs[0]
                    const { pill, stat, alerts } = chrome(product)
                    return (
                        <ProductCard
                            key={product.id}
                            product={product}
                            open={!!open[product.id]}
                            onToggle={() => setOpen((o) => ({ ...o, [product.id]: !o[product.id] }))}
                            stat={stat}
                            pill={pill}
                            alerts={alerts}
                            tabs={tabs}
                            current={current}
                            onTab={(id) => setTabOf((s) => ({ ...s, [product.id]: id }))}
                            note={product.readOnly && current?.label !== 'Overview' ? C.readOnly : null}
                        >
                            {current ? body(product, current) : null}
                        </ProductCard>
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
