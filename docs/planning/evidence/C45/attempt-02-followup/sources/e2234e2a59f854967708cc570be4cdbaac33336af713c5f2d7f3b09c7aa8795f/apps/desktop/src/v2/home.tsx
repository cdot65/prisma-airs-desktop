import { forwardRef } from 'react'
import type { AdminProduct, AdminProductId, FeatureId, InboxEntry, ProductId, ProductRead, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { HOME_COPY, INBOX_COPY, READ_COPY, SHELL_COPY, SNOOZE_COPY, clockOf } from './copy'
import { Glyph, PRODUCT_GLYPHS, type GlyphName } from './icons'
import { type Act, destinationFeature, follow, holdWhat } from './inbox/inbox'
import { useShellStore } from './store'

export interface HomeProps {
    snapshot: ShellSnapshot
    onAct: Act
    onNavigate: (id: FeatureId) => void
}

/**
 * What the empty attention list may say (I4): an all-clear only after a
 * complete read; otherwise the unread, in-progress, failed or partial
 * coverage is named instead.
 */
export function attentionEmpty(snapshot: ShellSnapshot): string {
    const read = snapshot.inbox.read
    if (read.state === 'pending') return HOME_COPY.firstRead
    if (read.state === 'reading' && read.lastReadAt === null) return HOME_COPY.reading
    if (read.state === 'failed' && read.failedAt !== null) {
        return read.lastReadAt === null ? READ_COPY.failed(clockOf(read.failedAt)) : HOME_COPY.clearSoFar(READ_COPY.failedKept(clockOf(read.failedAt), clockOf(read.lastReadAt)))
    }
    // A read in progress shows the cached read until the new one lands: its age stays named (C12).
    const stale = read.lastReadAt !== null && !snapshot.overview.fresh
    if (read.state === 'reading' && read.lastReadAt !== null) return HOME_COPY.clearSoFar(stale ? READ_COPY.readingStaleQualifier(clockOf(read.lastReadAt)) : READ_COPY.readingQualifier(clockOf(read.lastReadAt)))
    if (read.sourcesMissing.length > 0) return HOME_COPY.clearSoFar(READ_COPY.unread(read.sourcesMissing.length))
    // A healthy empty read older than the freshness window is a gap, not an all-clear (C12).
    if (stale) return HOME_COPY.clearSoFar(READ_COPY.staleQualifier(clockOf(read.lastReadAt ?? 0)))
    return HOME_COPY.allClear
}

/** The read-status line (I4): first read pending, reading, finished (with partial coverage), or failed. */
export function readLine(snapshot: ShellSnapshot, tenant: string | null): { text: string; tone: 'info' | 'flagged' | 'allowed' | 'blocked' } {
    const read = snapshot.inbox.read
    if (!tenant) return { text: HOME_COPY.noTenant, tone: 'info' }
    if (read.state === 'reading' && read.lastReadAt === null) return { text: HOME_COPY.reading, tone: 'info' }
    // Reading again: the cached read stays shown, with its age, until the new one lands.
    if (read.state === 'reading' && read.lastReadAt !== null) {
        const when = clockOf(read.lastReadAt)
        return snapshot.overview.fresh ? { text: READ_COPY.readingKept(when), tone: 'info' } : { text: READ_COPY.readingStale(when), tone: 'flagged' }
    }
    // A failed read is failed, attempted or not before: never "pending", never an all-clear.
    if (read.state === 'failed' && read.failedAt !== null) {
        return read.lastReadAt === null ? { text: READ_COPY.failed(clockOf(read.failedAt)), tone: 'blocked' } : { text: READ_COPY.failedKept(clockOf(read.failedAt), clockOf(read.lastReadAt)), tone: 'blocked' }
    }
    if (read.state === 'pending' || (read.lastReadAt === null && read.settledAt === null)) return { text: HOME_COPY.firstRead, tone: 'info' }
    // A settled attempt that established nothing (every source needs a role)
    // is announced for when it settled, never as a first read still pending.
    const when = clockOf(read.lastReadAt ?? read.settledAt ?? 0)
    const needsRole = read.products.filter((p) => p.state === 'needsRole').length
    if (needsRole > 0) return { text: READ_COPY.finishedNeedsRole(when, needsRole), tone: 'flagged' }
    if (read.sourcesMissing.length > 0) return { text: READ_COPY.finishedIncomplete(when, read.sourcesMissing.length), tone: 'flagged' }
    // Expired coverage stays distinct from healthy-empty (C12): the cached read is kept, its age named.
    if (read.lastReadAt !== null && !snapshot.overview.fresh) return { text: READ_COPY.stale(when), tone: 'flagged' }
    const activity = snapshot.overview.read?.summary.sessions
    if (activity === 0 && snapshot.inbox.items.length === 0) return { text: READ_COPY.finishedNoActivity(when, tenant), tone: 'allowed' }
    return { text: READ_COPY.finished(when), tone: 'allowed' }
}

/**
 * The one line under the greeting (R2App `homeSub`): the product count, the
 * core's read-status line (I4: pending, reading, finished with its coverage,
 * or failed, exactly as `readLine` says it) and the needs-action count.
 */
export function homeSub(snapshot: ShellSnapshot, tenant: string, products: number, needing: number): string {
    const parts = [HOME_COPY.sub.products(products), readLine(snapshot, tenant).text]
    if (needing > 0) parts.push(HOME_COPY.sub.needAction(needing))
    return parts.join(' · ')
}

type Pill = { word: string; bg: string; fg: string; glyph: GlyphName }
const PILLS: Record<'ok' | 'partial' | 'role' | 'pending' | 'failed' | 'gated', Pill> = {
    ok: { word: HOME_COPY.pill.connected, bg: 'var(--v2-ok-bg)', fg: 'var(--v2-ok)', glyph: 'check' },
    partial: { word: HOME_COPY.pill.partial, bg: 'var(--v2-flag-bg)', fg: 'var(--v2-flag)', glyph: 'alert' },
    role: { word: HOME_COPY.pill.needsRole, bg: 'var(--v2-flag-bg)', fg: 'var(--v2-flag)', glyph: 'alert' },
    pending: { word: HOME_COPY.pill.connecting, bg: 'var(--v2-inset)', fg: 'var(--v2-soft)', glyph: 'clock' },
    failed: { word: HOME_COPY.pill.failed, bg: 'var(--v2-block-bg)', fg: 'var(--v2-block)', glyph: 'ban' },
    gated: { word: HOME_COPY.pill.unavailable, bg: 'var(--v2-inset)', fg: 'var(--v2-faint)', glyph: 'lock' },
}

interface Card {
    id: AdminProductId
    /** The read can be asked for again (a role granted, a failure, partial coverage): the C30 recheck. */
    recheck: ProductId | null
    name: string
    pill: Pill
    value: string
    valueColor: string
    metric: string
    spark: { h: number; hot: boolean }[]
    sub: string
    subColor: string
    subGlyph: GlyphName
    isNew: boolean
    gated: boolean
}

/** The products the read reports on (the inventory ids with an admitted service). */
const readIdOf = (id: AdminProductId): ProductId | null => (id === 'agenticIdentity' || id === 'agenticEndpoint' ? null : id)

const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`
const count = (n: number) => n.toLocaleString('en-US')

/**
 * One card per Administration product (R2App `cards`), from the core only:
 * Runtime's headline is the read's detection events over its chart; the
 * Gateway's is the chosen workspace's day; Red Teaming and model/skill scans
 * carry their read state (the core publishes no figures for them); the two
 * gated products carry their gate. Nothing scripted is drawn.
 */
export function cardOf(snapshot: ShellSnapshot, product: AdminProduct, attention: InboxEntry[]): Card {
    const readId = readIdOf(product.id)
    const read = snapshot.inbox.read
    const state: ProductRead | undefined = readId ? read.products.find((p) => p.product === readId) : undefined
    const recheck = readId && state && (state.state === 'needsRole' || state.state === 'failed' || state.state === 'incomplete') ? readId : null
    const base = { id: product.id, name: product.label, isNew: product.availability.state === 'unavailable', gated: product.availability.state === 'unavailable', recheck }
    const blank = (pill: Pill, sub: string, subColor = 'var(--v2-faint)', subGlyph: GlyphName = 'clock'): Card => ({ ...base, pill, value: '—', valueColor: 'var(--v2-faint)', metric: HOME_COPY.metric[product.id] ?? '', spark: [], sub, subColor, subGlyph })
    if (product.availability.state === 'unavailable') return blank(PILLS.gated, product.availability.reason, 'var(--v2-faint)', 'lock')
    if (!state || state.state === 'pending' || state.state === 'notRead') return blank(PILLS.pending, read.state === 'reading' ? HOME_COPY.reading : HOME_COPY.firstRead)
    if (state.state === 'needsRole') return blank(PILLS.role, HOME_COPY.cardNeedsRole(state.detail), 'var(--v2-flag)', 'alert')
    if (state.state === 'failed') return blank(PILLS.failed, state.detail ?? READ_COPY.productState.failed ?? 'Failed', 'var(--v2-block)', 'ban')
    const pill = state.state === 'incomplete' ? PILLS.partial : PILLS.ok
    const worst = attention.find((e) => e.product === readId)
    const overview = snapshot.overview.read
    // The worst attention item is the last line when there is one; else the line states what the
    // read established, and names what it could not (R1): unknown is never an all-clear.
    const worstLine = worst ? { sub: worst.title, subColor: worst.severity === 'bad' ? 'var(--v2-block)' : 'var(--v2-flag)', subGlyph: (worst.severity === 'bad' ? 'ban' : 'alert') as GlyphName } : null
    if (product.id === 'runtimeSecurity' && overview) {
        const events = overview.summary.detectionEvents
        const buckets = overview.chart?.buckets ?? []
        const max = Math.max(1, ...buckets.map((b) => b.violating))
        const spark = buckets.slice(-12).map((b) => ({ h: Math.max(2, Math.round((b.violating / max) * 38)), hot: b.violating > 0 }))
        const violating = overview.summary.violating
        const line =
            worstLine ??
            (overview.chart === null || overview.chartFailure !== null
                ? { sub: HOME_COPY.chartNotRead, subColor: 'var(--v2-flag)', subGlyph: 'alert' as GlyphName }
                : violating === null
                  ? { sub: HOME_COPY.activityUnknown, subColor: 'var(--v2-flag)', subGlyph: 'alert' as GlyphName }
                  : violating > 0
                    ? { sub: HOME_COPY.runtimeViolating(violating), subColor: 'var(--v2-soft)', subGlyph: 'check' as GlyphName }
                    : { sub: HOME_COPY.noDetections, subColor: 'var(--v2-soft)', subGlyph: 'check' as GlyphName })
        return {
            ...base,
            pill,
            value: events === null ? '—' : count(events),
            valueColor: events !== null && events > 0 ? 'var(--v2-block)' : 'var(--v2-ink)',
            metric: HOME_COPY.metric.runtimeSecurity,
            spark,
            ...line,
        }
    }
    if (product.id === 'gateway') {
        const g = overview?.gateway ?? null
        if (!g) {
            // No workspace chosen (or no read yet): no day to draw, and the gate said (R2).
            return { ...base, pill, value: '—', valueColor: 'var(--v2-faint)', metric: HOME_COPY.metric.gateway, spark: [], ...(worstLine ?? { sub: HOME_COPY.gatewayNoWorkspace, subColor: 'var(--v2-faint)', subGlyph: 'route' as GlyphName }) }
        }
        const requests = g.requests
        const errors = g.errors
        const rate = requests !== null && errors !== null && requests > 0 ? (errors / requests) * 100 : null
        const line =
            worstLine ??
            (g.usageFailure !== null || requests === null
                ? { sub: HOME_COPY.usageNotRead(g.workspaceName), subColor: 'var(--v2-flag)', subGlyph: 'alert' as GlyphName }
                : errors === null
                  ? { sub: HOME_COPY.errorsUnknown(g.workspaceName), subColor: 'var(--v2-flag)', subGlyph: 'alert' as GlyphName }
                  : rate === null
                    ? { sub: HOME_COPY.gatewayNoRequests(g.workspaceName), subColor: 'var(--v2-soft)', subGlyph: 'check' as GlyphName }
                    : { sub: HOME_COPY.gatewayErrorRate(rate), subColor: rate > 5 ? 'var(--v2-block)' : 'var(--v2-soft)', subGlyph: (rate > 5 ? 'ban' : 'check') as GlyphName })
        return {
            ...base,
            pill: g.partial ? PILLS.partial : pill,
            value: requests === null ? '—' : count(requests),
            valueColor: 'var(--v2-ink)',
            metric: g.cost === null ? HOME_COPY.metric.gateway : `${HOME_COPY.metric.gateway} · ${dollars(g.cost)}`,
            spark: [],
            ...line,
        }
    }
    // Read-state cards: the read's coverage is the figure the core has.
    return {
        ...base,
        pill,
        value: state.sourcesTotal > 0 ? `${state.sourcesRead}/${state.sourcesTotal}` : '—',
        valueColor: 'var(--v2-ink)',
        metric: HOME_COPY.metric[product.id] ?? READ_COPY.sources(state.sourcesRead, state.sourcesTotal),
        spark: [],
        sub: worst ? worst.title : state.detail ?? (state.state === 'incomplete' ? HOME_COPY.partialRead(state.sourcesRead, state.sourcesTotal) : HOME_COPY.readOnly),
        subColor: worst ? (worst.severity === 'bad' ? 'var(--v2-block)' : 'var(--v2-flag)') : state.state === 'incomplete' ? 'var(--v2-flag)' : 'var(--v2-soft)',
        subGlyph: worst ? (worst.severity === 'bad' ? 'ban' : 'alert') : state.state === 'incomplete' ? 'alert' : 'check',
    }
}

const severityTile = (e: InboxEntry): { glyph: GlyphName; fg: string; bg: string } =>
    e.severity === 'bad' ? { glyph: 'ban', fg: 'var(--v2-block)', bg: 'var(--v2-block-bg)' } : e.severity === 'warn' ? { glyph: 'alert', fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)' } : { glyph: 'info', fg: 'var(--v2-soft)', bg: 'var(--v2-inset)' }

/**
 * Home as the prototype draws it (R2App start=home, C36): the greeting row
 * with the read summary and Refresh, the Explain banner while the calls are
 * shown, the six product cards, then Needs attention beside Quick actions.
 * Every figure reads the snapshot; every action is a core intent whose
 * outcome reaches the shell's surfaces (DESIGN-V2 §7).
 */
export const Home = forwardRef<HTMLHeadingElement, HomeProps>(function Home({ snapshot, onAct, onNavigate }, ref) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const context = snapshot.active?.context ?? null
    const read = snapshot.inbox.read
    const products = snapshot.administration?.products ?? []
    const attention = snapshot.inbox.items.filter((e) => e.needsAction && !e.hold)
    const shown = attention.slice(0, 5)
    const needsRole = read.products.find((p) => p.state === 'needsRole') ?? null
    const account = snapshot.harness.signIn?.account && snapshot.harness.signIn.failure === null ? snapshot.harness.signIn.account : null
    const readNow = () => context && tenant && void onAct(SHELL_COPY.readOf(tenant.label), { kind: 'readTenant', context })
    const hold = (kind: 'acknowledgeItem' | 'snoozeItem', id: string, title: string) => context && void onAct(holdWhat(kind, title), { kind, context, id })
    // A recheck re-reads one product (C30); its admission is announced for the product.
    const recheck = async (product: ProductId) => {
        if (!context || !tenant) return
        const label = READ_COPY.products[product] ?? product
        const outcome = await onAct(SHELL_COPY.recheckOf(label, tenant.label), { kind: 'recheckProduct', context, product })
        if (outcome.kind === 'admitted') useShellStore.getState().toast(READ_COPY.recheck(label), 'info', undefined, tenant.id, context)
    }
    const openProduct = (id: AdminProductId, tab?: string) => {
        if (tenant) useShellStore.getState().arrive({ tenant: tenant.id, feature: 'administration', resource: null, item: null, product: id, tab })
        onNavigate('administration')
    }
    const quick: { label: string; sub: string; key: string; glyph: GlyphName; go: () => void }[] = [
        { label: HOME_COPY.quick.prompt, sub: HOME_COPY.quick.promptSub, key: '', glyph: 'play', go: () => openProduct('runtimeSecurity', 'playground') },
        { label: HOME_COPY.quick.harness, sub: HOME_COPY.quick.harnessSub, key: '⌘3', glyph: 'bot', go: () => onNavigate('harness') },
        { label: HOME_COPY.quick.request, sub: HOME_COPY.quick.requestSub, key: '⌘6', glyph: 'send', go: () => onNavigate('apiClient') },
        { label: HOME_COPY.quick.learn, sub: HOME_COPY.quick.learnSub, key: '⌘4', glyph: 'cap', go: () => onNavigate('academy') },
    ]
    const button32 = 'flex h-8 min-h-0 items-center rounded-[7px] border text-[12px] whitespace-nowrap'
    return (
        <section aria-labelledby='home-heading' data-slot='home' className='flex flex-col' style={{ gap: 20 }}>
            <div className='flex items-end' style={{ gap: 12 }}>
                <div className='flex flex-col' style={{ gap: 2 }}>
                    <h1 ref={ref} id='home-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)', letterSpacing: '-0.01em' }}>
                        {account ? HOME_COPY.greeting(account) : HOME_COPY.title}
                    </h1>
                    <p role='status' aria-live='polite' data-slot='read-status' data-tone={readLine(snapshot, tenant?.label ?? null).tone} className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                        {tenant ? homeSub(snapshot, tenant.label, products.length, attention.length) : HOME_COPY.noTenant}
                    </p>
                </div>
                {context && (
                    <button type='button' aria-live='polite' onClick={readNow} disabled={read.state === 'reading'} className={`${button32} ml-auto px-3.5 text-[13px] disabled:opacity-60`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                        {read.state === 'reading' ? HOME_COPY.reading : HOME_COPY.refresh}
                    </button>
                )}
            </div>
            {snapshot.showCalls && (
                <div data-slot='home-explain' className='flex gap-2 rounded-[10px] text-[13px] leading-[1.45]' style={{ padding: '12px 14px', background: 'var(--v2-acc-soft)', maxWidth: 860 }}>
                    <Glyph name='info' size={15} stroke={2} color='var(--v2-acc)' className='mt-px' />
                    <span>{HOME_COPY.explain}</span>
                </div>
            )}
            {/* Three columns at the board's width (each card at least 300px); fewer when the page is narrower (the collapsed rail, 200% zoom). */}
            {tenant && products.length > 0 && (
                <ul data-slot='cards' aria-label={HOME_COPY.cards} className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 300px), 1fr))', gap: 16 }}>
                    {products.map((product) => {
                        const c = cardOf(snapshot, product, attention)
                        return (
                            // The card is one button (the prototype's); a Recheck control, when the read allows one,
                            // sits beside the card's last line above the stretched button, never inside it.
                            <li key={c.id} className='relative flex'>
                                <button
                                    type='button'
                                    data-slot='card'
                                    data-product={c.id}
                                    aria-label={`${c.name}: ${c.pill.word}, ${c.value} ${c.metric}, ${c.sub}${c.isNew ? ` · ${SHELL_COPY.newTag}` : ''}`}
                                    onClick={() => openProduct(c.id)}
                                    className='flex w-full flex-col rounded-[12px] border text-left after:absolute after:inset-0 after:rounded-[12px] after:content-[""]'
                                    style={{ gap: 10, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}
                                >
                                    <span className='flex w-full items-center' style={{ gap: 9 }}>
                                        <span className='grid size-7 shrink-0 place-items-center rounded-[8px]' style={{ background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}>
                                            <Glyph name={PRODUCT_GLYPHS[c.id]} size={15} />
                                        </span>
                                        <b className='min-w-0 flex-grow truncate text-[13px]'>{c.name}</b>
                                        <span className='flex shrink-0 items-center gap-1 rounded-full text-[12px] font-semibold whitespace-nowrap' style={{ padding: '2px 8px', background: c.pill.bg, color: c.pill.fg }}>
                                            <Glyph name={c.pill.glyph} size={10} stroke={2.6} />
                                            {c.pill.word}
                                        </span>
                                    </span>
                                    <span className='flex w-full items-end' style={{ gap: 12 }}>
                                        <span className='flex min-w-0 flex-1 flex-col'>
                                            <span className='text-[26px] font-bold tabular-nums' style={{ letterSpacing: '-0.02em', color: c.valueColor }}>
                                                {c.value}
                                            </span>
                                            <span className='text-[12px] leading-[1.35]' style={{ color: 'var(--v2-faint)' }}>
                                                {c.metric}
                                            </span>
                                        </span>
                                        <span aria-hidden='true' className='ml-auto flex h-[38px] w-[110px] shrink-0 items-end' style={{ gap: 3 }}>
                                            {c.spark.map((b, i) => (
                                                <span key={i} className='flex-1' style={{ height: b.h, borderRadius: '2px 2px 0 0', background: b.hot ? 'var(--v2-block)' : 'var(--v2-acc)' }} />
                                            ))}
                                        </span>
                                    </span>
                                    <span className='flex w-full items-center gap-1.5 text-[12px] font-semibold' style={{ paddingTop: 8, borderTop: '1px solid var(--v2-rule)', color: c.subColor }}>
                                        <Glyph name={c.subGlyph} size={12} stroke={2.2} />
                                        <span className='min-w-0 flex-grow truncate'>{c.sub}</span>
                                        {c.isNew && (
                                            <span className='shrink-0 rounded-full text-[12px] font-bold' style={{ padding: '1px 6px', background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)' }}>
                                                {SHELL_COPY.newTag}
                                            </span>
                                        )}
                                    </span>
                                </button>
                                {c.recheck && context && (
                                    <button
                                        type='button'
                                        onClick={() => void recheck(c.recheck!)}
                                        disabled={read.state === 'reading'}
                                        className='absolute right-5 bottom-[14px] z-10 h-7 rounded-[7px] border px-2.5 text-[12px] disabled:opacity-60'
                                        style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}
                                    >
                                        {HOME_COPY.recheck}
                                    </button>
                                )}
                            </li>
                        )
                    })}
                </ul>
            )}
            <div data-slot='home-lower' className='grid' style={{ gridTemplateColumns: 'minmax(0, 1.75fr) minmax(0, 1fr)', gap: 16 }}>
                <section aria-label={HOME_COPY.attention} data-slot='attention-section' className='flex flex-col rounded-[12px] border' style={{ padding: '4px 0', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                    <div className='flex items-center' style={{ padding: '16px 18px 10px' }}>
                        <h2 className='text-[13px] font-bold'>
                            {HOME_COPY.attention}{' '}
                            <span className='font-normal' style={{ color: 'var(--v2-faint)' }}>
                                {attention.length > 0 ? attention.length : ''}
                            </span>
                        </h2>
                        <button type='button' onClick={() => onNavigate('inbox')} className='ml-auto text-[12px] font-semibold' style={{ color: 'var(--v2-acc)' }}>
                            {HOME_COPY.openInbox}
                        </button>
                    </div>
                    {shown.length === 0 ? (
                        <div className='flex flex-col items-center text-center leading-[1.5]' style={{ gap: 12, padding: '28px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }}>
                            <span>{attentionEmpty(snapshot)}</span>
                            {needsRole && (
                                <button type='button' onClick={() => openProduct(needsRole.product)} className={`${button32} px-3.5 font-semibold`} style={{ borderColor: 'var(--v2-flag)', background: 'var(--v2-flag-bg)', color: 'var(--v2-flag)' }}>
                                    {HOME_COPY.openRoleIssue}
                                </button>
                            )}
                        </div>
                    ) : (
                        <ul data-slot='attention' className='flex flex-col'>
                            {shown.map((e) => {
                                const tile = severityTile(e)
                                return (
                                    // One line at the board's width; at narrow widths (the collapsed rail, 200% zoom) the action
                                    // group wraps under the text, which keeps at least 200px, and the controls keep their 32px.
                                    <li key={e.id} className='flex flex-wrap items-center' style={{ gap: 12, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)' }}>
                                        <span aria-hidden='true' className='grid size-7 shrink-0 place-items-center rounded-[8px]' style={{ background: tile.bg, color: tile.fg }}>
                                            <Glyph name={tile.glyph} size={14} stroke={2} />
                                        </span>
                                        <div className='min-w-0 flex-grow' style={{ flexBasis: 200 }}>
                                            <div className='font-semibold [overflow-wrap:anywhere]'>{e.title}</div>
                                            {/* The severity is a word as well as a colour and an icon (I9). */}
                                            {/* The line wraps rather than clips (nothing is cut off at narrow widths or 200% zoom). */}
                                            <div className='text-[12px] [overflow-wrap:anywhere]' style={{ color: 'var(--v2-faint)' }}>
                                                <span data-slot='severity' className='font-semibold' style={{ color: tile.fg }}>
                                                    {INBOX_COPY.severity[e.severity]}
                                                </span>
                                                {' · '}
                                                {[e.product ? READ_COPY.products[e.product] : null, e.source, e.detail].filter(Boolean).join(' · ')}
                                            </div>
                                        </div>
                                        <div className='flex shrink-0 items-center' style={{ gap: 12 }}>
                                            {destinationFeature(e) && e.actionLabel && tenant && (
                                                <button type='button' onClick={() => follow(e, tenant.id, onNavigate)} className={`${button32} shrink-0 px-3`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                                                    {e.actionLabel}
                                                </button>
                                            )}
                                            <button type='button' aria-label={`${HOME_COPY.acknowledge}: ${e.title}`} title={HOME_COPY.acknowledge} onClick={() => hold('acknowledgeItem', e.id, e.title)} className='grid size-8 min-h-0 shrink-0 place-items-center rounded-[7px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                                                <Glyph name='check' size={12} stroke={2.2} />
                                            </button>
                                            <button type='button' aria-label={`${SNOOZE_COPY.offer}: ${e.title}`} title={SNOOZE_COPY.offer} onClick={() => hold('snoozeItem', e.id, e.title)} className='grid size-8 min-h-0 shrink-0 place-items-center rounded-[7px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                                                <Glyph name='clock' size={12} stroke={2} />
                                            </button>
                                        </div>
                                    </li>
                                )
                            })}
                        </ul>
                    )}
                </section>
                <section aria-label={HOME_COPY.quickActions} data-slot='quick-actions' className='flex flex-col rounded-[12px] border' style={{ padding: '4px 0', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                    <h2 className='text-[13px] font-bold' style={{ padding: '16px 18px 10px' }}>
                        {HOME_COPY.quickActions}
                    </h2>
                    {quick.map((q) => (
                        <button key={q.label} type='button' data-inset-focus='' onClick={q.go} className='flex items-center text-left' style={{ gap: 12, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-ink)' }}>
                            <span aria-hidden='true' className='grid size-7 shrink-0 place-items-center rounded-[8px]' style={{ background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)' }}>
                                <Glyph name={q.glyph} size={14} stroke={1.9} />
                            </span>
                            <span className='flex-grow'>
                                <b className='font-semibold'>{q.label}</b>
                                <br />
                                <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                    {q.sub}
                                </span>
                            </span>
                            <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                {q.key}
                            </span>
                        </button>
                    ))}
                </section>
            </div>
        </section>
    )
})
