import { forwardRef, useEffect } from 'react'
import type { FeatureId, InboxEntry, Intent, ProductId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import type { DispatchOutcome } from '../../modules/tenants/dispatch'
import { INBOX_COPY as C, READ_COPY, SHELL_COPY, SNOOZE_COPY, clockOf } from '../copy'
import { Glyph, type GlyphName } from '../icons'
import { useShellStore } from '../store'
import type { VerdictTone } from '../verdict'
import { PageIntro } from '../page-intro'

/**
 * Sends one intent and reports its outcome on the shell's surfaces (DESIGN-V2
 * §7): a refusal or failed delivery as a warn toast naming `what`, an
 * unconfirmed delivery inline with Retry (never replayed on its own).
 */
export type Act = (what: string, intent: Intent) => Promise<DispatchOutcome>

export interface InboxProps {
    snapshot: ShellSnapshot
    onAct: Act
    onNavigate: (feature: FeatureId) => void
}

export type InboxTab = 'all' | 'action' | 'updates' | 'snoozed'
const TABS: InboxTab[] = ['all', 'action', 'updates', 'snoozed']

export const toneOf = (severity: InboxEntry['severity']): VerdictTone => (severity === 'bad' ? 'blocked' : severity === 'warn' ? 'flagged' : 'info')

/** Where an entry's action leads (C30): the destination the core names. */
export function destinationFeature(entry: InboxEntry): FeatureId | null {
    return entry.target?.feature ?? null
}

/**
 * Follows an entry's contextual action: the target's inventory and item are
 * handed to the destination (it selects them on arrival), then the shell
 * navigates through the core.
 */
export function follow(entry: InboxEntry, tenant: string, onNavigate: (feature: FeatureId) => void) {
    const target = entry.target
    if (!target) return
    // The target belongs to the tenant it was followed in (I2): another tenant never inherits it.
    useShellStore.getState().arrive({ tenant, feature: target.feature, resource: target.resource, item: target.item })
    onNavigate(target.feature)
}

/** The tenant-named description of a hold action, for its outcome report. */
export function holdWhat(kind: 'acknowledgeItem' | 'snoozeItem' | 'unsnoozeItem', title: string): string {
    return kind === 'acknowledgeItem' ? SHELL_COPY.acknowledgmentOf(title) : kind === 'snoozeItem' ? SHELL_COPY.snoozeOf(title) : SHELL_COPY.unsnoozeOf(title)
}

/** Which tab an entry belongs to; held entries are never "needs action". */
export function tabOf(entry: InboxEntry): InboxTab {
    if (entry.hold?.kind === 'snooze') return 'snoozed'
    if (entry.needsAction && !entry.hold) return 'action'
    return 'updates'
}

const severityTile = (severity: InboxEntry['severity']): { glyph: GlyphName; fg: string; bg: string } =>
    severity === 'bad' ? { glyph: 'ban', fg: 'var(--v2-block)', bg: 'var(--v2-block-bg)' } : severity === 'warn' ? { glyph: 'alert', fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)' } : { glyph: 'info', fg: 'var(--v2-soft)', bg: 'var(--v2-inset)' }

const button32 = 'flex h-8 min-h-0 items-center rounded-[7px] text-[13px] whitespace-nowrap'

/**
 * The Inbox (C30, screens/inbox-reads.md): the active tenant's entries from
 * the core's Inbox scope, in tabs with counts, a detail pane with the facts,
 * the explanation and the contextual actions. Acknowledge and snooze are
 * core intents on the entry; the snooze return time is the core's and the
 * label is the one Home uses (I11). The selected tab and item are kept per
 * tenant across a switch (I2) and cleared when the tenant is deleted.
 */
export const Inbox = forwardRef<HTMLHeadingElement, InboxProps>(function Inbox({ snapshot, onAct, onNavigate }, _ref) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const context = snapshot.active?.context ?? null
    const tenantKey = tenant?.id ?? 'none'
    const selection = useShellStore((s) => s.inboxSelection[tenantKey])
    const selectInbox = useShellStore((s) => s.selectInbox)
    const tab: InboxTab = selection?.tab ?? 'all'
    const selectedId = selection?.item ?? null
    const items = snapshot.inbox.items
    const counts: Record<InboxTab, number> = { all: items.length, action: 0, updates: 0, snoozed: 0 }
    for (const e of items) counts[tabOf(e)] += 1
    const visible = items.filter((e) => tab === 'all' || tabOf(e) === tab)
    const selected = items.find((e) => e.id === selectedId) ?? null
    useEffect(() => {
        if (selectedId && !items.some((e) => e.id === selectedId)) selectInbox(tenantKey, { tab, item: null })
    }, [items, selectedId, selectInbox, tenantKey, tab])
    const read = snapshot.inbox.read

    // Holds are core intents; their outcomes reach the shell's surfaces. A
    // snooze's return time is the core's: the shell announces it from the
    // receipt the snapshot carries (the same toast whether asked here or on Home).
    const act = (entry: InboxEntry, kind: 'acknowledgeItem' | 'snoozeItem' | 'unsnoozeItem') => context && void onAct(holdWhat(kind, entry.title), { kind, context, id: entry.id })
    const readNow = () => context && tenant && void onAct(SHELL_COPY.readOf(tenant.label), { kind: 'readTenant', context })

    // Never attempted, in progress, failed, partial, expired and healthy-empty
    // reads stay distinct (I4, C12). Entries the core supplies (a setup
    // warning before any read) are listed whatever the read status.
    const failedUnread = read.state === 'failed' && read.lastReadAt === null && read.failedAt !== null
    const stale = (read.state === 'done' || read.state === 'reading') && read.lastReadAt !== null && !snapshot.overview.fresh
    const readingAgain = read.state === 'reading' && read.lastReadAt !== null
    const empty = !tenant
        ? C.emptyPending
        : items.length === 0 && read.state === 'reading'
          ? C.emptyReading
          : items.length === 0 && read.state === 'pending'
            ? C.emptyPending
            : items.length === 0 && failedUnread
              ? C.emptyFailed(clockOf(read.failedAt ?? 0))
              : items.length === 0 && read.lastReadAt === null && read.settledAt !== null
                ? C.emptyUnread(clockOf(read.settledAt), read.sourcesMissing.length)
                : visible.length === 0
                  ? tab === 'all' || tab === 'action'
                      ? read.sourcesMissing.length > 0
                          ? C.emptyPartial(read.products.reduce((n, p) => n + p.sourcesRead, 0), read.products.reduce((n, p) => n + p.sourcesTotal, 0), read.sourcesMissing)
                          : stale
                            ? C.emptyStale(clockOf(read.lastReadAt ?? 0))
                            : C.emptyClear
                      : C.emptyTab
                  : null
    const partialLine =
        read.state === 'failed' && read.failedAt !== null && read.lastReadAt !== null
            ? C.failedKept(clockOf(read.failedAt), clockOf(read.lastReadAt))
            : (read.lastReadAt ?? read.settledAt) !== null && read.sourcesMissing.length > 0
              ? C.partial(read.products.reduce((n, p) => n + p.sourcesRead, 0), read.products.reduce((n, p) => n + p.sourcesTotal, 0), clockOf(read.lastReadAt ?? read.settledAt ?? 0))
              : readingAgain
                ? stale
                    ? C.readingStale(clockOf(read.lastReadAt ?? 0))
                    : C.readingKept(clockOf(read.lastReadAt ?? 0))
                : stale
                  ? C.stale(clockOf(read.lastReadAt ?? 0))
                  : null

    // The empty states' role issue, the footer's coverage lines and the product hand-off (C37, R2App).
    const needsRole = read.products.find((p) => p.state === 'needsRole') ?? null
    const openProduct = (id: ProductId) => {
        if (tenant) useShellStore.getState().arrive({ tenant: tenant.id, feature: 'administration', resource: null, item: null, product: id })
        onNavigate('administration')
    }
    const sourcesRead = read.products.reduce((n, p) => n + p.sourcesRead, 0)
    const sourcesTotal = read.products.reduce((n, p) => n + p.sourcesTotal, 0)
    const readAt = read.lastReadAt ?? read.settledAt
    const footLine1 = !tenant || readAt === null ? C.footNoReads(tenant?.label ?? '') : read.sourcesMissing.length > 0 ? C.footPartial(sourcesRead, sourcesTotal, clockOf(readAt)) : C.footAll(clockOf(readAt))
    const gateway = snapshot.overview.read?.gateway ?? null
    const footLine2 = gateway ? C.footGateway(gateway.workspaceName, gateway.policies) : (snapshot.overview.read?.gaps[0] ?? '')

    return (
        <section aria-label={C.title} data-slot='inbox' className='flex min-h-0 flex-grow flex-col' style={{ gap: 20 }}>
            <PageIntro
                description={
                    <p role='status' data-slot='inbox-sub' className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                        {tenant ? C.sub(tenant.label) : C.emptyPending}
                        {partialLine ? ` · ${partialLine}` : ''}
                    </p>
                }
                actions={context && (
                    <button type='button' onClick={readNow} disabled={read.state === 'reading'} className={`${button32} ml-auto border px-3.5 disabled:opacity-60`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                        {read.state === 'reading' ? C.reading : 'Refresh'}
                    </button>
                )}
            />
            {/* The list column is the prototype's 430px beside the detail; one under the other when both cannot fit (200% zoom, a narrow window). */}
            <div data-slot='inbox-columns' className='grid min-h-0 min-w-0 flex-grow' style={{ gridTemplateColumns: 'minmax(0, 430px) minmax(0, 1fr)', gap: 20 }}>
                <section aria-label={C.listCard} data-slot='inbox-list' className='flex min-h-0 min-w-0 flex-col overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                    <div className='flex items-center' style={{ padding: '14px 18px', gap: 10, borderBottom: '1px solid var(--v2-rule)' }}>
                        <div role='tablist' aria-label={C.filter} className='flex rounded-[8px]' style={{ padding: 2, background: 'var(--v2-inset)' }}>
                            {TABS.map((t) => (
                                <button
                                    key={t}
                                    role='tab'
                                    type='button'
                                    id={`itab-${t}`}
                                    aria-controls='inbox-panel'
                                    aria-selected={tab === t}
                                    onClick={() => selectInbox(tenantKey, { tab: t, item: selectedId })}
                                    className='h-7 min-h-0 rounded-[6px] px-3 text-[12px] font-semibold'
                                    style={{ background: tab === t ? 'var(--v2-paper)' : 'transparent', boxShadow: tab === t ? '0 1px 2px rgba(0,0,0,0.12)' : 'none', color: 'var(--v2-ink)' }}
                                >
                                    {C.tabs[t]}{' '}
                                    <span className='font-medium' style={{ color: 'var(--v2-faint)' }}>
                                        {counts[t]}
                                    </span>
                                </button>
                            ))}
                        </div>
                    </div>
                    <div role='tabpanel' id='inbox-panel' aria-labelledby={`itab-${tab}`} className='min-h-0 flex-grow overflow-auto'>
                        {empty ? (
                            <div role='status' data-slot='inbox-empty' className='flex flex-col items-center text-center leading-[1.5]' style={{ gap: 12, padding: '32px 18px', color: 'var(--v2-faint)' }}>
                                <span>{empty}</span>
                                {needsRole && context && (
                                    <button type='button' onClick={() => openProduct(needsRole.product)} className={`${button32} border px-3.5 font-semibold`} style={{ borderColor: 'var(--v2-flag)', background: 'var(--v2-flag-bg)', color: 'var(--v2-flag)' }}>
                                        {C.openRoleIssue}
                                    </button>
                                )}
                            </div>
                        ) : (
                            <ul aria-label={C.ariaList} className='flex flex-col'>
                                {visible.map((e) => {
                                    const tile = severityTile(e.severity)
                                    const current = selected?.id === e.id
                                    return (
                                        <li key={e.id}>
                                            <button
                                                type='button'
                                                aria-current={current ? 'true' : undefined}
                                                data-inset-focus=''
                                                onClick={() => selectInbox(tenantKey, { tab, item: e.id })}
                                                className='flex w-full min-w-0 items-start text-left'
                                                style={{ gap: 10, padding: '14px 18px', borderBottom: '1px solid var(--v2-rule)', background: current ? 'var(--v2-inset)' : 'transparent', boxShadow: current ? 'inset 3px 0 0 var(--v2-acc)' : 'none', color: 'var(--v2-ink)' }}
                                            >
                                                <span aria-hidden='true' className='grid size-[26px] shrink-0 place-items-center rounded-[7px]' style={{ background: tile.bg, color: tile.fg }}>
                                                    <Glyph name={tile.glyph} size={13} stroke={2} />
                                                </span>
                                                <span className='min-w-0 flex-grow'>
                                                    <span className='block font-semibold [overflow-wrap:anywhere]'>{e.title}</span>
                                                    <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                        {/* The severity is a word as well as a colour and an icon (I9). */}
                                                        <span data-slot='severity' className='font-semibold' style={{ color: tile.fg }}>
                                                            {C.severity[e.severity]}
                                                        </span>
                                                        {' · '}
                                                        {e.product ? READ_COPY.products[e.product] : C.kind[e.kind]} · {e.source}
                                                        {e.hold?.kind === 'snooze' && e.hold.returnsAt !== null ? ` · ${SNOOZE_COPY.returns(e.hold.returnsAt)}` : ''}
                                                    </span>
                                                    {e.hold?.kind === 'acknowledge' && (
                                                        <span className='ml-2 inline-flex items-center gap-1 text-[12px] font-semibold' style={{ color: 'var(--v2-ok)' }}>
                                                            <Glyph name='check' size={11} stroke={2.6} />
                                                            {C.acknowledged}
                                                        </span>
                                                    )}
                                                </span>
                                                <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                    {clockOf(e.when)}
                                                </span>
                                            </button>
                                        </li>
                                    )
                                })}
                            </ul>
                        )}
                    </div>
                    <div data-slot='inbox-foot' className='text-[12px] leading-[1.6]' style={{ padding: '14px 18px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-faint)' }}>
                        <span>{footLine1}</span>
                        <br />
                        <span>{footLine2}</span>
                    </div>
                </section>
                <aside aria-label={C.detail} data-slot='inbox-detail' className='flex min-w-0 flex-col rounded-[12px] border break-words' style={{ padding: '28px 30px', gap: 18, borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                    {!selected ? (
                        <div className='flex flex-grow flex-col items-center justify-center text-center leading-[1.5]' style={{ gap: 12, color: 'var(--v2-faint)' }}>
                            <span>{empty ?? C.selectHint}</span>
                        </div>
                    ) : (
                        <>
                            <div className='flex items-center' style={{ gap: 10 }}>
                                <span aria-hidden='true' className='grid size-[34px] place-items-center rounded-[9px]' style={{ background: severityTile(selected.severity).bg, color: severityTile(selected.severity).fg }}>
                                    <Glyph name={severityTile(selected.severity).glyph} size={17} stroke={2} />
                                </span>
                                <span className='rounded-full text-[12px] font-semibold' style={{ padding: '2px 9px', background: severityTile(selected.severity).bg, color: severityTile(selected.severity).fg }}>
                                    {C.severity[selected.severity]}
                                </span>
                                <span style={{ color: 'var(--v2-faint)' }}>
                                    {selected.product ? READ_COPY.products[selected.product] : C.kind[selected.kind]} · {selected.source}
                                </span>
                            </div>
                            <h2 className='min-w-0 text-[22px] font-bold' style={{ letterSpacing: '-0.01em' }}>
                                {selected.title}
                            </h2>
                            <p className='text-[14px] leading-[1.55]' style={{ color: 'var(--v2-soft)' }}>
                                {selected.detail}
                            </p>
                            {selected.facts.length > 0 && (
                                <dl aria-label={C.facts} className='grid' style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 10 }}>
                                    {selected.facts.map((f) => (
                                        <div key={f.label} className='rounded-[10px]' style={{ padding: '12px 14px', background: 'var(--v2-inset)' }}>
                                            <dt className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                {f.label}
                                            </dt>
                                            <dd className='font-semibold' style={{ marginTop: 2 }}>
                                                {f.value}
                                            </dd>
                                        </div>
                                    ))}
                                </dl>
                            )}
                            {snapshot.showCalls && (
                                <div data-slot='inbox-why' className='flex gap-2 rounded-[10px] text-[13px] leading-[1.45]' style={{ padding: '12px 14px', background: 'var(--v2-acc-soft)', maxWidth: 860 }}>
                                    <Glyph name='info' size={15} stroke={2} color='var(--v2-acc)' className='mt-px' />
                                    <span>
                                        <span className='font-semibold'>{C.why}: </span>
                                        {C.whyText[selected.kind]}
                                    </span>
                                </div>
                            )}
                            {selected.hold?.kind === 'snooze' && selected.hold.returnsAt !== null && (
                                <p data-slot='snooze-returns' className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                    {SNOOZE_COPY.returns(selected.hold.returnsAt)}
                                </p>
                            )}
                            <div className='flex flex-wrap' style={{ gap: 8, marginTop: 'auto' }}>
                                {destinationFeature(selected) && selected.actionLabel && (
                                    <button type='button' onClick={() => tenant && follow(selected, tenant.id, onNavigate)} className={`${button32} px-[18px] font-semibold`} style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                                        {selected.actionLabel}
                                    </button>
                                )}
                                {selected.hold?.kind === 'acknowledge' ? (
                                    <button type='button' aria-pressed='true' onClick={() => void act(selected, 'unsnoozeItem')} className={`${button32} border px-3.5`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                                        {C.needsAction}
                                    </button>
                                ) : (
                                    <button type='button' aria-pressed='false' onClick={() => void act(selected, 'acknowledgeItem')} className={`${button32} border px-3.5`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                                        {C.acknowledge}
                                    </button>
                                )}
                                {selected.hold?.kind === 'snooze' ? (
                                    <button type='button' onClick={() => void act(selected, 'unsnoozeItem')} className={`${button32} border px-3.5`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                                        {C.unsnooze}
                                    </button>
                                ) : (
                                    <button type='button' title={SNOOZE_COPY.offer} aria-label={`${SNOOZE_COPY.offer}: ${selected.title}`} onClick={() => void act(selected, 'snoozeItem')} className={`${button32} border px-3.5`} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                                        {SNOOZE_COPY.offer}
                                    </button>
                                )}
                            </div>
                        </>
                    )}
                </aside>
            </div>
        </section>
    )
})
