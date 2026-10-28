import { forwardRef } from 'react'
import type { FeatureId, ProductId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { FEATURE_ICONS } from '../shell/icons'
import { ENVIRONMENT_LABELS, HOME_COPY, READ_COPY, SHELL_COPY, SNOOZE_COPY, clockOf } from './copy'
import { type Act, destinationFeature, follow, holdWhat, toneOf } from './inbox/inbox'
import { useShellStore } from './store'
import { Verdict } from './verdict'

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
 * Home (C28 skeleton, C30 attention and reads): the destination title row,
 * the tenant line, the one read-status line from the core's ReadStatus, up
 * to five items needing attention with their contextual action, Acknowledge
 * and Snooze (whose label is the Inbox's, I11), one card per chosen product
 * with its read state, and the destinations. Nothing here is invented:
 * every line reads the snapshot.
 */
export const Home = forwardRef<HTMLHeadingElement, HomeProps>(function Home({ snapshot, onAct, onNavigate }, ref) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const context = snapshot.active?.context ?? null
    const status = readLine(snapshot, tenant?.label ?? null)
    const attention = snapshot.inbox.items.filter((e) => e.needsAction && !e.hold).slice(0, 5)
    const read = snapshot.inbox.read
    // Every action here is a core intent whose outcome reaches the shell's
    // surfaces, named for the tenant or the item (DESIGN-V2 §7).
    const readNow = () => context && tenant && void onAct(SHELL_COPY.readOf(tenant.label), { kind: 'readTenant', context })
    const recheck = async (product: ProductId) => {
        if (!context || !tenant) return
        const label = READ_COPY.products[product] ?? product
        const outcome = await onAct(SHELL_COPY.recheckOf(label, tenant.label), { kind: 'recheckProduct', context, product })
        if (outcome.kind === 'admitted') useShellStore.getState().toast(READ_COPY.recheck(label), 'info', undefined, tenant.id, context)
    }
    const hold = (kind: 'acknowledgeItem' | 'snoozeItem', id: string, title: string) => context && void onAct(holdWhat(kind, title), { kind, context, id })
    return (
        <section aria-labelledby='home-heading' data-slot='home' style={{ maxWidth: 'var(--v2-reading)' }}>
            <div className='flex flex-col gap-1'>
                <h1 ref={ref} id='home-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {HOME_COPY.title}
                </h1>
                {tenant && (
                    <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                        {tenant.label} · {ENVIRONMENT_LABELS[tenant.environment]}
                        {tenant.tsgId ? ` · TSG ${tenant.tsgId}` : ''}
                    </p>
                )}
                <p role='status' aria-live='polite' data-slot='read-status' className='mt-1 flex items-center gap-3 text-[13px]'>
                    <Verdict tone={status.tone} word={status.text} />
                    {context && (
                        <button type='button' onClick={readNow} disabled={read.state === 'reading'} className='h-7 rounded-[7px] border px-2.5 text-[12px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }}>
                            {HOME_COPY.readNow}
                        </button>
                    )}
                </p>
            </div>
            <div className='mt-6 rounded-[12px] border p-5' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                <div className='flex items-center justify-between'>
                    <h2 className='text-[15px] font-semibold'>{HOME_COPY.attention}</h2>
                    <button type='button' onClick={() => onNavigate('inbox')} className='text-[12px] underline' style={{ color: 'var(--v2-acc)' }}>
                        {HOME_COPY.openInbox}
                    </button>
                </div>
                {attention.length === 0 ? (
                    <p className='mt-2 text-[13px]' style={{ color: 'var(--v2-faint)' }}>
                        {attentionEmpty(snapshot)}
                    </p>
                ) : (
                    <ul data-slot='attention' className='mt-3 flex flex-col gap-2'>
                        {attention.map((e) => (
                            <li key={e.id} className='flex flex-wrap items-center gap-2 rounded-[10px] border px-3 py-2' style={{ borderColor: 'var(--v2-rule)' }}>
                                <Verdict tone={toneOf(e.severity)} word={e.severity === 'bad' ? 'Critical' : e.severity === 'warn' ? 'Warning' : 'Info'} />
                                <span className='flex-grow text-[13px] font-medium'>{e.title}</span>
                                <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                    {e.product ? READ_COPY.products[e.product] : ''}
                                </span>
                                {destinationFeature(e) && e.actionLabel && tenant && (
                                    <button type='button' onClick={() => follow(e, tenant.id, onNavigate)} className='h-7 rounded-[7px] px-2.5 text-[12px] font-semibold' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                                        {e.actionLabel}
                                    </button>
                                )}
                                <button type='button' onClick={() => hold('acknowledgeItem', e.id, e.title)} className='h-7 rounded-[7px] border px-2.5 text-[12px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                    Acknowledge
                                </button>
                                <button type='button' title={SNOOZE_COPY.offer} aria-label={`${SNOOZE_COPY.offer}: ${e.title}`} onClick={() => hold('snoozeItem', e.id, e.title)} className='h-7 rounded-[7px] border px-2.5 text-[12px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                    {SNOOZE_COPY.offer}
                                </button>
                            </li>
                        ))}
                    </ul>
                )}
            </div>
            {tenant && read.products.length > 0 && (
                <div className='mt-6'>
                    <h2 className='text-[15px] font-semibold'>{HOME_COPY.products}</h2>
                    <ul data-slot='products' className='mt-3 grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}>
                        {read.products.map((p) => (
                            <li key={p.product} className='rounded-[12px] border p-4' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                                <p className='text-[13px] font-semibold'>{READ_COPY.products[p.product]}</p>
                                <p className='mt-2 flex flex-wrap items-center gap-2'>
                                    <Verdict tone={p.state === 'ok' ? 'allowed' : p.state === 'failed' ? 'blocked' : p.state === 'incomplete' || p.state === 'needsRole' ? 'flagged' : 'info'} word={READ_COPY.productState[p.state] ?? p.state} />
                                    <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                        {READ_COPY.sources(p.sourcesRead, p.sourcesTotal)}
                                    </span>
                                </p>
                                {p.detail && (
                                    <p className='mt-1 text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                        {p.detail}
                                    </p>
                                )}
                                {(p.state === 'needsRole' || p.state === 'failed' || p.state === 'incomplete') && context && (
                                    <button type='button' onClick={() => void recheck(p.product)} disabled={read.state === 'reading'} className='mt-2 h-7 rounded-[7px] border px-2.5 text-[12px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }}>
                                        Recheck
                                    </button>
                                )}
                            </li>
                        ))}
                    </ul>
                </div>
            )}
            <div className='mt-6'>
                <h2 className='text-[15px] font-semibold'>{HOME_COPY.destinations}</h2>
                <ul className='mt-3 grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))' }}>
                    {snapshot.features
                        .filter((f) => f.sidebar && f.id !== 'home')
                        .map((f) => {
                            const Icon = FEATURE_ICONS[f.id]
                            const unavailable = f.availability.state === 'unavailable'
                            return (
                                <li key={f.id}>
                                    <button
                                        type='button'
                                        disabled={unavailable}
                                        onClick={() => onNavigate(f.id)}
                                        className='flex min-h-11 w-full items-center gap-3 rounded-[12px] border px-4 text-left text-[14px] font-medium disabled:opacity-60'
                                        style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}
                                    >
                                        <Icon aria-hidden='true' size={16} />
                                        <span className='flex-grow'>{f.label}</span>
                                        {unavailable && f.availability.state === 'unavailable' && (
                                            <span className='text-[11px] uppercase' style={{ color: 'var(--v2-faint)' }}>
                                                {f.availability.plannedChunk}
                                            </span>
                                        )}
                                    </button>
                                </li>
                            )
                        })}
                </ul>
            </div>
        </section>
    )
})
