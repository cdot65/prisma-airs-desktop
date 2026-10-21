import { useEffect, useId, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, CheckCircle2, Info, RefreshCw } from 'lucide-react'
import type { ContextRef, FeatureId, InboxItem, OverviewView, RuntimeChart, RuntimeOverview } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { MANAGEMENT_FAILURE_TEXT } from '../administration/management-read'

/** Ten minutes: v1's freshness and automatic-retry interval. */
export const FRESH_SECS = 600

const clock = (secs: number) => new Date(secs * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
const nowSecs = () => Math.floor(Date.now() / 1000)
const count = (n: number | null) => (n === null ? 'Unreported' : n.toLocaleString())

interface Props {
    context: ContextRef
    overview: OverviewView
    ready: boolean
    dispatch: Dispatch
    onNavigate: (id: FeatureId) => void
}

/**
 * Runtime Home/Inbox (C12): the last day's sessions chart and API key
 * expiries as Inbox items and Needs attention, each source's state stated
 * (fresh, stale, failed, partial or unknown) and never shown as a healthy
 * zero. Reads happen on opening when nothing fresh is held (not again for
 * ten minutes after a failure) and on Refresh; nothing is ever replayed.
 */
export function OverviewInbox({ context, overview, ready, dispatch, onNavigate }: Props) {
    const ids = useId()
    const [filter, setFilter] = useState<'all' | 'action' | 'update'>('all')
    const [selected, setSelected] = useState<string | null>(null)
    const [table, setTable] = useState(false)
    const [message, setMessage] = useState<string | null>(null)

    async function refresh() {
        const outcome = await dispatch({ kind: 'management', context, request: { kind: 'overview' } })
        setMessage(outcome.kind === 'admitted' ? null : outcomeMessage(outcome))
    }

    useEffect(() => {
        const recentlyFailed = overview.failedAt !== null && nowSecs() - overview.failedAt < FRESH_SECS
        if (ready && !overview.fresh && !overview.refreshing && !recentlyFailed) void refresh()
    }, [context, ready])

    if (!ready) {
        return (
            <Callout tone='default' icon={Info} data-slot='overview-not-configured'>
                Runtime insights read the tenant's sessions chart and API keys with its management client. Configure one in
                Settings to see them.
            </Callout>
        )
    }

    const read = overview.read
    const items = read?.items ?? []
    const shown = items.filter((i) => filter === 'all' || (filter === 'action' ? i.action : !i.action))
    const detail = shown.find((i) => i.id === selected) ?? shown[0] ?? null
    const hold = (id: string, kind: 'acknowledge' | 'snooze') => void dispatch({ kind: 'holdInsight', context, id, hold: kind })
    const go = (item: InboxItem) =>
        onNavigate(item.destination === 'sessions' ? 'sessions' : item.destination === 'gateway' ? 'gateway' : 'administration')

    const status = overview.refreshing
        ? 'Refreshing Runtime insights…'
        : read
          ? `${overview.fresh ? 'Updated' : 'Stale · as of'} ${clock(read.readAt)}${read.gaps.length > 0 ? ' · partly complete' : ''}`
          : 'Not read yet'

    return (
        <div className='flex flex-col gap-5' data-slot='overview'>
            <div className='flex flex-wrap items-center gap-3'>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='overview-status'>
                    {status}
                </p>
                <Button type='button' tier='secondary' size='sm' disabled={overview.refreshing} onClick={() => void refresh()}>
                    <RefreshCw aria-hidden className='size-4' /> Refresh Runtime insights
                </Button>
            </div>
            {overview.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='overview-failure'>
                    <p>
                        The last refresh failed{overview.failedAt ? ` at ${clock(overview.failedAt)}` : ''}: {MANAGEMENT_FAILURE_TEXT[overview.failure]}
                    </p>
                    {read ? <p className='mt-1 text-[12px]'>Showing what was read at {clock(read.readAt)}. It may be out of date.</p> : null}
                </Callout>
            ) : null}
            {message ? <p role='alert' className='text-[13px]'>{message}</p> : null}

            <NeedsAttention overview={overview} onHold={hold} onRestore={(id) => void dispatch({ kind: 'restoreInsight', context, id })} onOpen={go} />

            <section aria-labelledby={`${ids}-chart`} className='flex flex-col gap-2' data-slot='overview-chart'>
                <div className='flex flex-wrap items-center gap-2'>
                    <h2 id={`${ids}-chart`} className='text-[15px] font-semibold'>
                        Runtime detections
                    </h2>
                    <Button type='button' size='sm' tier='tertiary' aria-pressed={table} onClick={() => setTable((t) => !t)}>
                        Table
                    </Button>
                </div>
                {read?.chart ? (
                    <>
                        <p className='text-academy-ink-soft text-[12px]' data-slot='overview-summary'>
                            {`${read.chart.bucketSeconds === 3600 ? 'hourly' : `${read.chart.bucketSeconds} s buckets`} · last 24h · ${count(read.summary.sessions)} sessions · ${count(read.summary.violating)} violating · ${count(read.summary.benign)} benign · ${count(read.summary.detectionEvents)} detection events`}
                        </p>
                        {read.chart.buckets.length === 0 ? (
                            <p className='text-[13px]'>No detections in the last 24 hours.</p>
                        ) : table ? (
                            <ChartTable chart={read.chart} />
                        ) : (
                            <DetectionChart chart={read.chart} />
                        )}
                        {read.summary.detectionEvents === null && read.chart.buckets.length > 0 ? (
                            <p className='text-academy-ink-faint text-[12px]'>Violating sessions · detector details unavailable</p>
                        ) : null}
                    </>
                ) : read?.chartFailure ? (
                    <p className='text-[13px]' data-slot='overview-chart-unavailable'>
                        Runtime activity unavailable: {MANAGEMENT_FAILURE_TEXT[read.chartFailure]}
                    </p>
                ) : (
                    <p className='text-academy-ink-soft text-[13px]'>Runtime activity loading.</p>
                )}
            </section>

            <GatewayDay read={read} onOpen={() => onNavigate('gateway')} />

            <section aria-labelledby={`${ids}-inbox`} className='flex flex-col gap-3' data-slot='inbox'>
                <h2 id={`${ids}-inbox`} className='text-[15px] font-semibold'>
                    Inbox
                </h2>
                <div role='group' aria-label='Inbox filter' className='flex gap-1'>
                    {(
                        [
                            ['all', 'All', items.length],
                            ['action', 'Needs action', items.filter((i) => i.action).length],
                            ['update', 'Updates', items.filter((i) => !i.action).length],
                        ] as const
                    ).map(([id, label, n]) => (
                        <Button key={id} type='button' size='sm' tier={filter === id ? 'secondary' : 'tertiary'} aria-pressed={filter === id} onClick={() => setFilter(id)}>
                            {`${label} ${n}`}
                        </Button>
                    ))}
                </div>
                {shown.length === 0 ? (
                    <p className='text-[13px]'>
                        No matching insights from loaded sources. Sources still loading, unavailable or partly listed are shown below.
                        Refresh to check again.
                    </p>
                ) : (
                    <div className='flex flex-wrap gap-4'>
                        <ul className='flex min-w-64 flex-1 flex-col gap-1' aria-label='Insights'>
                            {shown.map((i) => (
                                <li key={i.id}>
                                    <button
                                        type='button'
                                        aria-current={detail?.id === i.id ? 'true' : undefined}
                                        className='hover:bg-academy-paper-deep w-full rounded px-2 py-1.5 text-left text-[13px] aria-[current=true]:font-semibold'
                                        onClick={() => setSelected(i.id)}
                                    >
                                        {i.title}
                                        <span className='text-academy-ink-soft block text-[12px]'>
                                            {i.source} · {i.action ? 'Needs action' : 'Update'}
                                        </span>
                                    </button>
                                </li>
                            ))}
                        </ul>
                        {detail ? (
                            <div className='border-academy-rule rounded-academy-md flex min-w-64 flex-1 flex-col gap-2 border p-4 text-[13px]' data-slot='inbox-detail'>
                                <h3 className='text-[14px] font-semibold'>{detail.title}</h3>
                                <p>{detail.detail}</p>
                                {detail.perMille !== null ? (
                                    <p>
                                        {`${(detail.perMille / 10).toFixed(1)}% of ${
                                            detail.destination === 'gateway' ? 'Gateway requests failed' : 'sessions reported violating'
                                        }`}
                                    </p>
                                ) : null}
                                <dl className='grid grid-cols-[auto_1fr] gap-x-3 gap-y-1'>
                                    {detail.facts.map((f) => (
                                        <div key={f.label} className='contents'>
                                            <dt className='text-academy-ink-soft'>{f.label}</dt>
                                            <dd>{f.value}</dd>
                                        </div>
                                    ))}
                                </dl>
                                <Button type='button' size='sm' tier='secondary' className='self-start' onClick={() => go(detail)}>
                                    {detail.actionLabel}
                                </Button>
                            </div>
                        ) : null}
                    </div>
                )}
                {read && read.gaps.length > 0 ? (
                    <div data-slot='inbox-gaps'>
                        <h3 className='text-[13px] font-semibold'>Source coverage</h3>
                        <ul className='text-academy-ink-soft list-disc pl-5 text-[12px]'>
                            {read.gaps.map((g) => (
                                <li key={g}>{g}</li>
                            ))}
                        </ul>
                    </div>
                ) : null}
            </section>
        </div>
    )
}

const usd = (cents: number) => (cents / 100).toLocaleString(undefined, { style: 'currency', currency: 'USD' })

/**
 * The chosen Gateway workspace's last day (C18), with each source's state:
 * no workspace chosen, its data, or why it could not be read.
 */
function GatewayDay({ read, onOpen }: { read: RuntimeOverview | null; onOpen: () => void }) {
    const ids = useId()
    if (!read) return null
    const g = read.gateway
    return (
        <section aria-labelledby={`${ids}-gw`} className='flex flex-col gap-2' data-slot='overview-gateway'>
            <h2 id={`${ids}-gw`} className='text-[15px] font-semibold'>
                {g ? `AI Gateway · ${g.workspaceName}` : 'AI Gateway'}
            </h2>
            {!g ? (
                <div className='flex flex-wrap items-center gap-2 text-[13px]'>
                    <p>No Gateway workspace was chosen when this was read. Choose one in AI Gateway to see its last day here.</p>
                    <Button type='button' size='sm' tier='tertiary' onClick={onOpen}>
                        Open AI Gateway
                    </Button>
                </div>
            ) : (
                <>
                    {g.usageFailure ? (
                        <p className='text-[13px]'>Gateway activity unavailable: {MANAGEMENT_FAILURE_TEXT[g.usageFailure]}</p>
                    ) : (
                        <p className='text-[13px]' data-slot='overview-gateway-day'>
                            {`Last day${g.partial ? ' (part of the day only: the service’s quota was exceeded)' : ''}: ${
                                g.requests === null ? 'requests not reported' : `${g.requests.toLocaleString()} requests`
                            } · ${g.errors === null ? 'errors not reported' : `${g.errors.toLocaleString()} errors`} · ${
                                g.cost === null ? 'cost not reported' : `${usd(g.cost)} cost`
                            }`}
                        </p>
                    )}
                    <p className='text-academy-ink-soft text-[12px]'>
                        {g.policies !== null
                            ? `${g.policies} limit polic${g.policies === 1 ? 'y applies' : 'ies apply'} to this workspace.`
                            : `Limit policies unavailable${g.limitsFailure ? `: ${MANAGEMENT_FAILURE_TEXT[g.limitsFailure]}` : '.'}`}
                    </p>
                </>
            )}
        </section>
    )
}

function NeedsAttention({
    overview,
    onHold,
    onRestore,
    onOpen,
}: {
    overview: OverviewView
    onHold: (id: string, kind: 'acknowledge' | 'snooze') => void
    onRestore: (id: string) => void
    onOpen: (item: InboxItem) => void
}) {
    const critical = overview.attention.filter((i) => i.tone === 'bad').length
    const warn = overview.attention.length - critical
    const lead = {
        items: `${critical} critical · ${warn} warn`,
        firstLaunch: 'Checking Runtime Security',
        allClear:
            overview.held.length > 0
                ? 'Nothing open: what still needs attention is acknowledged or snoozed'
                : 'Nothing needs attention and nothing is waiting in Inbox',
        gap: 'Can’t confirm all clear',
    }[overview.face]
    const gapDetail =
        overview.face === 'gap'
            ? overview.failure
                ? `Runtime Security didn’t answer (${MANAGEMENT_FAILURE_TEXT[overview.failure]}).`
                : !overview.fresh
                  ? 'The last read is older than ten minutes.'
                  : (overview.read?.gaps[0] ?? 'A source is incomplete.')
            : null
    return (
        <section aria-label='Needs attention' className='border-academy-rule rounded-academy-md flex flex-col gap-2 border p-4 text-[13px]' data-slot='needs-attention'>
            <h2 className='text-[15px] font-semibold'>Needs attention</h2>
            <p role='status' data-slot='attention-face'>
                {overview.face === 'allClear' ? <CheckCircle2 aria-hidden className='mr-1 inline size-4' /> : null}
                {lead}
            </p>
            {gapDetail ? <p className='text-academy-ink-soft'>{gapDetail}</p> : null}
            {overview.attention.length > 0 ? (
                <ul className='flex flex-col gap-2'>
                    {overview.attention.map((i) => (
                        <li key={i.id} className='flex flex-wrap items-center gap-2'>
                            <span className='flex-1'>{i.title}</span>
                            <Button type='button' size='sm' tier='tertiary' title='Acknowledge · returns if it gets worse' onClick={() => onHold(i.id, 'acknowledge')}>
                                Acknowledge
                            </Button>
                            <Button type='button' size='sm' tier='tertiary' onClick={() => onHold(i.id, 'snooze')}>
                                Snooze 24h
                            </Button>
                            <Button type='button' size='sm' tier='secondary' onClick={() => onOpen(i)}>
                                Open evidence
                            </Button>
                        </li>
                    ))}
                </ul>
            ) : null}
            {overview.held.length > 0 ? (
                <ul className='text-academy-ink-soft flex flex-col gap-1' aria-label='Set aside'>
                    {overview.held.map((h) => (
                        <li key={h.item.id} className='flex flex-wrap items-center gap-2'>
                            <span className='flex-1'>
                                {h.item.title} · {h.kind === 'acknowledge' ? 'acknowledged' : 'snoozed'}
                            </span>
                            <Button type='button' size='sm' tier='tertiary' aria-label={`Restore ${h.item.title}`} onClick={() => onRestore(h.item.id)}>
                                Restore
                            </Button>
                        </li>
                    ))}
                </ul>
            ) : null}
        </section>
    )
}

/** Series: detectors when every bucket reported a breakdown (at most four,
 *  ranked by total), otherwise violating sessions. */
export function chartSeries(chart: RuntimeChart): { name: string; values: number[] }[] {
    const complete = chart.buckets.every((b) => b.detections !== null)
    if (!complete) return [{ name: 'Violating sessions', values: chart.buckets.map((b) => b.violating) }]
    const totals = new Map<string, number>()
    for (const b of chart.buckets) for (const d of b.detections ?? []) totals.set(d.detector, (totals.get(d.detector) ?? 0) + d.count)
    const top = [...totals.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 4)
    if (top.length === 0) return [{ name: 'Violating sessions', values: chart.buckets.map((b) => b.violating) }]
    return top.map(([name]) => ({ name, values: chart.buckets.map((b) => b.detections?.find((d) => d.detector === name)?.count ?? 0) }))
}

const COLORS = ['var(--academy-accent)', '#d65d0e', '#689d6a', '#b16286']

function DetectionChart({ chart }: { chart: RuntimeChart }) {
    const series = chartSeries(chart)
    const max = Math.max(1, ...series.flatMap((s) => s.values))
    const w = 640
    const h = 196
    const n = chart.buckets.length
    const x = (i: number) => (n <= 1 ? w / 2 : (i / (n - 1)) * w)
    const y = (v: number) => h - (v / max) * (h - 8)
    return (
        <figure className='flex flex-col gap-1'>
            <svg role='img' aria-label={`Runtime detections, ${series.map((s) => s.name).join(', ')}`} viewBox={`0 0 ${w} ${h}`} className='h-48 w-full'>
                {series.map((s, si) => (
                    <polyline key={s.name} fill='none' stroke={COLORS[si % COLORS.length]} strokeWidth={si === 0 ? 2.5 : 1.5} points={s.values.map((v, i) => `${x(i)},${y(v)}`).join(' ')} />
                ))}
            </svg>
            <figcaption className='flex flex-wrap gap-3 text-[12px]'>
                {series.map((s, si) => (
                    <span key={s.name}>
                        <span aria-hidden style={{ color: COLORS[si % COLORS.length] }}>
                            ■{' '}
                        </span>
                        {s.name}
                    </span>
                ))}
            </figcaption>
        </figure>
    )
}

function ChartTable({ chart }: { chart: RuntimeChart }) {
    return (
        <table className='w-full border-collapse text-left text-[12px]' data-slot='overview-chart-table'>
            <caption className='sr-only'>Runtime sessions per bucket</caption>
            <thead>
                <tr>
                    <th scope='col'>Bucket</th>
                    <th scope='col'>Sessions</th>
                    <th scope='col'>Violating</th>
                    <th scope='col'>Detections</th>
                </tr>
            </thead>
            <tbody>
                {chart.buckets.map((b) => (
                    <tr key={b.time}>
                        <td>{b.time}</td>
                        <td>{b.total}</td>
                        <td>{b.violating}</td>
                        <td>{b.detections === null ? 'Unreported' : b.detections.map((d) => `${d.detector} ${d.count}`).join(', ') || '0'}</td>
                    </tr>
                ))}
            </tbody>
        </table>
    )
}
