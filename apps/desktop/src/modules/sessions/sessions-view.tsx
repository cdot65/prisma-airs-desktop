import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout, Input, cn } from '@ai-security-academy/presentation'
import { AlertTriangle, Eye, EyeOff, FlaskConical, Info, Lock } from 'lucide-react'
import type {
    ContextRef,
    FeatureId,
    ManagementPage,
    Report,
    ReportKind,
    ReportProblem,
    ScanContent,
    SessionSearch,
    SessionsView as SessionsSnapshot,
    ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { MANAGEMENT_FAILURE_TEXT, type ObserveManagement, fieldText, readManagement } from '../administration/management-read'
import {
    CATEGORIES,
    KIND_TEXT,
    type Row,
    VERDICT_TEXT,
    type Verdict,
    appId,
    appName,
    coverage,
    detectors,
    scanId,
    sessionId,
    subRequest,
    suggestedKind,
    verdictOf,
} from './sessions-model'

/** Sessions per inventory page (v1). */
export const PAGE_SIZE = 25

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
    onNavigate: (id: FeatureId) => void
}

type Tab = 'sessions' | 'reports' | 'exceptions'

interface Shown {
    page: ManagementPage
    rows: Row[]
    readAt: Date
}

type SessionState =
    | { kind: 'loading'; row: Row }
    | { kind: 'shown'; row: Row; session: Row; actions: Row[]; more: boolean }
    | { kind: 'failed'; row: Row; message: string }

type SearchState =
    | { kind: 'searching'; query: string }
    | { kind: 'shown'; search: SessionSearch; rows: Row[]; scan: Row | null }
    | { kind: 'failed'; message: string }

/** What a revealed scan shows; only ever held while Sessions is open. */
type Revealed = { kind: 'loading' } | { kind: 'shown'; content: ScanContent } | { kind: 'failed'; message: string }

const TEXTAREA = cn(
    'rounded-academy-md bg-academy-bg-inset border-academy-rule focus-visible:border-academy-accent-ring w-full border px-3.5 py-2.5',
    'text-academy-ink min-h-24 resize-y text-[13px] leading-snug'
)

const time = (date: Date) => date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
const scanKey = (row: Row) => `${scanId(row)}\u001f${subRequest(row)}`

export const REPORT_PROBLEM_TEXT: Record<ReportProblem, string> = {
    reason: 'Say why this verdict is wrong (up to 2,000 characters).',
    field: 'The report has a value the desktop cannot keep. Choose one of the listed categories.',
    detectors: 'The scan names more detectors than a report can keep.',
    full: 'This tenant already has 2,000 reports. Remove some before adding more.',
    verdict: 'The scan’s verdict could not be kept.',
    scan: 'Open the scan’s session first; a report is built from the scan as the service returned it.',
}

/**
 * Sessions investigations and metadata-only FP/FN reports (C11). The
 * inventory is the last 24 hours in pages of 25; searches walk v1's bounded
 * windows and say what they covered. A scan's prompt and response are read
 * only when the operator reveals them and are dropped when Sessions closes;
 * reports keep the scan's metadata and the operator's reason, never content,
 * and write nothing to the tenant.
 */
export const SessionsView = forwardRef<HTMLHeadingElement, Props>(function SessionsView(
    { snapshot, dispatch, observe, onNavigate },
    ref
) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const context: ContextRef | null = snapshot.active?.context ?? null
    const ready = tenant?.management.state === 'ready'
    const [tab, setTab] = useState<Tab>('sessions')
    // The tenant's reports open with Sessions, so a scan can be reported
    // straight from its session. They are local work: existing reports stay
    // usable without a management client (only new reports need one).
    useEffect(() => {
        if (context) void dispatch({ kind: 'openReports', context })
    }, [context])
    const heading = <PageHeading ref={ref} id='sessions-heading' kicker='Sessions & Reports' title='Investigate sessions' />

    if (!tenant || !context) {
        return (
            <section aria-labelledby='sessions-heading' data-slot='sessions'>
                {heading}
                <Callout tone='default' icon={Info} data-slot='sessions-no-tenant'>
                    <p>Add a tenant in Settings to investigate its sessions.</p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            </section>
        )
    }
    const tabs: { id: Tab; label: string }[] = [
        { id: 'sessions', label: 'Sessions' },
        { id: 'reports', label: `FP/FN reports${snapshot.sessions.reports.items.length > 0 ? ` (${snapshot.sessions.reports.items.length})` : ''}` },
        { id: 'exceptions', label: 'Exception patterns' },
    ]
    return (
        <section aria-labelledby='sessions-heading' data-slot='sessions' className='flex flex-col gap-5'>
            {heading}
            <p className='text-academy-ink-soft -mt-4 text-[13px]'>
                {ready
                    ? `${tenant.label}, read with its management client. Prompts and responses stay hidden until you reveal them.`
                    : `${tenant.label}. Its FP/FN reports are kept on this Mac and stay available without a management client.`}
            </p>
            <div role='tablist' aria-label='Sessions sections' className='flex flex-wrap gap-2'>
                {tabs.map((t) => (
                    <Button
                        key={t.id}
                        type='button'
                        role='tab'
                        aria-selected={tab === t.id}
                        tier={tab === t.id ? 'secondary' : 'tertiary'}
                        size='sm'
                        className={tab === t.id ? 'bg-academy-paper-deep font-semibold' : undefined}
                        onClick={() => setTab(t.id)}
                    >
                        {t.label}
                    </Button>
                ))}
            </div>
            {tab === 'sessions' && !ready ? (
                <Callout tone='default' icon={Lock} data-slot='sessions-not-configured'>
                    <p>
                        {tenant.label} has no ready management client. Sessions are read with a separate OAuth client;
                        scanning keeps working without one.
                    </p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            ) : tab === 'sessions' ? (
                <Investigation context={context} sessions={snapshot.sessions} dispatch={dispatch} observe={observe} />
            ) : tab === 'reports' ? (
                <Reports context={context} sessions={snapshot.sessions} dispatch={dispatch} />
            ) : (
                <ExceptionPreview />
            )}
        </section>
    )
})

function Investigation({
    context,
    sessions,
    dispatch,
    observe,
}: {
    context: ContextRef
    sessions: SessionsSnapshot
    dispatch: Dispatch
    observe: ObserveManagement
}) {
    const ids = useId()
    const [shown, setShown] = useState<Shown | null>(null)
    const [failure, setFailure] = useState<string | null>(null)
    const [loading, setLoading] = useState(false)
    const [application, setApplication] = useState('')
    const [verdict, setVerdict] = useState<'all' | Verdict>('all')
    const [detector, setDetector] = useState('')
    const [query, setQuery] = useState('')
    const [search, setSearch] = useState<SearchState | null>(null)
    const [open, setOpen] = useState<SessionState | null>(null)
    const [details, setDetails] = useState<Record<string, string>>({})
    const [revealed, setRevealed] = useState<Record<string, Revealed>>({})
    const [reporting, setReporting] = useState<Row | null>(null)
    const token = useRef(0)
    const searchToken = useRef(0)
    const sessionToken = useRef(0)

    async function load(offset: number) {
        const mine = ++token.current
        setLoading(true)
        const outcome = await readManagement(dispatch, observe, context, { kind: 'page', resource: 'sessions', offset, limit: PAGE_SIZE })
        if (mine !== token.current) return
        setLoading(false)
        if (outcome.kind === 'notAdmitted') return setFailure(outcome.message)
        const result = outcome.result
        if (result.kind === 'page' && result.page.resource === 'sessions' && result.page.offset === offset) {
            setShown({ page: result.page, rows: JSON.parse(result.page.items) as Row[], readAt: new Date() })
            setFailure(null)
        } else setFailure(MANAGEMENT_FAILURE_TEXT[result.kind === 'failed' ? result.failure : 'invalidResponse'])
    }

    useEffect(() => {
        void load(0)
    }, [])

    async function runSearch() {
        const trimmed = query.trim()
        if (trimmed.length === 0) return
        const mine = ++searchToken.current
        setSearch({ kind: 'searching', query: trimmed })
        const outcome = await readManagement(dispatch, observe, context, { kind: 'search', query: trimmed })
        if (mine !== searchToken.current) return
        if (outcome.kind === 'notAdmitted') {
            setSearch({
                kind: 'failed',
                message: outcome.message.startsWith('Check the entered')
                    ? 'Search by a session ID (ses…), a scan ID (scn… or a UUID) or a detector code such as prompt_injection.'
                    : outcome.message,
            })
        } else if (outcome.result.kind === 'search') {
            const found = outcome.result.search
            setSearch({ kind: 'shown', search: found, rows: JSON.parse(found.sessions) as Row[], scan: found.scan ? (JSON.parse(found.scan) as Row) : null })
        } else {
            setSearch({ kind: 'failed', message: MANAGEMENT_FAILURE_TEXT[outcome.result.kind === 'failed' ? outcome.result.failure : 'invalidResponse'] })
        }
    }

    async function openSession(row: Row) {
        const mine = ++sessionToken.current
        setOpen({ kind: 'loading', row })
        setReporting(null)
        const outcome = await readManagement(dispatch, observe, context, {
            kind: 'session',
            sessionId: sessionId(row),
            appId: appId(row),
            appName: appName(row),
        })
        if (mine !== sessionToken.current) return
        if (outcome.kind === 'notAdmitted') return setOpen({ kind: 'failed', row, message: outcome.message })
        const result = outcome.result
        if (result.kind === 'session') {
            const session = JSON.parse(result.session.session) as Row
            const actions = Array.isArray(session.session_actions) ? (session.session_actions as Row[]) : []
            setOpen({ kind: 'shown', row, session, actions, more: result.session.more })
        } else setOpen({ kind: 'failed', row, message: MANAGEMENT_FAILURE_TEXT[result.kind === 'failed' ? result.failure : 'invalidResponse'] })
    }

    async function loadDetails(row: Row, scan: Row) {
        const key = scanKey(scan)
        const outcome = await readManagement(dispatch, observe, context, {
            kind: 'transaction',
            sessionId: sessionId(row),
            appId: appId(row),
            appName: appName(row),
            scanId: scanId(scan),
            subRequest: subRequest(scan),
        })
        const text =
            outcome.kind === 'notAdmitted'
                ? outcome.message
                : outcome.result.kind === 'transaction'
                  ? JSON.stringify(JSON.parse(outcome.result.transaction), null, 2)
                  : MANAGEMENT_FAILURE_TEXT[outcome.result.kind === 'failed' ? outcome.result.failure : 'invalidResponse']
        setDetails((d) => ({ ...d, [key]: text }))
    }

    async function reveal(scan: Row) {
        const key = scanKey(scan)
        setRevealed((r) => ({ ...r, [key]: { kind: 'loading' } }))
        const outcome = await readManagement(dispatch, observe, context, { kind: 'content', scanId: scanId(scan), subRequest: subRequest(scan) })
        const next: Revealed =
            outcome.kind === 'notAdmitted'
                ? { kind: 'failed', message: outcome.message }
                : outcome.result.kind === 'content'
                  ? { kind: 'shown', content: outcome.result.content }
                  : { kind: 'failed', message: MANAGEMENT_FAILURE_TEXT[outcome.result.kind === 'failed' ? outcome.result.failure : 'invalidResponse'] }
        setRevealed((r) => (r[key] ? { ...r, [key]: next } : r))
    }

    const hide = (scan: Row) =>
        setRevealed((r) => {
            const { [scanKey(scan)]: _, ...rest } = r
            return rest
        })

    const rows = shown?.rows ?? []
    const applications = [...new Set(rows.map(appName).filter((a) => a.length > 0))].sort()
    const detectorCodes = [...new Set(rows.flatMap(detectors))].sort()
    const counts = { all: rows.length, malicious: 0, benign: 0 }
    for (const row of rows) {
        const v = verdictOf(row).verdict
        if (v === 'malicious') counts.malicious += 1
        if (v === 'benign') counts.benign += 1
    }
    const visible = rows.filter(
        (row) =>
            (application === '' || appName(row) === application) &&
            (verdict === 'all' || verdictOf(row).verdict === verdict) &&
            (detector === '' || detectors(row).includes(detector))
    )
    const page = shown?.page
    const filtered = application !== '' || verdict !== 'all' || detector !== ''

    return (
        <div className='flex flex-col gap-5' data-slot='sessions-investigation'>
            <form
                className='flex flex-wrap items-end gap-3'
                role='search'
                onSubmit={(e) => {
                    e.preventDefault()
                    void runSearch()
                }}
            >
                <label className='flex min-w-56 flex-1 flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-search`}>
                    Search by session ID, scan ID or detector
                    <Input id={`${ids}-search`} value={query} maxLength={256} onChange={(e) => setQuery(e.target.value)} autoComplete='off' spellCheck={false} />
                </label>
                <Button type='submit' tier='secondary' disabled={search?.kind === 'searching' || query.trim().length === 0}>
                    Search
                </Button>
                {search ? (
                    <Button
                        type='button'
                        tier='tertiary'
                        onClick={() => {
                            searchToken.current += 1
                            setSearch(null)
                        }}
                    >
                        Clear search
                    </Button>
                ) : null}
            </form>
            {search ? <SearchResult state={search} onOpen={(row) => void openSession(row)} /> : null}

            <div className='flex flex-wrap items-end gap-3' data-slot='sessions-filters'>
                <label className='flex flex-col gap-1 text-[13px] font-medium'>
                    Application
                    <select className='border-academy-rule rounded-academy-sm bg-academy-bg-inset min-h-11 border px-2' value={application} onChange={(e) => setApplication(e.target.value)}>
                        <option value=''>All applications</option>
                        {applications.map((a) => (
                            <option key={a} value={a}>
                                {a}
                            </option>
                        ))}
                    </select>
                </label>
                <label className='flex flex-col gap-1 text-[13px] font-medium'>
                    Threat category
                    <select className='border-academy-rule rounded-academy-sm bg-academy-bg-inset min-h-11 border px-2' value={detector} onChange={(e) => setDetector(e.target.value)}>
                        <option value=''>All detectors</option>
                        {detectorCodes.map((d) => (
                            <option key={d} value={d}>
                                {d}
                            </option>
                        ))}
                    </select>
                </label>
                <div role='group' aria-label='Verdict' className='flex gap-1'>
                    {(['all', 'malicious', 'benign'] as const).map((v) => (
                        <Button key={v} type='button' size='sm' tier={verdict === v ? 'secondary' : 'tertiary'} aria-pressed={verdict === v} onClick={() => setVerdict(v)}>
                            {`${v === 'all' ? 'All' : VERDICT_TEXT[v]}${shown ? ` ${counts[v]}` : ''}`}
                        </Button>
                    ))}
                </div>
                <Button type='button' tier='secondary' disabled={loading} onClick={() => void load(page?.offset ?? 0)}>
                    Refresh
                </Button>
            </div>
            <p className='text-academy-ink-faint text-[12px]'>
                The list shows the last 24 hours. Filters and counts cover this page only.
            </p>
            <div role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='sessions-progress'>
                {loading
                    ? 'Reading sessions…'
                    : page && shown
                      ? `${page.count === 0 ? 'No sessions' : `Sessions ${page.offset + 1}–${page.offset + page.count}${page.total !== null ? ` of ${page.total}` : page.complete ? '' : '+'}`} · read at ${time(shown.readAt)}`
                      : ''}
            </div>
            {failure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    <p>{failure}</p>
                    {shown ? <p className='mt-1 text-[12px]'>Showing sessions read at {time(shown.readAt)}. They may be out of date.</p> : null}
                </Callout>
            ) : null}
            {page && page.count === 0 && page.offset === 0 && page.complete && !failure ? (
                <Callout tone='default' icon={Info}>
                    The service reports no sessions in the last 24 hours.
                </Callout>
            ) : null}
            {rows.length > 0 ? (
                <SessionTable rows={visible} onOpen={(row) => void openSession(row)} caption='Sessions of the last 24 hours' />
            ) : null}
            {filtered && rows.length > 0 && visible.length === 0 ? <p className='text-academy-ink-faint text-[12px]'>No sessions on this page match the filters.</p> : null}
            {page ? (
                <div className='flex flex-wrap gap-2'>
                    <Button type='button' tier='secondary' disabled={loading || page.offset === 0} onClick={() => void load(0)}>
                        First page
                    </Button>
                    <Button
                        type='button'
                        tier='secondary'
                        disabled={loading || page.complete || page.nextOffset === null}
                        onClick={() => page.nextOffset !== null && void load(page.nextOffset)}
                    >
                        Next page
                    </Button>
                </div>
            ) : null}

            {open ? (
                <SessionPanel
                    state={open}
                    details={details}
                    revealed={revealed}
                    reports={sessions.reports.items}
                    onDetails={(scan) => open.kind === 'shown' && void loadDetails(open.row, scan)}
                    onReveal={(scan) => void reveal(scan)}
                    onHide={hide}
                    onReport={setReporting}
                    onClose={() => {
                        sessionToken.current += 1
                        setOpen(null)
                        setReporting(null)
                    }}
                />
            ) : null}
            {reporting && open?.kind === 'shown' ? (
                <ReportForm
                    key={scanKey(reporting)}
                    context={context}
                    dispatch={dispatch}
                    problem={sessions.reports.problem}
                    reportsReady={sessions.reports.state === 'ready'}
                    target={{
                        scanId: scanId(reporting),
                        subRequest: subRequest(reporting),
                        sessionId: sessionId(open.row),
                        appId: appId(open.row),
                        appName: appName(open.row),
                    }}
                    existing={sessions.reports.items.find((r) => r.scanId === scanId(reporting) && r.scanSubReqId === subRequest(reporting)) ?? null}
                    suggested={suggestedKind(verdictOf(reporting).verdict)}
                    onDone={() => setReporting(null)}
                />
            ) : null}
            <CachePanel context={context} sessions={sessions} dispatch={dispatch} />
        </div>
    )
}

function SessionTable({ rows, onOpen, caption }: { rows: Row[]; onOpen: (row: Row) => void; caption: string }) {
    return (
        <table className='w-full border-collapse text-left text-[13px]' data-slot='sessions-table'>
            <caption className='sr-only'>{caption}</caption>
            <thead>
                <tr className='border-academy-rule border-b'>
                    <th scope='col' className='py-2 pr-3 font-medium'>
                        Session
                    </th>
                    <th scope='col' className='py-2 pr-3 font-medium'>
                        Application
                    </th>
                    <th scope='col' className='py-2 pr-3 font-medium'>
                        Verdict
                    </th>
                    <th scope='col' className='py-2 font-medium'>
                        <span className='sr-only'>Actions</span>
                    </th>
                </tr>
            </thead>
            <tbody>
                {rows.map((row, index) => {
                    const { verdict } = verdictOf(row)
                    const found = detectors(row)
                    return (
                        <tr key={`${appId(row)}-${sessionId(row)}-${index}`} className='border-academy-rule border-b' data-slot='sessions-row'>
                            <td className='py-2 pr-3 font-mono text-[12px] break-all'>{sessionId(row) || '—'}</td>
                            <td className='py-2 pr-3 break-all'>{appName(row) || '—'}</td>
                            <td className='py-2 pr-3'>
                                {VERDICT_TEXT[verdict]}
                                {found.length > 0 ? <span className='text-academy-ink-soft'> · {found.join(', ')}</span> : null}
                            </td>
                            <td className='py-2 text-right'>
                                <Button type='button' tier='secondary' aria-label={`Open session ${sessionId(row)}`} onClick={() => onOpen(row)}>
                                    Open
                                </Button>
                            </td>
                        </tr>
                    )
                })}
            </tbody>
        </table>
    )
}

function SearchResult({ state, onOpen }: { state: SearchState; onOpen: (row: Row) => void }) {
    if (state.kind === 'searching') {
        return (
            <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                Searching for {state.query}…
            </p>
        )
    }
    if (state.kind === 'failed') {
        return (
            <Callout tone='danger' icon={AlertTriangle} role='alert'>
                {state.message}
            </Callout>
        )
    }
    return (
        <div className='flex flex-col gap-2' data-slot='sessions-search-result'>
            <p role='status' aria-live='polite' className='text-[13px]'>
                {coverage(state.search, state.rows.length)}
            </p>
            {state.scan ? (
                <pre className='bg-academy-bg-inset rounded-academy-md max-h-64 overflow-auto p-3 font-mono text-[12px]' data-slot='sessions-scan-metadata'>
                    {JSON.stringify(state.scan, null, 2)}
                </pre>
            ) : null}
            {state.rows.length > 0 ? <SessionTable rows={state.rows} onOpen={onOpen} caption={`Search results for ${state.search.query}`} /> : null}
        </div>
    )
}

function SessionPanel(props: {
    state: SessionState
    details: Record<string, string>
    revealed: Record<string, Revealed>
    reports: Report[]
    onDetails: (scan: Row) => void
    onReveal: (scan: Row) => void
    onHide: (scan: Row) => void
    onReport: (scan: Row) => void
    onClose: () => void
}) {
    const { state } = props
    return (
        <section aria-label={`Session ${sessionId(state.row)}`} className='border-academy-rule rounded-academy-md flex flex-col gap-3 border p-4' data-slot='sessions-session'>
            <div className='flex flex-wrap items-center justify-between gap-2'>
                <h2 className='text-[16px] font-semibold'>
                    Session <span className='font-mono text-[14px]'>{sessionId(state.row)}</span>
                </h2>
                <Button type='button' tier='tertiary' onClick={props.onClose}>
                    Close session
                </Button>
            </div>
            {state.kind === 'loading' ? <p role='status' className='text-academy-ink-soft text-[13px]'>Reading the session…</p> : null}
            {state.kind === 'failed' ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    {state.message}
                </Callout>
            ) : null}
            {state.kind === 'shown' ? (
                <>
                    <p className='text-academy-ink-soft text-[13px]'>
                        {appName(state.session) || appName(state.row)} ·{' '}
                        {state.more ? `At least ${state.actions.length} scans (the first ${state.actions.length} are shown)` : `${state.actions.length} scans`}
                    </p>
                    <ol className='flex flex-col gap-3'>
                        {state.actions.map((scan) => {
                            const key = scanKey(scan)
                            const { verdict, action } = verdictOf(scan)
                            const shown = props.revealed[key]
                            const reported = props.reports.some((r) => r.scanId === scanId(scan) && r.scanSubReqId === subRequest(scan))
                            return (
                                <li key={key} className='border-academy-rule flex flex-col gap-2 border-b pb-3' data-slot='sessions-scan'>
                                    <div className='flex flex-wrap items-center gap-2 text-[13px]'>
                                        <span className='font-mono text-[12px]'>{scanId(scan)}</span>
                                        <span>
                                            {VERDICT_TEXT[verdict]}
                                            {detectors(scan).length > 0 ? ` · ${detectors(scan).join(', ')}` : ''}
                                            {action ? ` · ${action}` : ''}
                                        </span>
                                        {reported ? <span className='text-academy-ink-soft text-[12px]'>Reported</span> : null}
                                    </div>
                                    <div className='flex flex-wrap gap-2'>
                                        <Button type='button' size='sm' tier='secondary' onClick={() => props.onDetails(scan)} aria-label={`Details for scan ${scanId(scan)}`}>
                                            Details
                                        </Button>
                                        {shown ? (
                                            <Button type='button' size='sm' tier='secondary' onClick={() => props.onHide(scan)} aria-label={`Hide content of scan ${scanId(scan)}`}>
                                                <EyeOff aria-hidden className='size-4' /> Hide content
                                            </Button>
                                        ) : (
                                            <Button type='button' size='sm' tier='secondary' onClick={() => props.onReveal(scan)} aria-label={`Show content of scan ${scanId(scan)}`}>
                                                <Eye aria-hidden className='size-4' /> Show content
                                            </Button>
                                        )}
                                        <Button type='button' size='sm' tier='secondary' onClick={() => props.onReport(scan)} aria-label={`Report scan ${scanId(scan)}`}>
                                            {reported ? 'Edit report…' : 'Report…'}
                                        </Button>
                                    </div>
                                    {props.details[key] ? (
                                        <pre className='bg-academy-bg-inset rounded-academy-md max-h-64 overflow-auto p-3 font-mono text-[12px]' data-slot='sessions-transaction'>
                                            {props.details[key]}
                                        </pre>
                                    ) : null}
                                    {shown ? <ContentCard state={shown} /> : null}
                                </li>
                            )
                        })}
                    </ol>
                </>
            ) : null}
        </section>
    )
}

function ContentCard({ state }: { state: Revealed }) {
    if (state.kind === 'loading') return <p role='status' className='text-academy-ink-soft text-[13px]'>Reading the content…</p>
    if (state.kind === 'failed')
        return (
            <Callout tone='danger' icon={AlertTriangle} role='alert'>
                {state.message}
            </Callout>
        )
    return (
        <div className='bg-academy-bg-inset rounded-academy-md flex flex-col gap-2 p-3 text-[13px]' data-slot='sessions-content'>
            <p className='font-medium'>Prompt</p>
            <p className='break-words whitespace-pre-wrap'>{state.content.prompt ?? '— (none)'}</p>
            <p className='font-medium'>Response</p>
            <p className='break-words whitespace-pre-wrap'>{state.content.response ?? '— (none)'}</p>
        </div>
    )
}

function ReportForm(props: {
    context: ContextRef
    dispatch: Dispatch
    problem: ReportProblem | null
    reportsReady: boolean
    target: { scanId: string; subRequest: number; sessionId: string; appId: string; appName: string }
    existing: Report | null
    suggested: ReportKind
    onDone: () => void
}) {
    const ids = useId()
    const [kind, setKind] = useState<ReportKind>(props.existing?.kind ?? props.suggested)
    const [category, setCategory] = useState(props.existing?.category ?? '')
    const [reason, setReason] = useState(props.existing?.reason ?? '')
    const [message, setMessage] = useState<string | null>(null)
    const [saving, setSaving] = useState(false)

    async function save() {
        if (reason.trim().length === 0 || reason.length > 2000) return setMessage(REPORT_PROBLEM_TEXT.reason)
        setSaving(true)
        const outcome = await props.dispatch({
            kind: 'saveReport',
            context: props.context,
            target: props.target,
            input: { kind, category, reason },
        })
        setSaving(false)
        if (outcome.kind === 'admitted') props.onDone()
        else setMessage(outcome.kind === 'refused' && outcome.refusal === 'invalidInput' && props.problem ? REPORT_PROBLEM_TEXT[props.problem] : outcomeMessage(outcome))
    }

    return (
        <form
            aria-label={`Report scan ${props.target.scanId}`}
            className='border-academy-rule rounded-academy-md flex flex-col gap-3 border p-4'
            data-slot='sessions-report-form'
            onSubmit={(e) => {
                e.preventDefault()
                void save()
            }}
        >
            <h2 className='text-[16px] font-semibold'>{props.existing ? 'Edit report' : 'Report this verdict'}</h2>
            <p className='text-academy-ink-soft text-[13px]'>
                Kept on this Mac with the scan’s metadata. The prompt and response are never part of a report, and
                nothing is sent to the tenant.
            </p>
            {!props.reportsReady ? <p className='text-academy-ink-soft text-[13px]'>This tenant’s reports are not open, so a report cannot be saved now.</p> : null}
            <fieldset className='flex flex-wrap gap-4'>
                <legend className='mb-1 text-[13px] font-medium'>Kind</legend>
                {(['false_positive', 'false_negative'] as const).map((k) => (
                    <label key={k} className='flex items-center gap-2 text-[13px]'>
                        <input
                            type='radio'
                            name={`${ids}-kind`}
                            checked={kind === k}
                            onChange={() => {
                                setKind(k)
                                setCategory('')
                            }}
                        />
                        {KIND_TEXT[k]}
                    </label>
                ))}
            </fieldset>
            <label className='flex flex-col gap-1 text-[13px] font-medium'>
                Category
                <select className='border-academy-rule rounded-academy-sm bg-academy-bg-inset min-h-11 border px-2' value={category} onChange={(e) => setCategory(e.target.value)}>
                    <option value=''>No category</option>
                    {CATEGORIES[kind].map((c) => (
                        <option key={c} value={c}>
                            {c}
                        </option>
                    ))}
                </select>
            </label>
            <label className='flex flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-reason`}>
                Why is this verdict wrong?
                <textarea id={`${ids}-reason`} className={TEXTAREA} value={reason} maxLength={2000} onChange={(e) => setReason(e.target.value)} />
            </label>
            {message ? (
                <p role='alert' className='text-academy-danger text-[13px]'>
                    {message}
                </p>
            ) : null}
            <div className='flex gap-2'>
                <Button type='submit' tier='primary' disabled={saving || !props.reportsReady}>
                    Save report
                </Button>
                <Button type='button' tier='tertiary' onClick={props.onDone}>
                    Cancel
                </Button>
            </div>
        </form>
    )
}

const STORAGE_TEXT = {
    memory: 'Kept in memory for this credential until it changes or the app quits.',
    protected: 'Sealed on this Mac under a key in your Keychain, and removed with the credential.',
    unavailable: 'Protected storage is unavailable; kept in memory only.',
} as const

function CachePanel({ context, sessions, dispatch }: { context: ContextRef; sessions: SessionsSnapshot; dispatch: Dispatch }) {
    const [message, setMessage] = useState<string | null>(null)
    const cache = sessions.investigation
    if (!cache) return null
    const exporting = sessions.export?.kind === 'investigation' ? sessions.export : null
    const send = async (kind: 'clearInvestigation' | 'exportInvestigation') => setMessage(outcomeMessage(await dispatch({ kind, context })))
    return (
        <section aria-label='Investigation cache' className='border-academy-rule flex flex-col gap-2 border-t pt-4 text-[13px]' data-slot='sessions-cache'>
            <h2 className='text-[14px] font-semibold'>Investigation cache</h2>
            <p>
                {cache.sessions} sessions, {cache.transactions} transactions and {cache.contents} revealed scans. {STORAGE_TEXT[cache.storage]}
            </p>
            <div className='flex flex-wrap gap-2'>
                <Button type='button' tier='secondary' onClick={() => void send('exportInvestigation')} aria-disabled={exporting?.state === 'saving'}>
                    Export metadata…
                </Button>
                <Button type='button' tier='secondary' onClick={() => void send('clearInvestigation')}>
                    Clear cache
                </Button>
            </div>
            <p className='text-academy-ink-faint text-[12px]'>An export holds session and transaction metadata only, never a prompt or response.</p>
            <ExportStatus view={exporting} />
            {message ? <p role='alert'>{message}</p> : null}
        </section>
    )
}

function ExportStatus({ view }: { view: SessionsSnapshot['export'] }) {
    if (!view) return null
    const text = {
        saving: 'Choose where to save the export…',
        saved: `Saved ${view.name ?? ''}.`,
        cancelled: 'Export cancelled. Nothing was saved.',
        failed: 'The export could not be saved.',
    }[view.state]
    return (
        <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]' data-slot='sessions-export-status'>
            {text}
        </p>
    )
}

function Reports({ context, sessions, dispatch }: { context: ContextRef; sessions: SessionsSnapshot; dispatch: Dispatch }) {
    const reports = sessions.reports
    const [message, setMessage] = useState<string | null>(null)
    const [editing, setEditing] = useState<Report | null>(null)
    const send = async (intent: Parameters<Dispatch>[0]) => setMessage(outcomeMessage(await dispatch(intent)))
    const exporting = sessions.export?.kind === 'reports' ? sessions.export : null
    return (
        <div className='flex flex-col gap-4' data-slot='sessions-reports'>
            <p className='text-academy-ink-soft text-[13px]'>
                False-positive and false-negative reports for this tenant, kept on this Mac. They hold scan metadata and
                your reason only; they suppress nothing and change no policy.
            </p>
            {reports.state === 'loading' ? <p role='status'>Opening reports…</p> : null}
            {reports.state === 'failed' ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    {reports.failure === 'unreadable'
                        ? 'This tenant’s report file could not be read. It was kept unchanged.'
                        : 'This tenant’s reports could not be opened.'}
                </Callout>
            ) : null}
            {reports.unsaved ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    The last change could not be saved on this Mac. It is shown here but will be lost when the app quits.
                </Callout>
            ) : null}
            {reports.undo ? (
                <div className='flex flex-wrap items-center gap-2 text-[13px]' role='status'>
                    {`Removed the report for ${reports.undo.scanId}.`}
                    <Button type='button' size='sm' tier='secondary' onClick={() => void send({ kind: 'undoReport', context })}>
                        Undo
                    </Button>
                </div>
            ) : null}
            {reports.state === 'ready' && reports.items.length === 0 ? <p className='text-[13px]'>No reports yet. Report a scan from its session.</p> : null}
            {reports.items.length > 0 ? (
                <>
                    <table className='w-full border-collapse text-left text-[13px]' data-slot='sessions-reports-table'>
                        <caption className='sr-only'>FP/FN reports</caption>
                        <thead>
                            <tr className='border-academy-rule border-b'>
                                <th scope='col' className='py-2 pr-3 font-medium'>
                                    Scan
                                </th>
                                <th scope='col' className='py-2 pr-3 font-medium'>
                                    Kind
                                </th>
                                <th scope='col' className='py-2 pr-3 font-medium'>
                                    Reason
                                </th>
                                <th scope='col' className='py-2 font-medium'>
                                    <span className='sr-only'>Actions</span>
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            {reports.items.map((r) => (
                                <tr key={`${r.scanId}-${r.scanSubReqId}`} className='border-academy-rule border-b align-top' data-slot='sessions-report-row'>
                                    <td className='py-2 pr-3'>
                                        <span className='font-mono text-[12px] break-all'>{r.scanId}</span>
                                        <span className='text-academy-ink-soft block text-[12px]'>
                                            {[r.application, VERDICT_TEXT[r.verdict as Verdict] ?? fieldText(r.verdict), r.detectors.join(', ')].filter(Boolean).join(' · ')}
                                        </span>
                                    </td>
                                    <td className='py-2 pr-3'>
                                        {KIND_TEXT[r.kind]}
                                        {r.category ? <span className='text-academy-ink-soft block text-[12px]'>{r.category}</span> : null}
                                    </td>
                                    <td className='py-2 pr-3 break-words whitespace-pre-wrap'>{r.reason}</td>
                                    <td className='flex flex-col gap-1 py-2 text-right'>
                                        <Button type='button' size='sm' tier='secondary' aria-label={`Edit report for ${r.scanId}`} onClick={() => setEditing(r)}>
                                            Edit…
                                        </Button>
                                        <Button
                                            type='button'
                                            size='sm'
                                            tier='secondary'
                                            aria-label={`Remove report for ${r.scanId}`}
                                            onClick={() => void send({ kind: 'removeReport', context, scanId: r.scanId, subRequest: r.scanSubReqId })}
                                        >
                                            Remove
                                        </Button>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='button' tier='secondary' aria-disabled={exporting?.state === 'saving'} onClick={() => void send({ kind: 'exportReports', context })}>
                            Export CSV…
                        </Button>
                    </div>
                </>
            ) : null}
            <ExportStatus view={exporting} />
            {editing ? (
                <ReportForm
                    key={`${editing.scanId}-${editing.scanSubReqId}`}
                    context={context}
                    dispatch={dispatch}
                    problem={reports.problem}
                    reportsReady={reports.state === 'ready'}
                    target={{ scanId: editing.scanId, subRequest: editing.scanSubReqId, sessionId: '', appId: '', appName: '' }}
                    existing={editing}
                    suggested={editing.kind}
                    onDone={() => setEditing(null)}
                />
            ) : null}
            {message ? <p role='alert' className='text-[13px]'>{message}</p> : null}
        </div>
    )
}

/** v1's inert Exception Patterns prototype: synthetic data, nothing live. */
function ExceptionPreview() {
    return (
        <div className='flex flex-col gap-3' data-slot='sessions-exceptions'>
            <Callout tone='default' icon={FlaskConical}>
                <p className='font-medium'>Synthetic data · Memory only · No live integrations</p>
                <p className='mt-1'>
                    A preview of how recurring false positives could become reviewed exception patterns. Nothing here
                    reads or changes a tenant, and nothing is kept.
                </p>
            </Callout>
            <table className='w-full border-collapse text-left text-[13px]'>
                <caption className='sr-only'>Synthetic exception pattern preview</caption>
                <thead>
                    <tr className='border-academy-rule border-b'>
                        <th scope='col' className='py-2 pr-3 font-medium'>
                            Pattern
                        </th>
                        <th scope='col' className='py-2 pr-3 font-medium'>
                            Simulated matches
                        </th>
                        <th scope='col' className='py-2 font-medium'>
                            State
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <tr className='border-academy-rule border-b'>
                        <td className='py-2 pr-3'>Order numbers flagged as card numbers (sim-application-001)</td>
                        <td className='py-2 pr-3'>12</td>
                        <td className='py-2'>Simulated · not applied</td>
                    </tr>
                </tbody>
            </table>
        </div>
    )
}
