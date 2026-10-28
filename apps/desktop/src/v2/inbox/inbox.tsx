import { forwardRef, useEffect } from 'react'
import type { FeatureId, InboxEntry, Intent, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import type { DispatchOutcome } from '../../features/tenants/dispatch'
import { INBOX_COPY as C, READ_COPY, SHELL_COPY, SNOOZE_COPY, clockOf } from '../copy'
import { useShellStore } from '../store'
import { Verdict, type VerdictTone } from '../verdict'

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

/**
 * The Inbox (C30, screens/inbox-reads.md): the active tenant's entries from
 * the core's Inbox scope, in tabs with counts, a detail pane with the facts,
 * the explanation and the contextual actions. Acknowledge and snooze are
 * core intents on the entry; the snooze return time is the core's and the
 * label is the one Home uses (I11). The selected tab and item are kept per
 * tenant across a switch (I2) and cleared when the tenant is deleted.
 */
export const Inbox = forwardRef<HTMLHeadingElement, InboxProps>(function Inbox({ snapshot, onAct, onNavigate }, ref) {
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
                      ? stale
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

    return (
        <section aria-labelledby='inbox-heading' data-slot='inbox' className='flex min-h-0 flex-col gap-4'>
            <div className='flex items-end justify-between gap-4'>
                <div>
                    <h1 ref={ref} id='inbox-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                        {C.title}
                    </h1>
                    {partialLine && (
                        <p role='status' className='mt-1 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {partialLine}
                        </p>
                    )}
                </div>
                {context && (
                    <button type='button' onClick={readNow} disabled={read.state === 'reading'} className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }}>
                        {C.readNow}
                    </button>
                )}
            </div>
            <div role='tablist' aria-label={C.title} className='flex gap-1'>
                {TABS.map((t) => (
                    <button
                        key={t}
                        role='tab'
                        type='button'
                        aria-selected={tab === t}
                        onClick={() => selectInbox(tenantKey, { tab: t, item: selectedId })}
                        className='h-8 rounded-[7px] px-3 text-[13px]'
                        style={{ background: tab === t ? 'var(--v2-sel)' : 'transparent', color: tab === t ? 'var(--v2-sel-ink)' : 'var(--v2-ink)' }}
                    >
                        {C.tabs[t]} <span style={{ opacity: 0.7 }}>{counts[t]}</span>
                    </button>
                ))}
            </div>
            {/* Two columns while both fit; one under the other at 200% zoom or a narrow window (no horizontal clipping). */}
            <div data-slot='inbox-columns' className='grid min-h-0 min-w-0 gap-4' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 300px), 1fr))' }}>
                <div className='min-w-0'>
                    {empty ? (
                        <p role='status' data-slot='inbox-empty' className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {empty}
                        </p>
                    ) : (
                        <ul aria-label={C.ariaList} className='flex flex-col gap-1'>
                            {visible.map((e) => (
                                <li key={e.id}>
                                    <button
                                        type='button'
                                        aria-current={selected?.id === e.id ? 'true' : undefined}
                                        onClick={() => selectInbox(tenantKey, { tab, item: e.id })}
                                        className='flex w-full min-w-0 flex-col gap-1 rounded-[12px] border px-4 py-3 text-left break-words'
                                        style={{ borderColor: selected?.id === e.id ? 'var(--v2-acc)' : 'var(--v2-rule)', background: 'var(--v2-paper)' }}
                                    >
                                        <span className='flex flex-wrap items-center gap-2'>
                                            <Verdict tone={toneOf(e.severity)} word={C.severity[e.severity]} />
                                            <span className='min-w-0 text-[14px] font-medium'>{e.title}</span>
                                            {e.hold?.kind === 'acknowledge' && <span className='text-[11px]' style={{ color: 'var(--v2-faint)' }}>{C.acknowledged}</span>}
                                        </span>
                                        <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                            {e.product ? READ_COPY.products[e.product] : C.kind[e.kind]} · {e.source} · {clockOf(e.when)}
                                            {e.hold?.kind === 'snooze' && e.hold.returnsAt !== null ? ` · ${SNOOZE_COPY.returns(e.hold.returnsAt)}` : ''}
                                        </span>
                                    </button>
                                </li>
                            ))}
                        </ul>
                    )}
                </div>
                <aside aria-label={C.detail} data-slot='inbox-detail' className='min-w-0 rounded-[12px] border p-5 break-words' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                    {!selected ? (
                        <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {C.selectHint}
                        </p>
                    ) : (
                        <div className='flex flex-col gap-3'>
                            <div className='flex flex-wrap items-center gap-2'>
                                <Verdict tone={toneOf(selected.severity)} word={C.severity[selected.severity]} />
                                <h2 className='min-w-0 text-[16px] font-semibold'>{selected.title}</h2>
                            </div>
                            <p className='text-[13px]'>{selected.detail}</p>
                            {selected.facts.length > 0 && (
                                <table className='text-[12px]'>
                                    <caption className='sr-only'>{C.facts}</caption>
                                    <tbody>
                                        {selected.facts.map((f) => (
                                            <tr key={f.label}>
                                                <th scope='row' className='pr-3 text-left font-medium'>
                                                    {f.label}
                                                </th>
                                                <td>{f.value}</td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            )}
                            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                <span className='font-semibold'>{C.why}: </span>
                                {C.whyText[selected.kind]}
                            </p>
                            {selected.hold?.kind === 'snooze' && selected.hold.returnsAt !== null && (
                                <p data-slot='snooze-returns' className='text-[12px]'>
                                    {SNOOZE_COPY.returns(selected.hold.returnsAt)}
                                </p>
                            )}
                            <div className='flex flex-wrap gap-2'>
                                {destinationFeature(selected) && selected.actionLabel && (
                                    <button type='button' onClick={() => tenant && follow(selected, tenant.id, onNavigate)} className='h-8 rounded-[7px] px-3 text-[13px] font-semibold' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                                        {selected.actionLabel}
                                    </button>
                                )}
                                {selected.hold?.kind === 'acknowledge' ? (
                                    <button type='button' onClick={() => void act(selected, 'unsnoozeItem')} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                        {C.needsAction}
                                    </button>
                                ) : (
                                    <button type='button' onClick={() => void act(selected, 'acknowledgeItem')} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                        {C.acknowledge}
                                    </button>
                                )}
                                {selected.hold?.kind === 'snooze' ? (
                                    <button type='button' onClick={() => void act(selected, 'unsnoozeItem')} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                        {C.unsnooze}
                                    </button>
                                ) : (
                                    <button type='button' title={SNOOZE_COPY.offer} aria-label={`${SNOOZE_COPY.offer}: ${selected.title}`} onClick={() => void act(selected, 'snoozeItem')} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                        {SNOOZE_COPY.offer}
                                    </button>
                                )}
                            </div>
                        </div>
                    )}
                </aside>
            </div>
        </section>
    )
})
