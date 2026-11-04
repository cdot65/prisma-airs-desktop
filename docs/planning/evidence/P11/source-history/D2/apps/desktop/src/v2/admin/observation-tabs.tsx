import { useEffect, useId, useRef, useState } from 'react'
import type { AttackPage, Bar, ContextRef, ManagementFailure, ManagementRequest, ManagementResult, RedTeamObservation, ShellSnapshot, SupplyChainObservation } from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, readManagement } from '../../modules/administration/management-read'
import type { Dispatch } from '../../modules/tenants/dispatch'
import { Btn } from '../controls'

export const ATTACK_PAGES = 10
const WINDOWS = [7, 15, 30] as const
type Window = (typeof WINDOWS)[number]

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

/** A sum of status bars, unknown when the breakdown was not reported (unless the tenant has no scans at all). */
export function statusSum(bars: Bar[] | null, scans: number, labels: string[]): number | null {
    if (bars === null) return scans === 0 ? 0 : null
    return bars.filter((b) => labels.includes(b.label)).reduce((n, b) => n + b.count, 0)
}
function tally(values: (string | null)[]): Bar[] {
    const counts = new Map<string, number>()
    for (const v of values) counts.set(words(v), (counts.get(words(v)) ?? 0) + 1)
    return [...counts].map(([label, count]) => ({ label, count })).sort((a, b) => b.count - a.count)
}
const failureText = (failure: ManagementFailure) => MANAGEMENT_FAILURE_TEXT[failure]
type Listed = { rows: unknown[]; total: number | null; capped: boolean; complete: boolean }
const reported = (total: number, capped: boolean) => `${capped ? '≥ ' : ''}${total.toLocaleString()}`
function coverage(list: Listed | null, failure: ManagementFailure | null) {
    if (failure) return `Unavailable: ${failureText(failure)}`
    if (!list) return 'Not read'
    if (list.complete) return `${list.rows.length.toLocaleString()} read · complete`
    return list.total !== null ? `Partial sample · ${list.rows.length} of ${reported(list.total, list.capped)}` : `Partial sample · ${list.rows.length} observed`
}
const bound = (n: number, complete: boolean) => (complete ? n.toLocaleString() : `≥ ${n.toLocaleString()}`)
const resultFailure = (result: ManagementResult) => (result.kind === 'failed' ? failureText(result.failure) : MANAGEMENT_FAILURE_TEXT.invalidResponse)

const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
const cardStyle = { borderColor: 'var(--v2-rule)' }

export type RedTeamTabId = 'overview' | 'targets' | 'scans' | 'attack-results' | 'reports'
export type ModelTabId = 'overview' | 'model-scans' | 'skill-scans'

interface Props {
    snapshot: ShellSnapshot
    context: ContextRef
    dispatch: Dispatch
    observe: ObserveManagement
    product: 'redTeam' | 'modelSkill'
    tab: string
}

/**
 * AI Red Teaming and Model and skill scans as R2App's product cards draw
 * them (C39b), over the C13 read-only observation: counts, statuses and
 * outcomes only. Scan, target, model and skill names, model URIs, findings
 * and attack prompts stay in Prisma AIRS; skills appear by a pseudonym for
 * one read. Each source states whether it is complete, partial or
 * unavailable and never passes for zero. Nothing here starts a scan.
 */
export function ObservationProduct({ snapshot, context, dispatch, observe, product, tab }: Props) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const ready = tenant?.management.state === 'ready'
    const [range, setRange] = useState<Window>(30)
    const [redTeam, setRedTeam] = useState<Held<RedTeamObservation>>(empty)
    const [models, setModels] = useState<Held<SupplyChainObservation>>(empty)
    const tokens = useRef({ redTeam: 0, models: 0 })
    async function read(request: ManagementRequest): Promise<ManagementResult | string> {
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
    useEffect(() => {
        if (!ready) return
        if (product === 'redTeam' && !redTeam.read && !redTeam.loading && !redTeam.failure) void readRedTeam(range)
        if (product === 'modelSkill' && !models.read && !models.loading && !models.failure) void readModels()
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per product shown
    }, [product, ready])
    if (!tenant) return null
    if (!ready) {
        return (
            <p className='text-[13px]' style={soft} data-slot='red-team-not-configured'>
                Red Teaming and model observation use the tenant's management client. Configure one in Settings to see them.
            </p>
        )
    }
    return product === 'redTeam' ? (
        <RedTeamPanel held={redTeam} range={range} onRange={(d) => { setRange(d); void readRedTeam(d) }} onRefresh={() => void readRedTeam(range)} context={context} read={read} section={tab as RedTeamTabId} />
    ) : (
        <ModelsPanel held={models} onRefresh={() => void readModels()} section={tab as ModelTabId} />
    )
}

function ReadStatus<T extends { readAt: number }>({ held, what, extra, onRefresh }: { held: Held<T>; what: string; extra: string; onRefresh: () => void }) {
    const status = held.loading ? `Reading ${what}…` : held.read ? `Updated ${clock(held.read.readAt)} · ${extra}` : 'Not read yet'
    return (
        <>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <p role='status' aria-live='polite' className='text-[12px]' style={soft} data-slot='red-team-status'>
                    {status}
                </p>
                <Btn tier='tertiary' aria-disabled={held.loading} onClick={() => !held.loading && onRefresh()}>
                    {`Refresh ${what}`}
                </Btn>
            </div>
            {held.failure ? (
                <div role='alert' className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-flag-bg)' }} data-slot='red-team-failure'>
                    <p>The last read failed: {held.failure}</p>
                    {held.read ? <p className='mt-1 text-[12px]'>Showing what was read at {clock(held.read.readAt)}. It may be out of date.</p> : null}
                </div>
            ) : null}
        </>
    )
}

function Tiles({ label, tiles }: { label: string; tiles: [string, string][] }) {
    return (
        <dl aria-label={label} className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 150px), 1fr))', gap: 16 }} data-slot='red-team-tiles'>
            {tiles.map(([name, value]) => (
                <div key={name} className='rounded-[12px]' style={{ padding: '16px 18px', background: 'var(--v2-inset)' }}>
                    <dt className='text-[12px]' style={faint}>
                        {name}
                    </dt>
                    <dd className='text-[22px] font-bold tabular-nums'>{value}</dd>
                </div>
            ))}
        </dl>
    )
}

function Ranked({ title, bars, emptyText }: { title: string; bars: Bar[] | null; emptyText: string }) {
    const id = useId()
    const max = Math.max(1, ...(bars ?? []).map((b) => b.count))
    return (
        <section aria-labelledby={id} className='flex min-w-56 flex-1 flex-col' style={{ gap: 6 }}>
            <h3 id={id} className='text-[13px] font-semibold'>
                {title}
            </h3>
            {bars === null ? (
                <p className='text-[13px]' style={soft}>
                    Not reported by the service.
                </p>
            ) : bars.length === 0 ? (
                <p className='text-[13px]'>{emptyText}</p>
            ) : (
                <ul className='flex flex-col text-[12px]' style={{ gap: 4 }}>
                    {[...bars]
                        .sort((a, b) => b.count - a.count)
                        .map((b) => (
                            <li key={b.label} className='flex items-center' style={{ gap: 8 }}>
                                <span className='w-28 shrink-0 truncate'>{words(b.label)}</span>
                                <span className='h-2 rounded-[2px]' style={{ width: `${Math.max(4, Math.round((b.count / max) * 100))}%`, maxWidth: 160, background: 'var(--v2-acc)' }} aria-hidden='true' />
                                <span className='tabular-nums'>{b.count.toLocaleString()}</span>
                            </li>
                        ))}
                </ul>
            )}
        </section>
    )
}

function Sources({ rows }: { rows: [string, string][] }) {
    return (
        <div data-slot='red-team-sources' className='text-[12px]' style={soft}>
            <h3 className='text-[12px] font-semibold' style={faint}>
                Source coverage
            </h3>
            <ul>
                {rows.map(([name, state]) => (
                    <li key={name}>{`${name} · ${state}`}</li>
                ))}
            </ul>
        </div>
    )
}

function Unavailable({ items }: { items: string }) {
    return (
        <div className='rounded-[8px] text-[12px]' style={{ padding: '10px 12px', background: 'var(--v2-inset)', ...soft, maxWidth: 860 }} data-slot='red-team-unavailable'>
            <p className='font-semibold'>Not available in the desktop</p>
            <p>{items} Use Prisma AIRS for them; nothing here starts a scan or changes the tenant.</p>
        </div>
    )
}

function Table({ label, columns, grid, rows, slot }: { label: string; columns: string[]; grid: string; rows: { key: string; cells: React.ReactNode[] }[]; slot?: string }) {
    return (
        <div className='overflow-hidden rounded-[12px] border' style={cardStyle}>
            <div role='table' aria-label={label} data-slot={slot}>
                <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: grid, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                    {columns.map((c, i) => (
                        <span key={i} role='columnheader'>
                            {c === '' ? <span className='sr-only'>Actions</span> : c}
                        </span>
                    ))}
                </div>
                {rows.map((r) => (
                    <div key={r.key} role='row' className='grid items-center text-[12px]' style={{ gridTemplateColumns: grid, gap: 12, padding: '10px 18px', borderTop: '1px solid var(--v2-rule)' }}>
                        {r.cells.map((c, i) => (
                            <span key={i} role='cell' className='min-w-0 truncate' data-label={columns[i] || undefined}>
                                {c}
                            </span>
                        ))}
                    </div>
                ))}
            </div>
        </div>
    )
}

function RedTeamPanel({ held, range, onRange, onRefresh, context, read, section }: { held: Held<RedTeamObservation>; range: Window; onRange: (days: Window) => void; onRefresh: () => void; context: ContextRef; read: (request: ManagementRequest) => Promise<ManagementResult | string>; section: RedTeamTabId }) {
    const ids = useId()
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
        <div className='flex flex-col' style={{ gap: 14 }}>
            <ReadStatus held={held} what='Red Teaming' extra={`last ${observation?.window ?? range} days`} onRefresh={onRefresh} />
            {section === 'overview' ? (
                <fieldset className='flex flex-wrap items-center text-[13px]' style={{ gap: 12 }}>
                    <legend className='text-[12px]' style={soft}>
                        Statistics window
                    </legend>
                    {WINDOWS.map((days) => (
                        <label key={days} className='flex items-center' style={{ gap: 4 }}>
                            <input type='radio' name={`${ids}-window`} checked={range === days} onChange={() => onRange(days)} className='size-4 min-h-0' />
                            {`Last ${days} days`}
                        </label>
                    ))}
                </fieldset>
            ) : null}
            {observation ? (
                <>
                    {section === 'overview' ? (
                        <>
                            <Tiles label='Red Teaming summary' tiles={tiles} />
                            {stats?.noScans ? <p className='text-[13px]'>No scans in this tenant yet. Red Teaming scans run in Prisma AIRS.</p> : null}
                            <div className='flex flex-wrap' style={{ gap: 24 }}>
                                <Ranked title='Scans by status' bars={stats?.status ?? null} emptyText='Red Teaming returned no scans by status for this period.' />
                                <Ranked title='Targets by type' bars={stats?.targetTypes ?? null} emptyText='Red Teaming returned no targets by type for this period.' />
                                <Ranked title='Risk profile' bars={stats?.risk ?? null} emptyText='Red Teaming returned no risk ratings for this period.' />
                            </div>
                            <Sources rows={[['Statistics', observation.statisticsFailure ? `Unavailable: ${failureText(observation.statisticsFailure)}` : 'Read'], ['Scans', coverage(scans, observation.scansFailure)]]} />
                        </>
                    ) : null}
                    {section !== 'overview' ? (
                        // Every tab drawn from the scan list says what that list is: read in full, a partial or capped
                        // sample of a reported total, a sample of an unknown total, or unavailable (C39b attempt-01 R3).
                        <Sources rows={[['Scans', coverage(scans, observation.scansFailure)]]} />
                    ) : null}
                    {section === 'targets' ? <TargetsTable scans={scans} onResults={setAttacksOf} /> : null}
                    {section === 'attack-results' ? <ReportedScans scans={scans} title='Attack results' note='Attack results are read for one reported scan at a time, at most ten pages.' action='Open attack results' onOpen={setAttacksOf} /> : null}
                    {section === 'reports' ? <ReportedScans scans={scans} title='Reports' note='Reports are listed from scan metadata only; nothing is generated, exported or run here.' action='Open the report scan' onOpen={setAttacksOf} /> : null}
                    {section === 'scans' ? (
                        <section aria-labelledby={`${ids}-scans`} className='flex flex-col' style={{ gap: 10 }} data-slot='red-team-scans'>
                            <h2 id={`${ids}-scans`} className='text-[13px] font-semibold'>
                                Red Teaming scans
                            </h2>
                            <p className='text-[12px]' style={soft}>
                                Each list is read once, at most 100 scans; the filter applies to those rows.
                            </p>
                            <label className='flex items-center text-[12px]' style={{ gap: 8, ...soft }}>
                                Status
                                <select value={status} onChange={(e) => setStatus(e.target.value)} className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px]' style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
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
                                <p className='text-[13px]'>{scans.complete ? 'No scans in the rows read match this filter.' : 'None of the scans read match. The list could not be read in full; the Scans source line above says what was read.'}</p>
                            ) : (
                                <Table
                                    label='Red Teaming scans'
                                    columns={['Status', 'Scan', 'Type', 'Target', 'Created', '']}
                                    grid='minmax(0, 0.8fr) minmax(0, 0.8fr) minmax(0, 0.7fr) minmax(0, 1.2fr) minmax(0, 1fr) auto'
                                    rows={rows.map((r) => ({
                                        key: r.id,
                                        cells: [
                                            words(r.status),
                                            <span className='font-mono'>{short(r.id)}</span>,
                                            words(r.jobType),
                                            `${words(r.targetType)} · ${short(r.targetId)}`,
                                            r.createdAt ?? 'Not reported',
                                            REPORTED.includes((r.status ?? '').toUpperCase()) ? (
                                                <Btn tier='tertiary' className='text-[12px]' onClick={() => setAttacksOf(r.id)}>
                                                    {`Open attack results for scan ${short(r.id)}`}
                                                </Btn>
                                            ) : null,
                                        ],
                                    }))}
                                />
                            )}
                        </section>
                    ) : null}
                    {attacksOf ? <AttackResults key={attacksOf} scanId={attacksOf} context={context} read={read} onClose={() => setAttacksOf(null)} /> : null}
                </>
            ) : null}
            {section === 'overview' ? <Unavailable items='Starting or re-running Red Teaming scans, verifying ASR, and creating targets, custom adapters, network channels or prompt sets are not available here.' /> : null}
        </div>
    )
}

function AttackResults({ scanId, context, read, onClose }: { scanId: string; context: ContextRef; read: (request: ManagementRequest) => Promise<ManagementResult | string>; onClose: () => void }) {
    const ids = useId()
    const [pages, setPages] = useState<AttackPage[]>([])
    const [loading, setLoading] = useState(true)
    const [failure, setFailure] = useState<string | null>(null)
    const [shown, setShown] = useState(0)
    const heading = useRef<HTMLHeadingElement>(null)
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
        // eslint-disable-next-line react-hooks/exhaustive-deps -- one walk per scan
    }, [context, scanId])
    const rows = pages.flatMap((p) => p.rows)
    const last = pages.at(-1)
    const succeeded = rows.filter((a) => a.threat === true).length
    const capped = !last?.complete && pages.length >= ATTACK_PAGES
    const shortPage = !last?.complete && last !== undefined && last.rows.length < 100
    return (
        <section aria-labelledby={`${ids}-attacks`} className='flex flex-col rounded-[12px] border' style={{ gap: 10, padding: 18, ...cardStyle, background: 'var(--v2-paper)' }} data-slot='red-team-attacks'>
            <div className='flex flex-wrap items-center' style={{ gap: 10 }}>
                <h2 id={`${ids}-attacks`} ref={heading} tabIndex={-1} className='text-[13px] font-semibold outline-none'>
                    {`Attack results · scan ${short(scanId)}`}
                </h2>
                <Btn tier='tertiary' onClick={onClose}>
                    Close attack results
                </Btn>
            </div>
            <p className='text-[12px]' style={soft}>
                Attack prompts and the target's replies stay in the Red Teaming report.
            </p>
            {failure ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>{`Attack results could not be read: ${failure}`}</p>
            ) : null}
            <p role='status' className='text-[13px]' data-slot='red-team-attack-summary'>
                {loading
                    ? `Reading attack results · ${rows.length.toLocaleString()} read…`
                    : last
                      ? `${succeeded} of ${rows.length} succeeded · read 100 per page${last.complete ? ' · all attacks read' : last.total !== null ? ` · showing ${rows.length} of ${reported(last.total, last.capped)}` : ' · more may exist'}${shortPage ? ' · the service returned fewer than it reports' : ''}${capped ? ' · the 1,000-attack cap is reached' : ''}`
                      : ''}
            </p>
            {!loading && last?.complete && rows.length === 0 ? <p className='text-[13px]'>This scan reports no attacks. Static scans list their attacks once they finish.</p> : null}
            {!loading && last && !last.complete && rows.length === 0 ? <p className='text-[13px]'>No attack results came back, so none are shown; the results could not be read in full.</p> : null}
            {rows.length > 100 ? (
                <div role='group' aria-label='Attack results shown' className='flex flex-wrap items-center text-[13px]' style={{ gap: 8 }}>
                    <Btn tier='tertiary' aria-disabled={shown === 0} onClick={() => shown > 0 && setShown(shown - 1)}>
                        Previous 100 attacks
                    </Btn>
                    <span>{`Attacks ${shown * 100 + 1}–${Math.min(rows.length, shown * 100 + 100)} of ${rows.length.toLocaleString()} read`}</span>
                    <Btn tier='tertiary' aria-disabled={(shown + 1) * 100 >= rows.length} onClick={() => (shown + 1) * 100 < rows.length && setShown(shown + 1)}>
                        Next 100 attacks
                    </Btn>
                </div>
            ) : null}
            {rows.length > 0 ? (
                <Table
                    label='Attack results'
                    columns={['No.', 'Outcome', 'Attack', 'Severity', 'Type']}
                    grid='minmax(0, 0.4fr) minmax(0, 0.7fr) minmax(0, 1.6fr) minmax(0, 0.7fr) minmax(0, 0.8fr)'
                    rows={rows.slice(shown * 100, shown * 100 + 100).map((a) => ({
                        key: String(a.number),
                        cells: [`#${String(a.number).padStart(3, '0')}`, a.threat === true ? 'Succeeded' : a.threat === false ? 'Defended' : 'Not rated', `${a.subCategory ?? 'Not reported'} · ${a.multiTurn === true ? 'Multi-turn' : a.multiTurn === false ? 'Single turn' : 'Turns unreported'}`, words(a.severity), words(a.attackType)],
                    }))}
                />
            ) : null}
        </section>
    )
}

function ModelsPanel({ held, onRefresh, section }: { held: Held<SupplyChainObservation>; onRefresh: () => void; section: ModelTabId }) {
    const ids = useId()
    const overview = section === 'overview'
    const showModels = section === 'overview' || section === 'model-scans'
    const showSkills = section === 'overview' || section === 'skill-scans'
    const listOnly = section === 'model-scans' || section === 'skill-scans'
    const observation = held.read
    const models = observation?.models ?? null
    const stats = observation?.skillStatistics ?? null
    const skills = observation?.skills ?? null
    const listed = (list: Listed | null) => (list === null ? 'Unavailable' : bound(list.total ?? list.rows.length, (list.complete || list.total !== null) && !list.capped))
    const tiles: [string, string][] = [
        [models?.total !== null && models ? 'Model scans (reported)' : 'Model scans (observed)', listed(models)],
        [skills?.total !== null && skills ? 'Skill scans (reported)' : 'Skill scans (observed)', listed(skills)],
        ['Unique skills scanned (30 days)', stats ? stats.uniqueSkills.toLocaleString() : 'Unavailable'],
        ['Skill vulnerabilities (30 days)', stats ? stats.vulnerabilities.toLocaleString() : 'Unavailable'],
    ]
    const modelRows = showModels ? (models?.rows ?? []) : []
    const skillRows = showSkills ? (skills?.rows ?? []) : []
    return (
        <div className='flex flex-col' style={{ gap: 14 }}>
            <ReadStatus held={held} what='model observation' extra='current scan inventory and 30-day skill statistics' onRefresh={onRefresh} />
            {observation ? (
                <>
                    {overview ? (
                        <>
                            <Tiles label='Model and skill summary' tiles={tiles} />
                            <div className='flex flex-wrap' style={{ gap: 24 }}>
                                <Ranked title='Model scan outcomes' bars={models ? tally(models.rows.map((r) => r.outcome)) : null} emptyText='Model Security returned no model scans.' />
                                <Ranked title='Skill scan status' bars={skills ? tally(skills.rows.map((r) => r.status)) : null} emptyText='AgentGuard returned no skill scans.' />
                            </div>
                            <Sources rows={[['Model scans', coverage(models, observation.modelsFailure)], ['Skill statistics', observation.skillStatisticsFailure ? `Unavailable: ${failureText(observation.skillStatisticsFailure)}` : 'Read · last 30 days'], ['Skill scans', coverage(skills, observation.skillsFailure)]]} />
                        </>
                    ) : null}
                    {listOnly ? (
                        <section aria-labelledby={`${ids}-rows`} className='flex flex-col' style={{ gap: 10 }} data-slot='models-scans' data-section={section === 'model-scans' ? 'modelScans' : 'skillScans'}>
                            <h2 id={`${ids}-rows`} className='text-[13px] font-semibold'>
                                {section === 'model-scans' ? 'Model scans' : 'Skill scans'}
                            </h2>
                            <p className='text-[12px]' style={soft}>{`Coverage: ${section === 'model-scans' ? coverage(models, observation.modelsFailure) : coverage(skills, observation.skillsFailure)}`}</p>
                            <p className='text-[12px]' style={soft}>
                                At most 100 scans per list. Skills are named by a pseudonym that holds for this read only.
                            </p>
                            {modelRows.length + skillRows.length === 0 ? (
                                <p className='text-[13px]'>{section === 'model-scans' ? 'No model scans were read.' : 'No skill scans were read.'}</p>
                            ) : (
                                <Table
                                    label={section === 'model-scans' ? 'Model scans' : 'Skill scans'}
                                    columns={['Source', 'Subject', 'Result', 'Status', 'Created']}
                                    grid='minmax(0, 0.8fr) minmax(0, 1.6fr) minmax(0, 1fr) minmax(0, 0.8fr) minmax(0, 1fr)'
                                    rows={[
                                        ...modelRows.map((r) => ({ key: `m-${r.id}`, cells: ['Model Security', `Model scan ${short(r.id)}`, words(r.outcome), 'Not reported', r.createdAt ?? 'Not reported'] })),
                                        ...skillRows.map((r, i) => ({ key: `s-${i}-${r.pseudonym}`, cells: ['AgentGuard', `Skill ${r.pseudonym} (pseudonym for this read)`, r.vulnerabilities === null ? 'Vulnerabilities not reported' : r.vulnerabilities === 0 ? 'None found' : `${r.vulnerabilities} vulnerabilit${r.vulnerabilities === 1 ? 'y' : 'ies'} found`, words(r.status), r.createdAt ?? 'Not reported'] })),
                                    ]}
                                />
                            )}
                        </section>
                    ) : null}
                </>
            ) : null}
            {overview ? <Unavailable items='Starting model scans and managing security groups or model sources are not available here.' /> : null}
        </div>
    )
}

/** Red Team Targets (V2-T21): targets as the scan list carries them, identifiers and types only. */
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
        <section aria-labelledby={`${ids}-targets`} className='flex flex-col' style={{ gap: 10 }} data-slot='red-team-targets'>
            <h2 id={`${ids}-targets`} className='text-[13px] font-semibold'>
                Targets
            </h2>
            <p className='text-[12px]' style={soft}>
                Identifiers and types from the scans read; target names stay in Prisma AIRS.
            </p>
            {scans === null ? (
                <p className='text-[13px]'>The scan list could not be read.</p>
            ) : targets.size === 0 ? (
                <p className='text-[13px]'>No targets appear in the scans read.</p>
            ) : (
                <Table
                    label='Targets'
                    columns={['Target', 'Type', 'Scans read', '']}
                    grid='minmax(0, 1fr) minmax(0, 1fr) minmax(0, 0.6fr) auto'
                    rows={[...targets].map(([id, t]) => ({
                        key: id,
                        cells: [
                            <span className='font-mono'>{id === 'Not reported' ? id : short(id)}</span>,
                            words(t.type),
                            t.scans.toLocaleString(),
                            t.reported ? (
                                <Btn tier='tertiary' className='text-[12px]' onClick={() => onResults(t.reported!)}>
                                    {`Results for target ${id === 'Not reported' ? id : short(id)}`}
                                </Btn>
                            ) : null,
                        ],
                    }))}
                />
            )}
        </section>
    )
}

function ReportedScans({ scans, title, note, action, onOpen }: { scans: RedTeamObservation['scans']; title: string; note: string; action: string; onOpen: (scanId: string) => void }) {
    const ids = useId()
    const rows = (scans?.rows ?? []).filter((r) => REPORTED.includes((r.status ?? '').toUpperCase()))
    return (
        <section aria-labelledby={`${ids}-reported`} className='flex flex-col' style={{ gap: 10 }} data-slot='red-team-reported' data-title={title}>
            <h2 id={`${ids}-reported`} className='text-[13px] font-semibold'>
                {title}
            </h2>
            <p className='text-[12px]' style={soft}>
                {note}
            </p>
            {scans === null ? (
                <p className='text-[13px]'>The scan list could not be read.</p>
            ) : rows.length === 0 ? (
                <p className='text-[13px]'>No reported scans in the rows read.</p>
            ) : (
                <Table
                    label={title}
                    columns={['Scan', 'Status', 'Type', 'Created', '']}
                    grid='minmax(0, 0.8fr) minmax(0, 0.8fr) minmax(0, 0.8fr) minmax(0, 1fr) auto'
                    rows={rows.map((r) => ({
                        key: r.id,
                        cells: [
                            <span className='font-mono'>{short(r.id)}</span>,
                            words(r.status),
                            words(r.jobType),
                            r.createdAt ?? 'Not reported',
                            <Btn tier='tertiary' className='text-[12px]' onClick={() => onOpen(r.id)}>
                                {`${action} for scan ${short(r.id)}`}
                            </Btn>,
                        ],
                    }))}
                />
            )}
        </section>
    )
}
