import { useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, RefreshCw } from 'lucide-react'
import type {
    GatewayLimits,
    GatewayLogs,
    GatewayRead,
    GatewayUsage,
    LimitPolicy,
    ManagementFailure,
    ManagementResult,
    ObserveRead,
    PolicyList,
    Series,
} from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT } from '../administration/management-read'

/** One Gateway read through the view (scoped to the chosen workspace). */
export type GatewayReader = (read: GatewayRead, scoped: boolean) => Promise<ManagementResult | string>

const WINDOWS = [1, 7, 30] as const
const WINDOW_TEXT: Record<number, string> = { 1: 'Last day', 7: 'Last 7 days', 30: 'Last 30 days' }

function failureText(result: ManagementResult | string): string {
    if (typeof result === 'string') return result
    return result.kind === 'failed' ? MANAGEMENT_FAILURE_TEXT[result.failure] : MANAGEMENT_FAILURE_TEXT.invalidResponse
}

function observed(result: ManagementResult | string) {
    return typeof result !== 'string' && result.kind === 'gateway' && result.gateway.kind === 'observed' ? result.gateway.observed : null
}

async function observe(read: GatewayReader, what: ObserveRead) {
    const result = await read({ kind: 'observe', read: what }, true)
    return { result, observed: observed(result) }
}

const number = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 1 })
/** Cost as the service reports it (cents) in dollars. */
export const dollars = (cents: number) => (cents / 100).toLocaleString(undefined, { style: 'currency', currency: 'USD' })
const sourceFailure = (failure: ManagementFailure | null) => (failure ? MANAGEMENT_FAILURE_TEXT[failure] : null)

/** Bars drawn at most: wider series are combined into this many. */
const MAX_BARS = 160

/**
 * The bars to draw: each point, or (for a long series) consecutive points
 * combined, summed or (latency) averaged. Only the drawing is combined;
 * the summary and the values table keep every point.
 */
export function bars(values: number[], mean: boolean): number[] {
    if (values.length <= MAX_BARS) return values
    const size = Math.ceil(values.length / MAX_BARS)
    const out: number[] = []
    for (let i = 0; i < values.length; i += size) {
        const group = values.slice(i, i + size)
        const sum = group.reduce((a, b) => a + b, 0)
        out.push(mean ? sum / group.length : sum)
    }
    return out
}

/** A series' total as text: unknown is never zero. */
function totalText(series: Series, format: (n: number) => string): string {
    if (series.partial) return 'Part of the window only (the service’s quota was exceeded)'
    return series.total === null ? 'Not reported' : format(series.total)
}

interface ChartProps {
    title: string
    series: Series | null
    failure: ManagementFailure | null
    format: (n: number) => string
    /** What the total is (a sum, or a mean for latency). */
    totalLabel: string
}

/**
 * One series as bars with a text summary for assistive technology and its
 * values in a disclosure table (keyboard reachable).
 */
function Chart({ title, series, failure, format, totalLabel }: ChartProps) {
    const id = useId()
    // The values table is built only while it is open.
    const [open, setOpen] = useState(false)
    const failed = sourceFailure(failure)
    if (failed || !series) {
        return (
            <figure className='flex flex-col gap-1' data-slot='gateway-chart'>
                <figcaption className='text-[14px] font-semibold'>{title}</figcaption>
                <p role='status' className='text-academy-accent-warn text-[13px]'>
                    {failed ? `Not read: ${failed}` : 'Not read yet.'}
                </p>
            </figure>
        )
    }
    const peak = series.points.reduce((m, p) => Math.max(m, p.y), 0)
    const peakAt = series.points.find((p) => p.y === peak)
    const summary =
        series.points.length === 0
            ? `${title}: no activity in the window.`
            : `${title}: ${totalLabel} ${totalText(series, format)}; ${series.points.length} intervals; highest ${format(peak)}${peakAt ? ` at ${peakAt.x}` : ''}.`
    const width = 320
    const height = 64
    const drawn = bars(
        series.points.map((p) => p.y),
        totalLabel === 'Mean'
    )
    const highest = drawn.reduce((m, v) => Math.max(m, v), 0)
    const bar = drawn.length > 0 ? width / drawn.length : width
    return (
        <figure className='flex flex-col gap-1' data-slot='gateway-chart' aria-labelledby={`${id}-title`}>
            <figcaption id={`${id}-title`} className='flex flex-wrap items-baseline gap-2 text-[14px] font-semibold'>
                {title}
                <span className='text-academy-ink-soft text-[13px] font-normal' data-slot='gateway-chart-total'>
                    {`${totalLabel}: ${totalText(series, format)}`}
                </span>
            </figcaption>
            <svg role='img' aria-label={summary} viewBox={`0 0 ${width} ${height}`} className='h-16 w-full max-w-sm' preserveAspectRatio='none'>
                {drawn.map((v, i) => {
                    const h = highest > 0 ? (v / highest) * (height - 2) : 0
                    return <rect key={i} x={i * bar} y={height - h} width={Math.max(bar - 1, 1)} height={h} className='fill-current text-academy-accent' />
                })}
            </svg>
            {series.points.length > 0 ? (
                <details className='text-[12px]' onToggle={(e) => setOpen((e.currentTarget as HTMLDetailsElement).open)}>
                    <summary className='cursor-pointer'>Show {title.toLowerCase()} values</summary>
                    {open ? (
                    <table className='mt-1 text-left'>
                        <caption className='sr-only'>{title} by interval</caption>
                        <thead>
                            <tr>
                                <th scope='col' className='pr-3'>
                                    Interval
                                </th>
                                <th scope='col'>Value</th>
                            </tr>
                        </thead>
                        <tbody>
                            {series.points.map((p, i) => (
                                <tr key={`${p.x}-${i}`}>
                                    <td className='pr-3 font-mono'>{p.x}</td>
                                    <td>{format(p.y)}</td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                    ) : null}
                </details>
            ) : null}
        </figure>
    )
}

interface Held<T> {
    read: T | null
    loading: boolean
    failure: string | null
}

const idle = <T,>(): Held<T> => ({ read: null, loading: false, failure: null })

/** Keeps the newest of overlapping reads; a superseded reply is dropped. */
function useLatest() {
    const token = useRef(0)
    return {
        next: () => ++token.current,
        current: (mine: number) => mine === token.current,
    }
}

/** The chosen workspace's usage over a window (C18). */
export function UsagePanel({ workspace, read }: { workspace: string; read: GatewayReader }) {
    const [days, setDays] = useState<number>(7)
    const [held, setHeld] = useState<Held<GatewayUsage>>(idle)
    const latest = useLatest()
    const ids = useId()

    async function refresh(window = days) {
        const mine = latest.next()
        setHeld((h) => ({ ...h, loading: true }))
        const { result, observed: o } = await observe(read, { kind: 'usage', days: window })
        if (!latest.current(mine)) return
        setHeld((h) => (o?.kind === 'usage' ? { read: o.usage, loading: false, failure: null } : { ...h, loading: false, failure: failureText(result) }))
    }

    useEffect(() => {
        void refresh()
    }, [])

    const usage = held.read
    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col gap-3' data-slot='gateway-usage'>
            <h3 id={`${ids}-h`} className='text-[15px] font-semibold'>
                Usage of {workspace}
            </h3>
            <div className='flex flex-wrap items-center gap-2'>
                <div role='radiogroup' aria-label='Usage window' className='flex items-center gap-3 text-[13px]'>
                    {WINDOWS.map((w) => (
                        <label key={w} className='flex items-center gap-1'>
                            <input
                                type='radio'
                                name={`${ids}-window`}
                                checked={days === w}
                                onChange={() => {
                                    setDays(w)
                                    void refresh(w)
                                }}
                            />
                            {WINDOW_TEXT[w]}
                        </label>
                    ))}
                </div>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && void refresh()}>
                    <RefreshCw aria-hidden className='size-4' /> Refresh usage
                </Button>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    {held.loading ? 'Reading usage…' : usage ? `${WINDOW_TEXT[usage.days]} as the service reported it.` : ''}
                </p>
            </div>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    Usage could not be read: {held.failure}
                    {usage ? ' The values below are from the last read.' : ''}
                </Callout>
            ) : null}
            {usage ? (
                <>
                    <div className='grid gap-4 md:grid-cols-2'>
                        <Chart title='Requests' series={usage.requests} failure={usage.requestsFailure} format={number} totalLabel='Total' />
                        <Chart title='Cost' series={usage.cost} failure={usage.costFailure} format={dollars} totalLabel='Total' />
                        <Chart title='Tokens' series={usage.tokens} failure={usage.tokensFailure} format={number} totalLabel='Total' />
                        <Chart title='Latency' series={usage.latency} failure={usage.latencyFailure} format={(n) => `${number(n)} ms`} totalLabel='Mean' />
                    </div>
                    {usage.requests?.errors !== null && usage.requests?.errors !== undefined ? (
                        <p className='text-[13px]' data-slot='gateway-usage-errors'>
                            Errors in the window: {number(usage.requests.errors)}
                        </p>
                    ) : null}
                    <ModelsTable usage={usage} />
                </>
            ) : null}
        </section>
    )
}

function ModelsTable({ usage }: { usage: GatewayUsage }) {
    if (usage.modelsFailure) {
        return <p className='text-academy-accent-warn text-[13px]'>Models could not be read: {MANAGEMENT_FAILURE_TEXT[usage.modelsFailure]}</p>
    }
    const models = usage.models
    if (!models) return null
    if (models.rows.length === 0) {
        return <p className='text-[13px]'>{models.partial ? 'No models were returned for the part of the window read.' : 'No model was used in the window.'}</p>
    }
    return (
        <table className='text-left text-[13px]' data-slot='gateway-models'>
            <caption className='text-left font-semibold'>
                Busiest models{models.complete ? '' : ' (the busiest 100 shown)'}
                {models.partial ? ' · part of the window only' : ''}
            </caption>
            <thead>
                <tr>
                    <th scope='col' className='pr-4'>
                        Model
                    </th>
                    <th scope='col' className='pr-4'>
                        Provider
                    </th>
                    <th scope='col' className='pr-4'>
                        Requests
                    </th>
                    <th scope='col' className='pr-4'>
                        Cost
                    </th>
                    <th scope='col'>Tokens</th>
                </tr>
            </thead>
            <tbody>
                {models.rows.map((m) => (
                    <tr key={`${m.provider ?? ''}/${m.model}`}>
                        <th scope='row' className='pr-4 font-mono font-normal'>
                            {m.model}
                        </th>
                        <td className='pr-4'>{m.provider ?? '—'}</td>
                        <td className='pr-4'>{number(m.requests)}</td>
                        <td className='pr-4'>{m.cost === null ? 'Not reported' : dollars(m.cost)}</td>
                        <td>{m.totalTokens === null ? 'Not reported' : number(m.totalTokens)}</td>
                    </tr>
                ))}
            </tbody>
        </table>
    )
}

/** The chosen workspace's request logs, page by page (C18). */
/** The filters a list of log pages was read with. */
interface LogQuery {
    days: number
    trace: string | null
    status: number | null
}

const LOG_PAGE = 25
const LOG_PAGES = 40

const sameQuery = (page: GatewayLogs, q: LogQuery) => page.days === q.days && page.trace === q.trace && page.status === q.status

function queryText(q: LogQuery): string {
    return `${(WINDOW_TEXT[q.days] ?? `${q.days} days`).toLowerCase()}${q.trace !== null ? `, trace ${q.trace}` : ''}${
        q.status !== null ? `, status ${q.status}` : ''
    }`
}

export function LogsPanel({ workspace, read, trace: initialTrace }: { workspace: string; read: GatewayReader; trace?: string }) {
    const ids = useId()
    const [days, setDays] = useState<number>(7)
    const [trace, setTrace] = useState(initialTrace ?? '')
    const [status, setStatus] = useState('')
    const [pages, setPages] = useState<GatewayLogs[]>([])
    const [held, setHeld] = useState<{ loading: boolean; failure: string | null }>({ loading: false, failure: null })
    const [invalid, setInvalid] = useState<string | null>(null)
    const latest = useLatest()

    /** The query the form names, or why it names none. */
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

    /**
     * Reads one page of `q`. Page 0 starts a list; a later page continues
     * only the list of the same query it follows. Only the latest read
     * commits, and a page of any other query is never added.
     */
    async function load(page: number, q: LogQuery) {
        const mine = latest.next()
        setHeld({ loading: true, failure: null })
        const what: ObserveRead = { kind: 'logs', days: q.days, page }
        if (q.trace !== null) what.trace = q.trace
        if (q.status !== null) what.status = q.status
        const { result, observed: o } = await observe(read, what)
        if (!latest.current(mine)) return
        if (o?.kind === 'logs' && o.logs.page === page && sameQuery(o.logs, q)) {
            setPages((p) => (page === 0 ? [o.logs] : p.length === page && p.every((x) => sameQuery(x, q)) ? [...p, o.logs] : p))
            setHeld({ loading: false, failure: null })
        } else setHeld({ loading: false, failure: o ? 'The service answered for other filters.' : failureText(result) })
    }

    useEffect(() => {
        const q = query()
        if (q) void load(0, q)
    }, [])

    const rows = pages.flatMap((p) => p.rows)
    const last = pages.at(-1)
    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col gap-3' data-slot='gateway-logs'>
            <h3 id={`${ids}-h`} className='text-[15px] font-semibold'>
                Request logs of {workspace}
            </h3>
            <form
                className='flex flex-wrap items-end gap-2'
                onSubmit={(e) => {
                    e.preventDefault()
                    const q = query()
                    if (q) void load(0, q)
                }}
            >
                <label className='flex flex-col text-[13px]'>
                    Window
                    <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
                        {WINDOWS.map((w) => (
                            <option key={w} value={w}>
                                {WINDOW_TEXT[w]}
                            </option>
                        ))}
                    </select>
                </label>
                <label className='flex flex-col text-[13px]'>
                    Trace ID
                    <input value={trace} onChange={(e) => setTrace(e.target.value)} className='font-mono' spellCheck={false} />
                </label>
                <label className='flex flex-col text-[13px]'>
                    Status
                    <input value={status} onChange={(e) => setStatus(e.target.value)} inputMode='numeric' className='w-20' />
                </label>
                <Button type='submit' tier='secondary' size='sm'>
                    Read logs
                </Button>
            </form>
            {invalid ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {invalid}
                </p>
            ) : null}
            <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='gateway-logs-status'>
                {held.loading
                    ? 'Reading logs…'
                    : last
                      ? `${rows.length.toLocaleString()} request${rows.length === 1 ? '' : 's'} shown${
                            last.total !== null ? ` of ${last.total.toLocaleString()}` : ''
                        } for ${queryText(last)}${last.partial ? ' · part of the window only (the service’s quota was exceeded)' : ''}${
                            last.capped
                                ? ` · only the first ${(LOG_PAGES * LOG_PAGE).toLocaleString()} are read here: more exist${last.total === null ? ' or may exist' : ''}`
                                : last.more || rows.length === 0 || last.partial
                                  ? ''
                                  : ' · all read'
                        }`
                      : ''}
            </p>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    The logs could not be read: {held.failure}
                </Callout>
            ) : null}
            {last && rows.length === 0 && !held.loading ? (
                <p className='text-[13px]'>{last.partial ? 'No request was returned for the part of the window read.' : 'No request matches.'}</p>
            ) : null}
            {rows.length > 0 ? (
                <table className='text-left text-[13px]' data-slot='gateway-log-rows'>
                    <caption className='sr-only'>Requests, newest first</caption>
                    <thead>
                        <tr>
                            <th scope='col' className='pr-3'>
                                Time
                            </th>
                            <th scope='col' className='pr-3'>
                                Model
                            </th>
                            <th scope='col' className='pr-3'>
                                Status
                            </th>
                            <th scope='col' className='pr-3'>
                                Units
                            </th>
                            <th scope='col' className='pr-3'>
                                Cost
                            </th>
                            <th scope='col'>Trace</th>
                        </tr>
                    </thead>
                    <tbody>
                        {rows.map((r) => (
                            <tr key={r.id}>
                                <td className='pr-3'>{r.createdAt}</td>
                                <td className='pr-3 font-mono'>{r.provider ? `${r.provider}/${r.model}` : r.model}</td>
                                <td className='pr-3'>{r.status === 446 ? '446 (blocked by a guardrail)' : r.status}</td>
                                <td className='pr-3'>{r.units === null ? '—' : number(r.units)}</td>
                                <td className='pr-3'>{r.cost === null ? '—' : dollars(r.cost)}</td>
                                <td className='font-mono'>{r.traceId ?? '—'}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            ) : null}
            {last?.more ? (
                <Button
                    type='button'
                    tier='tertiary'
                    size='sm'
                    className='self-start'
                    aria-disabled={held.loading}
                    onClick={() => !held.loading && void load(last.page + 1, { days: last.days, trace: last.trace, status: last.status })}
                >
                    Read more logs
                </Button>
            ) : null}
        </section>
    )
}

function PolicyTable({ list, usage }: { list: PolicyList; usage: boolean }) {
    const what = usage ? 'usage limits' : 'rate limits'
    if (list.policies.length === 0) {
        return <p className='text-[13px]'>{list.complete ? `No ${what} apply to this workspace.` : `No ${what} were read. The service did not say none exist.`}</p>
    }
    const value = (p: LimitPolicy) =>
        usage
            ? `${p.creditLimit === null ? 'No limit reported' : p.limitType === 'cost' ? dollars(p.creditLimit) : number(p.creditLimit)}${
                  p.alertThreshold === null ? '' : ` · alert at ${number(p.alertThreshold)}%`
              }`
            : `${p.value === null ? '—' : number(p.value)}${p.unit ? ` per ${p.unit}` : ''}`
    return (
        <table className='text-left text-[13px]'>
            <caption className='text-left font-semibold'>
                {usage ? 'Usage limits' : 'Rate limits'}
                {list.complete ? '' : ' · more exist and are not shown'}
            </caption>
            <thead>
                <tr>
                    <th scope='col' className='pr-3'>
                        Name
                    </th>
                    <th scope='col' className='pr-3'>
                        Status
                    </th>
                    <th scope='col' className='pr-3'>
                        Limit
                    </th>
                    <th scope='col' className='pr-3'>
                        Applied by
                    </th>
                    <th scope='col'>{usage ? 'Next reset' : 'Target'}</th>
                </tr>
            </thead>
            <tbody>
                {list.policies.map((p) => (
                    <tr key={p.id}>
                        <th scope='row' className='pr-3 font-normal'>
                            {p.name ?? p.id}
                        </th>
                        <td className='pr-3'>{p.status}</td>
                        <td className='pr-3'>{value(p)}</td>
                        <td className='pr-3'>{p.groupBy.length > 0 ? p.groupBy.join(', ') : 'The whole workspace'}</td>
                        <td>{usage ? (p.nextResetAt ?? p.periodicReset ?? '—') : (p.target ?? '—')}</td>
                    </tr>
                ))}
            </tbody>
        </table>
    )
}

/** The rate- and usage-limit policies that apply to the chosen workspace. */
export function LimitsPanel({ workspace, read }: { workspace: string; read: GatewayReader }) {
    const ids = useId()
    const [held, setHeld] = useState<Held<GatewayLimits>>(idle)
    const latest = useLatest()

    async function refresh() {
        const mine = latest.next()
        setHeld((h) => ({ ...h, loading: true }))
        const { result, observed: o } = await observe(read, { kind: 'limits' })
        if (!latest.current(mine)) return
        setHeld((h) => (o?.kind === 'limits' ? { read: o.limits, loading: false, failure: null } : { ...h, loading: false, failure: failureText(result) }))
    }

    useEffect(() => {
        void refresh()
    }, [])

    const limits = held.read
    return (
        <section aria-labelledby={`${ids}-h`} className='flex flex-col gap-3' data-slot='gateway-limits'>
            <h3 id={`${ids}-h`} className='text-[15px] font-semibold'>
                Limits on {workspace}
            </h3>
            <div className='flex flex-wrap items-center gap-2'>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && void refresh()}>
                    <RefreshCw aria-hidden className='size-4' /> Refresh limits
                </Button>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    {held.loading ? 'Reading limits…' : ''}
                </p>
            </div>
            <Callout tone='default' icon={Info}>
                Limit policies are read here; they are changed in the Gateway console. What a policy matches by is not shown.
            </Callout>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    The limits could not be read: {held.failure}
                </Callout>
            ) : null}
            {limits ? (
                <>
                    {limits.usage ? <PolicyTable list={limits.usage} usage /> : null}
                    {limits.usageFailure ? (
                        <p className='text-academy-accent-warn text-[13px]'>Usage limits could not be read: {MANAGEMENT_FAILURE_TEXT[limits.usageFailure]}</p>
                    ) : null}
                    {limits.rate ? <PolicyTable list={limits.rate} usage={false} /> : null}
                    {limits.rateFailure ? (
                        <p className='text-academy-accent-warn text-[13px]'>Rate limits could not be read: {MANAGEMENT_FAILURE_TEXT[limits.rateFailure]}</p>
                    ) : null}
                </>
            ) : null}
        </section>
    )
}
