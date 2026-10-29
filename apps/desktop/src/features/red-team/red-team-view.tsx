import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info, RefreshCw } from 'lucide-react'
import type {
    AttackPage,
    Bar,
    ContextRef,
    FeatureId,
    ManagementFailure,
    ManagementRequest,
    ManagementResult,
    RedTeamObservation,
    ShellSnapshot,
    SupplyChainObservation,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import type { Dispatch } from '../tenants/dispatch'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, readManagement } from '../administration/management-read'

/** Attack results are read 100 per page, at most ten pages (v1). */
export const ATTACK_PAGES = 10
const WINDOWS = [7, 15, 30] as const
type Window = (typeof WINDOWS)[number]
type Tab = 'redTeam' | 'models'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
    /** v2 (C31): the product the shell's Administration accordion selected; strip and heading are then the shell's. */
    tab?: Tab
    /** v2 (C31): one Administration tab's part of the product; all of it when absent. */
    section?: RedTeamSection
    embedded?: boolean
}

export type RedTeamTab = Tab
/** The Administration tabs of the two read-only products (screens/administration-inventory.md). */
export type RedTeamSection = 'overview' | 'targets' | 'scans' | 'attacks' | 'reports' | 'modelOverview' | 'modelScans' | 'skillScans'

/** A product read held by the view: the last read, kept (dated) when a
 * later one fails. */
interface Held<T> {
    read: T | null
    loading: boolean
    failure: string | null
}

const empty = <T,>(): Held<T> => ({ read: null, loading: false, failure: null })
const clock = (secs: number) => new Date(secs * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
const short = (id: string | null) => (id ? id.slice(0, 8) : 'Not reported')
const words = (token: string | null) => (token ? token.replace(/_/g, ' ').toLowerCase().replace(/^./, (c) => c.toUpperCase()) : 'Not reported')

const ACTIVE = ['RUNNING', 'IN PROGRESS', 'QUEUED', 'PENDING']
const FAILED = ['FAILED', 'ERROR']
const REPORTED = ['COMPLETED', 'PASSED', 'FAILED']

/** A sum of status bars, unknown when the breakdown was not reported
 * (unless the tenant has no scans at all). */
export function statusSum(bars: Bar[] | null, scans: number, labels: string[]): number | null {
    if (bars === null) return scans === 0 ? 0 : null
    return bars.filter((b) => labels.includes(b.label)).reduce((n, b) => n + b.count, 0)
}

/** Counts of a field over the rows read. */
function tally(values: (string | null)[]): Bar[] {
    const counts = new Map<string, number>()
    for (const v of values) counts.set(words(v), (counts.get(words(v)) ?? 0) + 1)
    return [...counts].map(([label, count]) => ({ label, count })).sort((a, b) => b.count - a.count)
}

function failureText(failure: ManagementFailure) {
    return MANAGEMENT_FAILURE_TEXT[failure]
}

type Listed = { rows: unknown[]; total: number | null; capped: boolean; complete: boolean }

/** A reported total: a capped one is a lower bound. */
const reported = (total: number, capped: boolean) => `${capped ? '≥ ' : ''}${total.toLocaleString()}`

/** A list source's coverage: complete, a partial sample, or failed. */
function coverage(list: Listed | null, failure: ManagementFailure | null) {
    if (failure) return `Unavailable: ${failureText(failure)}`
    if (!list) return 'Not read'
    if (list.complete) return `${list.rows.length.toLocaleString()} read · complete`
    return list.total !== null
        ? `Partial sample · ${list.rows.length} of ${reported(list.total, list.capped)}`
        : `Partial sample · ${list.rows.length} observed`
}

/** A count shown as a lower bound when its source is a partial sample. */
const bound = (n: number, complete: boolean) => (complete ? n.toLocaleString() : `≥ ${n.toLocaleString()}`)

/**
 * Red Team and Model/skill observation (C13): v1's live reads only. AI Red
 * Teaming scan statistics for a 7, 15 or 30-day window, the first 100 scans
 * and a scan's attack results (read 100 per page on opening, at most ten
 * pages); Model
 * Security scans and AgentGuard skill statistics and scans. Each source
 * states whether it is complete, partial or unavailable and never passes for
 * zero. Names, model URIs, findings and attack prompts stay in the services;
 * skills appear by a pseudonym for one read. Scans, targets, adapters,
 * channels, prompt sets, ASR verification and Model Security workflows are
 * not available here and nothing on this page starts one. Keyed by context
 * in the shell: a tenant or credential change starts with nothing held.
 */
export const RedTeamView = forwardRef<HTMLHeadingElement, Props>(function RedTeamView({ snapshot, dispatch, observe, onNavigate, tab: controlledTab, section, embedded = false }, ref) {
    const ids = useId()
    const context = snapshot.active?.context ?? null
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const [tab, setTab] = useState<Tab>(controlledTab ?? 'redTeam')
    useEffect(() => {
        if (controlledTab) setTab(controlledTab)
    }, [controlledTab])
    const [range, setRange] = useState<Window>(30)
    const [redTeam, setRedTeam] = useState<Held<RedTeamObservation>>(empty)
    const [models, setModels] = useState<Held<SupplyChainObservation>>(empty)
    // A reply for a superseded read of the same product is dropped.
    const tokens = useRef({ redTeam: 0, models: 0 })

    async function read(request: ManagementRequest): Promise<ManagementResult | string> {
        if (!context) return 'No tenant is active.'
        const outcome = await readManagement(dispatch, observe, context, request)
        return outcome.kind === 'notAdmitted' ? outcome.message : outcome.result
    }

    async function readRedTeam(days: Window) {
        const mine = ++tokens.current.redTeam
        setRedTeam((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'redTeam', window: days })
        if (mine !== tokens.current.redTeam) return
        if (typeof result !== 'string' && result.kind === 'redTeam') setRedTeam({ read: result.observation, loading: false, failure: null })
        else setRedTeam((h) => ({ ...h, loading: false, failure: typeof result === 'string' ? result : resultFailure(result) }))
    }

    async function readModels() {
        const mine = ++tokens.current.models
        setModels((h) => ({ ...h, loading: true }))
        const result = await read({ kind: 'supplyChain' })
        if (mine !== tokens.current.models) return
        if (typeof result !== 'string' && result.kind === 'supplyChain') setModels({ read: result.observation, loading: false, failure: null })
        else setModels((h) => ({ ...h, loading: false, failure: typeof result === 'string' ? result : resultFailure(result) }))
    }

    // Each product is read when its tab is first shown with nothing held.
    useEffect(() => {
        if (!ready) return
        if (tab === 'redTeam' && !redTeam.read && !redTeam.loading && !redTeam.failure) void readRedTeam(range)
        if (tab === 'models' && !models.read && !models.loading && !models.failure) void readModels()
    }, [tab, ready])

    const heading = embedded ? null : <PageHeading ref={ref} id='red-team-heading' kicker='Red Team & Models' title='Red Teaming and model observation' />

    if (!tenant || !context) {
        return (
            <section aria-labelledby='red-team-heading' className='flex flex-col gap-4' data-slot='red-team'>
                {heading}
                <p className='text-[14px]'>Add a tenant in Settings to observe its Red Teaming and model scans.</p>
                <Button type='button' tier='secondary' className='self-start' onClick={() => onNavigate('settings')}>
                    Open Settings
                </Button>
            </section>
        )
    }

    return (
        <section aria-labelledby='red-team-heading' className='flex flex-col gap-5' data-slot='red-team'>
            {heading}
            <p className='text-academy-ink-soft max-w-3xl text-[14px]'>
                Read-only observation of {tenant.label}: counts, statuses and outcomes only. Scan, target, model and skill
                names, model URIs, findings and attack prompts stay in Prisma AIRS.
            </p>
            {!ready ? (
                <Callout tone='default' icon={Info} data-slot='red-team-not-configured'>
                    Red Teaming and model observation use the tenant's management client. Configure one in Settings to see them.
                </Callout>
            ) : (
                <>
                    <div role='tablist' aria-label='Product' className='flex gap-1' hidden={!!controlledTab}>
                        {(
                            [
                                ['redTeam', 'Red Teaming'],
                                ['models', 'Models & skills'],
                            ] as const
                        ).map(([id, label]) => (
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
                    <div role='tabpanel' id={`${ids}-panel`} aria-labelledby={`${ids}-tab-${tab}`} className='flex flex-col gap-5'>
                        {tab === 'redTeam' ? (
                            <RedTeamPanel
                                held={redTeam}
                                range={range}
                                onRange={(days) => {
                                    setRange(days)
                                    void readRedTeam(days)
                                }}
                                onRefresh={() => void readRedTeam(range)}
                                context={context}
                                read={read}
                                section={section}
                            />
                        ) : (
                            <ModelsPanel held={models} onRefresh={() => void readModels()} section={section} />
                        )}
                    </div>
                </>
            )}
        </section>
    )
})

function resultFailure(result: ManagementResult): string {
    return result.kind === 'failed' ? failureText(result.failure) : MANAGEMENT_FAILURE_TEXT.invalidResponse
}

function ReadStatus<T extends { readAt: number }>({ held, what, extra, onRefresh }: { held: Held<T>; what: string; extra: string; onRefresh: () => void }) {
    const status = held.loading ? `Reading ${what}…` : held.read ? `Updated ${clock(held.read.readAt)} · ${extra}` : 'Not read yet'
    return (
        <>
            <div className='flex flex-wrap items-center gap-3'>
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='red-team-status'>
                    {status}
                </p>
                <Button type='button' tier='secondary' size='sm' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    <RefreshCw aria-hidden className='size-4' /> {`Refresh ${what}`}
                </Button>
            </div>
            {held.failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='red-team-failure'>
                    <p>The last read failed: {held.failure}</p>
                    {held.read ? <p className='mt-1 text-[12px]'>Showing what was read at {clock(held.read.readAt)}. It may be out of date.</p> : null}
                </Callout>
            ) : null}
        </>
    )
}

function Tiles({ label, tiles }: { label: string; tiles: [string, string][] }) {
    return (
        <dl aria-label={label} className='grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))] gap-3' data-slot='red-team-tiles'>
            {tiles.map(([name, value]) => (
                <div key={name} className='border-academy-rule rounded-academy-md border p-3'>
                    <dt className='text-academy-ink-soft text-[12px]'>{name}</dt>
                    <dd className='text-[20px] font-semibold'>{value}</dd>
                </div>
            ))}
        </dl>
    )
}

function Ranked({ title, bars, emptyText }: { title: string; bars: Bar[] | null; emptyText: string }) {
    const id = useId()
    return (
        <section aria-labelledby={id} className='flex min-w-56 flex-1 flex-col gap-1'>
            <h3 id={id} className='text-[14px] font-semibold'>
                {title}
            </h3>
            {bars === null ? (
                <p className='text-academy-ink-soft text-[13px]'>Not reported by the service.</p>
            ) : bars.length === 0 ? (
                <p className='text-[13px]'>{emptyText}</p>
            ) : (
                <ul className='text-[13px]'>
                    {[...bars]
                        .sort((a, b) => b.count - a.count)
                        .map((b) => (
                            <li key={b.label} className='flex justify-between gap-4'>
                                <span>{words(b.label)}</span>
                                <span>{b.count.toLocaleString()}</span>
                            </li>
                        ))}
                </ul>
            )}
        </section>
    )
}

function Sources({ rows }: { rows: [string, string][] }) {
    return (
        <div data-slot='red-team-sources'>
            <h3 className='text-[13px] font-semibold'>Source coverage</h3>
            <ul className='text-academy-ink-soft text-[12px]'>
                {rows.map(([name, state]) => (
                    <li key={name}>{`${name} · ${state}`}</li>
                ))}
            </ul>
        </div>
    )
}

function Unavailable({ items }: { items: string }) {
    return (
        <Callout tone='default' icon={Info} data-slot='red-team-unavailable'>
            <p className='font-semibold'>Not available in the desktop</p>
            <p>{items} Use Prisma AIRS for them; nothing here starts a scan or changes the tenant.</p>
        </Callout>
    )
}

function RedTeamPanel({
    held,
    range,
    onRange,
    onRefresh,
    context,
    read,
    section,
}: {
    held: Held<RedTeamObservation>
    range: Window
    onRange: (days: Window) => void
    onRefresh: () => void
    context: ContextRef
    read: (request: ManagementRequest) => Promise<ManagementResult | string>
    section?: RedTeamSection
}) {
    const ids = useId()
    // The legacy page shows its overview and scans; the v2-only parts render only on their own tab.
    const show = (part: RedTeamSection) => !section || section === part
    const only = (part: RedTeamSection) => section === part
    const [status, setStatus] = useState('all')
    const [attacksOf, setAttacksOf] = useState<string | null>(null)
    const observation = held.read
    const stats = observation?.statistics ?? null
    const scans = observation?.scans ?? null
    const statuses = [...new Set((scans?.rows ?? []).map((r) => words(r.status)))].sort()
    const rows = (scans?.rows ?? []).filter((r) => status === 'all' || words(r.status) === status)
    const unknown = (n: number | null) => (n === null ? 'Unreported' : n.toLocaleString())
    const tiles: [string, string][] = stats
        ? [
              ['Scans', stats.scans.toLocaleString()],
              ['Targets scanned', stats.targets.toLocaleString()],
              ['Active scans', unknown(statusSum(stats.status, stats.scans, ACTIVE))],
              ['Failed scans', unknown(statusSum(stats.status, stats.scans, FAILED))],
          ]
        : [
              ['Scans', 'Unavailable'],
              ['Targets scanned', 'Unavailable'],
              ['Active scans', 'Unavailable'],
              ['Failed scans', 'Unavailable'],
          ]
    return (
        <>
            <ReadStatus held={held} what='Red Teaming' extra={`last ${observation?.window ?? range} days`} onRefresh={onRefresh} />
            {show('overview') ? (
            <fieldset className='flex flex-wrap items-center gap-3 text-[13px]'>
                <legend className='text-academy-ink-soft mb-1 text-[12px]'>Statistics window</legend>
                {WINDOWS.map((days) => (
                    <label key={days} className='flex items-center gap-1'>
                        <input type='radio' name={`${ids}-window`} checked={range === days} onChange={() => onRange(days)} />
                        {`Last ${days} days`}
                    </label>
                ))}
            </fieldset>
            ) : null}
            {observation ? (
                <>
                    {show('overview') ? (
                    <>
                    <Tiles label='Red Teaming summary' tiles={tiles} />
                    {stats?.noScans ? <p className='text-[13px]'>No scans in this tenant yet. Red Teaming scans run in Prisma AIRS.</p> : null}
                    <div className='flex flex-wrap gap-6'>
                        <Ranked title='Scans by status' bars={stats?.status ?? null} emptyText='Red Teaming returned no scans by status for this period.' />
                        <Ranked title='Targets by type' bars={stats?.targetTypes ?? null} emptyText='Red Teaming returned no targets by type for this period.' />
                        <Ranked title='Risk profile' bars={stats?.risk ?? null} emptyText='Red Teaming returned no risk ratings for this period.' />
                    </div>
                    <Sources
                        rows={[
                            ['Statistics', observation.statisticsFailure ? `Unavailable: ${failureText(observation.statisticsFailure)}` : 'Read'],
                            ['Scans', coverage(scans, observation.scansFailure)],
                        ]}
                    />
                    </>
                    ) : null}
                    {only('targets') ? <TargetsTable scans={scans} onResults={setAttacksOf} /> : null}
                    {only('attacks') ? <ReportedScans scans={scans} title='Attack results' note='Attack results are read for one reported scan at a time, at most ten pages.' action='Open attack results' onOpen={setAttacksOf} /> : null}
                    {only('reports') ? <ReportedScans scans={scans} title='Reports' note='Reports are listed from scan metadata only; nothing is generated, exported or run here.' action='Open attack results' onOpen={setAttacksOf} /> : null}
                    {show('scans') ? (
                    <section aria-labelledby={`${ids}-scans`} className='flex flex-col gap-2' data-slot='red-team-scans'>
                        <h2 id={`${ids}-scans`} className='text-[15px] font-semibold'>
                            Red Teaming scans
                        </h2>
                        <p className='text-academy-ink-soft text-[12px]'>Each list is read once, at most 100 scans; the filter applies to those rows.</p>
                        <label className='flex items-center gap-2 text-[13px]'>
                            Status
                            <select value={status} onChange={(e) => setStatus(e.target.value)} className='border-academy-rule rounded border px-2 py-1'>
                                <option value='all'>All</option>
                                {statuses.map((s) => (
                                    <option key={s} value={s}>
                                        {s}
                                    </option>
                                ))}
                            </select>
                        </label>
                        {scans === null ? (
                            <p className='text-[13px]'>The scan list could not be read.</p>
                        ) : rows.length === 0 ? (
                            <p className='text-[13px]'>
                                {scans.complete
                                    ? 'No scans in the rows read match this filter.'
                                    : 'None of the scans read match. The list could not be read in full; the coverage above says which.'}
                            </p>
                        ) : (
                            <table className='w-full text-left text-[13px]'>
                                <thead>
                                    <tr>
                                        <th scope='col'>Status</th>
                                        <th scope='col'>Scan</th>
                                        <th scope='col'>Type</th>
                                        <th scope='col'>Target</th>
                                        <th scope='col'>Created</th>
                                        <th scope='col'>
                                            <span className='sr-only'>Actions</span>
                                        </th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {rows.map((r) => (
                                        <tr key={r.id}>
                                            <td>{words(r.status)}</td>
                                            <td className='font-mono'>{short(r.id)}</td>
                                            <td>{words(r.jobType)}</td>
                                            <td>{`${words(r.targetType)} · ${short(r.targetId)}`}</td>
                                            <td>{r.createdAt ?? 'Not reported'}</td>
                                            <td>
                                                {REPORTED.includes((r.status ?? '').toUpperCase()) ? (
                                                    <Button type='button' size='sm' tier='tertiary' onClick={() => setAttacksOf(r.id)}>
                                                        {`Open attack results for scan ${short(r.id)}`}
                                                    </Button>
                                                ) : null}
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        )}
                    </section>
                    ) : null}
                    {attacksOf ? <AttackResults key={attacksOf} scanId={attacksOf} context={context} read={read} onClose={() => setAttacksOf(null)} /> : null}
                </>
            ) : null}
            {show('overview') ? (
                <Unavailable items='Starting or re-running Red Teaming scans, verifying ASR, and creating targets, custom adapters, network channels or prompt sets are not available here.' />
            ) : null}
        </>
    )
}

function AttackResults({
    scanId,
    context,
    read,
    onClose,
}: {
    scanId: string
    context: ContextRef
    read: (request: ManagementRequest) => Promise<ManagementResult | string>
    onClose: () => void
}) {
    const ids = useId()
    const [pages, setPages] = useState<AttackPage[]>([])
    const [loading, setLoading] = useState(true)
    const [failure, setFailure] = useState<string | null>(null)
    // Rows read are shown 100 at a time.
    const [shown, setShown] = useState(0)
    const heading = useRef<HTMLHeadingElement>(null)

    // The pages are read one after another, each carrying the rows received
    // so far, until the service's total is covered, a page comes back short
    // (anything it left out stays unread), a read fails or the ten-page cap
    // is reached; closing the results stops the walk.
    useEffect(() => {
        heading.current?.focus()
        let open = true
        void (async () => {
            let received = 0
            for (let page = 0; page < ATTACK_PAGES && open; page++) {
                const result = await read({ kind: 'attacks', scanId, page, received })
                if (!open) return
                if (typeof result === 'string' || result.kind !== 'attacks') {
                    setFailure(typeof result === 'string' ? result : resultFailure(result))
                    break
                }
                setPages((p) => [...p, result.attacks])
                received += result.attacks.rows.length
                if (result.attacks.complete || result.attacks.rows.length < 100) break
            }
            if (open) setLoading(false)
        })()
        return () => {
            open = false
        }
    }, [context, scanId])

    const rows = pages.flatMap((p) => p.rows)
    const last = pages.at(-1)
    const succeeded = rows.filter((a) => a.threat === true).length
    const capped = !last?.complete && pages.length >= ATTACK_PAGES
    const shortPage = !last?.complete && last !== undefined && last.rows.length < 100
    return (
        <section aria-labelledby={`${ids}-attacks`} className='border-academy-rule rounded-academy-md flex flex-col gap-2 border p-4' data-slot='red-team-attacks'>
            <div className='flex flex-wrap items-center gap-3'>
                <h2 id={`${ids}-attacks`} ref={heading} tabIndex={-1} className='text-[15px] font-semibold'>
                    {`Attack results · scan ${short(scanId)}`}
                </h2>
                <Button type='button' size='sm' tier='tertiary' onClick={onClose}>
                    Close attack results
                </Button>
            </div>
            <p className='text-academy-ink-soft text-[12px]'>Attack prompts and the target's replies stay in the Red Teaming report.</p>
            {failure ? (
                <p role='alert' className='text-[13px]'>
                    {`Attack results could not be read: ${failure}`}
                </p>
            ) : null}
            <p role='status' className='text-[13px]' data-slot='red-team-attack-summary'>
                {loading
                    ? `Reading attack results · ${rows.length.toLocaleString()} read…`
                    : last
                      ? `${succeeded} of ${rows.length} succeeded · read 100 per page${
                            last.complete
                                ? ' · all attacks read'
                                : last.total !== null
                                  ? ` · showing ${rows.length} of ${reported(last.total, last.capped)}`
                                  : ' · more may exist'
                        }${shortPage ? ' · the service returned fewer than it reports' : ''}${capped ? ' · the 1,000-attack cap is reached' : ''}`
                      : ''}
            </p>
            {!loading && last?.complete && rows.length === 0 ? (
                <p className='text-[13px]'>This scan reports no attacks. Static scans list their attacks once they finish.</p>
            ) : null}
            {!loading && last && !last.complete && rows.length === 0 ? (
                <p className='text-[13px]'>No attack results came back, so none are shown; the results could not be read in full.</p>
            ) : null}
            {rows.length > 100 ? (
                <div role='group' aria-label='Attack results shown' className='flex flex-wrap items-center gap-2 text-[13px]'>
                    <Button type='button' size='sm' tier='tertiary' aria-disabled={shown === 0} onClick={() => shown > 0 && setShown(shown - 1)}>
                        Previous 100 attacks
                    </Button>
                    <span>{`Attacks ${shown * 100 + 1}–${Math.min(rows.length, shown * 100 + 100)} of ${rows.length.toLocaleString()} read`}</span>
                    <Button
                        type='button'
                        size='sm'
                        tier='tertiary'
                        aria-disabled={(shown + 1) * 100 >= rows.length}
                        onClick={() => (shown + 1) * 100 < rows.length && setShown(shown + 1)}
                    >
                        Next 100 attacks
                    </Button>
                </div>
            ) : null}
            {rows.length > 0 ? (
                <table className='w-full text-left text-[13px]'>
                    <thead>
                        <tr>
                            <th scope='col'>No.</th>
                            <th scope='col'>Outcome</th>
                            <th scope='col'>Attack</th>
                            <th scope='col'>Severity</th>
                            <th scope='col'>Type</th>
                        </tr>
                    </thead>
                    <tbody>
                        {rows.slice(shown * 100, shown * 100 + 100).map((a) => (
                            <tr key={a.number}>
                                <td>{`#${String(a.number).padStart(3, '0')}`}</td>
                                <td>{a.threat === true ? 'Succeeded' : a.threat === false ? 'Defended' : 'Not rated'}</td>
                                <td>{`${a.subCategory ?? 'Not reported'} · ${a.multiTurn === true ? 'Multi-turn' : a.multiTurn === false ? 'Single turn' : 'Turns unreported'}`}</td>
                                <td>{words(a.severity)}</td>
                                <td>{words(a.attackType)}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            ) : null}
        </section>
    )
}

function ModelsPanel({ held, onRefresh, section }: { held: Held<SupplyChainObservation>; onRefresh: () => void; section?: RedTeamSection }) {
    const ids = useId()
    const overview = !section || section === 'modelOverview'
    const showModels = !section || section === 'modelOverview' || section === 'modelScans'
    const showSkills = !section || section === 'modelOverview' || section === 'skillScans'
    const listOnly = section === 'modelScans' || section === 'skillScans'
    const observation = held.read
    const models = observation?.models ?? null
    const stats = observation?.skillStatistics ?? null
    const skills = observation?.skills ?? null
    const listed = (list: Listed | null) =>
        list === null ? 'Unavailable' : bound(list.total ?? list.rows.length, (list.complete || list.total !== null) && !list.capped)
    const tiles: [string, string][] = [
        [models?.total !== null && models ? 'Model scans (reported)' : 'Model scans (observed)', listed(models)],
        [skills?.total !== null && skills ? 'Skill scans (reported)' : 'Skill scans (observed)', listed(skills)],
        ['Unique skills scanned (30 days)', stats ? stats.uniqueSkills.toLocaleString() : 'Unavailable'],
        ['Skill vulnerabilities (30 days)', stats ? stats.vulnerabilities.toLocaleString() : 'Unavailable'],
    ]
    return (
        <>
            <ReadStatus held={held} what='model observation' extra='current scan inventory and 30-day skill statistics' onRefresh={onRefresh} />
            {observation ? (
                <>
                    {overview ? (
                    <>
                    <Tiles label='Model and skill summary' tiles={tiles} />
                    <div className='flex flex-wrap gap-6'>
                        <Ranked
                            title='Model scan outcomes'
                            bars={models ? tally(models.rows.map((r) => r.outcome)) : null}
                            emptyText='Model Security returned no model scans.'
                        />
                        <Ranked title='Skill scan status' bars={skills ? tally(skills.rows.map((r) => r.status)) : null} emptyText='AgentGuard returned no skill scans.' />
                    </div>
                    <Sources
                        rows={[
                            ['Model scans', coverage(models, observation.modelsFailure)],
                            [
                                'Skill statistics',
                                observation.skillStatisticsFailure ? `Unavailable: ${failureText(observation.skillStatisticsFailure)}` : 'Read · last 30 days',
                            ],
                            ['Skill scans', coverage(skills, observation.skillsFailure)],
                        ]}
                    />
                    </>
                    ) : null}
                    {listOnly || !section ? (
                    <section aria-labelledby={`${ids}-rows`} className='flex flex-col gap-2' data-slot='models-scans' data-section={section ?? 'all'}>
                        <h2 id={`${ids}-rows`} className='text-[15px] font-semibold'>
                            {section === 'modelScans' ? 'Model scans' : section === 'skillScans' ? 'Skill scans' : 'Model and skill scans'}
                        </h2>
                        {section === 'modelScans' ? <p className='text-[12px]'>{`Coverage: ${coverage(models, observation.modelsFailure)}`}</p> : null}
                        {section === 'skillScans' ? <p className='text-[12px]'>{`Coverage: ${coverage(skills, observation.skillsFailure)}`}</p> : null}
                        <p className='text-academy-ink-soft text-[12px]'>
                            At most 100 scans per list. Skills are named by a pseudonym that holds for this read only.
                        </p>
                        {(showModels ? (models?.rows.length ?? 0) : 0) + (showSkills ? (skills?.rows.length ?? 0) : 0) === 0 ? (
                            <p className='text-[13px]'>{section === 'modelScans' ? 'No model scans were read.' : section === 'skillScans' ? 'No skill scans were read.' : 'No model or skill scans were read.'}</p>
                        ) : (
                            <table className='w-full text-left text-[13px]'>
                                <thead>
                                    <tr>
                                        <th scope='col'>Source</th>
                                        <th scope='col'>Subject</th>
                                        <th scope='col'>Result</th>
                                        <th scope='col'>Status</th>
                                        <th scope='col'>Created</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {(showModels ? (models?.rows ?? []) : []).map((r) => (
                                        <tr key={`m-${r.id}`}>
                                            <td>Model Security</td>
                                            <td>{`Model scan ${short(r.id)}`}</td>
                                            <td>{words(r.outcome)}</td>
                                            <td>Not reported</td>
                                            <td>{r.createdAt ?? 'Not reported'}</td>
                                        </tr>
                                    ))}
                                    {(showSkills ? (skills?.rows ?? []) : []).map((r, i) => (
                                        <tr key={`s-${i}-${r.pseudonym}`}>
                                            <td>AgentGuard</td>
                                            <td>{`Skill ${r.pseudonym} (pseudonym for this read)`}</td>
                                            <td>
                                                {r.vulnerabilities === null
                                                    ? 'Vulnerabilities not reported'
                                                    : r.vulnerabilities === 0
                                                      ? 'None found'
                                                      : `${r.vulnerabilities} vulnerabilit${r.vulnerabilities === 1 ? 'y' : 'ies'} found`}
                                            </td>
                                            <td>{words(r.status)}</td>
                                            <td>{r.createdAt ?? 'Not reported'}</td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        )}
                    </section>
                    ) : null}
                </>
            ) : null}
            {overview ? <Unavailable items='Starting model scans and managing security groups or model sources are not available here.' /> : null}
        </>
    )
}

/**
 * Red Team Targets (C31, V2-T21): targets as the scan list carries them,
 * identifiers and types only. Target names are outside the C13 privacy
 * boundary and never read; "Results" opens the target's latest reported scan.
 */
function TargetsTable({ scans, onResults }: { scans: RedTeamObservation['scans']; onResults: (scanId: string) => void }) {
    const ids = useId()
    const targets = new Map<string, { type: string | null; scans: number; reported: string | null }>()
    for (const r of scans?.rows ?? []) {
        const key = r.targetId ?? 'Not reported'
        const held = targets.get(key) ?? { type: r.targetType, scans: 0, reported: null }
        held.scans += 1
        if (!held.reported && REPORTED.includes((r.status ?? '').toUpperCase())) held.reported = r.id
        targets.set(key, held)
    }
    return (
        <section aria-labelledby={`${ids}-targets`} className='flex flex-col gap-2' data-slot='red-team-targets'>
            <h2 id={`${ids}-targets`} className='text-[15px] font-semibold'>
                Targets
            </h2>
            <p className='text-academy-ink-soft text-[12px]'>Identifiers and types from the scans read; target names stay in Prisma AIRS.</p>
            {scans === null ? (
                <p className='text-[13px]'>The scan list could not be read.</p>
            ) : targets.size === 0 ? (
                <p className='text-[13px]'>No targets appear in the scans read.</p>
            ) : (
                <table className='w-full text-left text-[13px]'>
                    <thead>
                        <tr>
                            <th scope='col'>Target</th>
                            <th scope='col'>Type</th>
                            <th scope='col'>Scans read</th>
                            <th scope='col'>
                                <span className='sr-only'>Actions</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        {[...targets].map(([id, t]) => (
                            <tr key={id}>
                                <td className='font-mono'>{id === 'Not reported' ? id : short(id)}</td>
                                <td>{words(t.type)}</td>
                                <td>{t.scans.toLocaleString()}</td>
                                <td>
                                    {t.reported ? (
                                        <Button type='button' size='sm' tier='tertiary' onClick={() => onResults(t.reported!)}>
                                            {`Results for target ${id === 'Not reported' ? id : short(id)}`}
                                        </Button>
                                    ) : null}
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            )}
        </section>
    )
}

/** The reported scans (completed, passed or failed) as metadata rows, each opening its attack results. */
function ReportedScans({ scans, title, note, action, onOpen }: { scans: RedTeamObservation['scans']; title: string; note: string; action: string; onOpen: (scanId: string) => void }) {
    const ids = useId()
    const rows = (scans?.rows ?? []).filter((r) => REPORTED.includes((r.status ?? '').toUpperCase()))
    return (
        <section aria-labelledby={`${ids}-reported`} className='flex flex-col gap-2' data-slot='red-team-reported' data-title={title}>
            <h2 id={`${ids}-reported`} className='text-[15px] font-semibold'>
                {title}
            </h2>
            <p className='text-academy-ink-soft text-[12px]'>{note}</p>
            {scans === null ? (
                <p className='text-[13px]'>The scan list could not be read.</p>
            ) : rows.length === 0 ? (
                <p className='text-[13px]'>No reported scans in the rows read.</p>
            ) : (
                <table className='w-full text-left text-[13px]'>
                    <thead>
                        <tr>
                            <th scope='col'>Scan</th>
                            <th scope='col'>Status</th>
                            <th scope='col'>Type</th>
                            <th scope='col'>Created</th>
                            <th scope='col'>
                                <span className='sr-only'>Actions</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        {rows.map((r) => (
                            <tr key={r.id}>
                                <td className='font-mono'>{short(r.id)}</td>
                                <td>{words(r.status)}</td>
                                <td>{words(r.jobType)}</td>
                                <td>{r.createdAt ?? 'Not reported'}</td>
                                <td>
                                    <Button type='button' size='sm' tier='tertiary' onClick={() => onOpen(r.id)}>
                                        {`${action} for scan ${short(r.id)}`}
                                    </Button>
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            )}
        </section>
    )
}
