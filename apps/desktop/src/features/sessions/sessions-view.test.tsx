/**
 * Sessions & Reports through the real App, DesktopClient and shell: the
 * 24-hour inventory with page-only filters, searches that say what they
 * covered, content shown only after an explicit reveal and forgotten on Hide
 * or when Sessions closes, reports sent as scan references plus the
 * operator's reason (never content), and the reports and cache controls.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type Intent,
    type ManagementResult,
    type Report,
    type SessionsView,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const PROMPT = 'SYNTHETIC-PROMPT-TEXT'

const row = (n: number, detector: string | null, status: string) => ({
    session_id: `ses-${n}`,
    application_id: `app-${n % 2}`,
    application_name: `Bot ${n % 2}`,
    status,
    detection_type_violation_breakdown: detector ? [{ detection_type: detector, violation_breakdown: { total: 1 } }] : [],
})
const ROWS = [row(1, 'prompt_injection', 'block'), row(2, null, 'allow'), row(3, 'dlp', 'alert')]
const ACTIONS = [
    { scan_id: 'scn-1-0', scan_sub_req_id: 0, session_id: 'ses-1', status: 'block', profile_name: 'Prod',
      detection_type_violation_breakdown: [{ detection_type: 'prompt_injection', violation_breakdown: { total: 1 } }] },
    { scan_id: 'scn-1-1', scan_sub_req_id: 0, session_id: 'ses-1', status: 'allow', detection_type_violation_breakdown: [] },
]

const REPORT: Report = {
    scanId: 'scn-1-0', scanSubReqId: 0, sessionId: 'ses-1', applicationId: 'app-1', application: 'Bot 1',
    kind: 'false_positive', verdict: 'malicious', action: 'Blocked', detectors: ['prompt_injection'], profile: 'Prod',
    category: 'Wrong detector', reason: 'Order numbers', reportedAt: '2026-10-18T10:00:00Z',
}

const sessionsView = (overrides: Partial<SessionsView> = {}): SessionsView => ({
    reports: { state: 'ready', failure: null, items: [], undo: null, problem: null, unsaved: false },
    investigation: null,
    export: null,
    ...overrides,
})

const ready = (sessions: SessionsView = sessionsView()): ShellSnapshot => ({
    ...withTenant(
        baseSnapshot,
        tenant({
            management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1234567890.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
        }),
        CONTEXT
    ),
    sessions,
})

type Answer = (request: Extract<Intent, { kind: 'management' }>['request']) => ManagementResult

const DEFAULT: Answer = (request) => {
    switch (request.kind) {
        case 'page':
            return { kind: 'page', page: { resource: 'sessions', offset: 0, limit: 25, items: JSON.stringify(ROWS), count: 3, nextOffset: null, complete: true, total: 3 } }
        case 'session':
            return { kind: 'session', session: { session: JSON.stringify({ session_id: 'ses-1', application_name: 'Bot 1', session_actions: ACTIONS }), actions: 2, more: false } }
        case 'transaction':
            return { kind: 'transaction', transaction: JSON.stringify({ scan_id: request.scanId, verdict: 'malicious' }) }
        case 'content':
            return { kind: 'content', content: { prompt: PROMPT, response: 'SYNTHETIC-RESPONSE-TEXT' } }
        default:
            return { kind: 'failed', failure: 'invalidResponse', outcome: 'completed' }
    }
}

function service(native: Native, answer: Answer = DEFAULT) {
    const requests = new Map<string, Extract<Intent, { kind: 'management' }>['request']>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind === 'management') {
            const operation = `op-${requests.size + 1}`
            requests.set(operation, intent.request)
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${requests.size}`, operation } }
        }
        return respond(intent)
    }
    native.observeManagement = (operation) => {
        const request = requests.get(operation)
        return { kind: 'management', result: request ? answer(request) : DEFAULT({ kind: 'verify' }) }
    }
    return requests
}

async function openSessions(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Sessions/ }))
    await screen.findByRole('heading', { level: 1, name: 'Investigate sessions' })
    return user
}

const sent = (native: Native, kind: Intent['kind']) => native.dispatched.filter((i) => i.kind === kind)

describe('Sessions & Reports', () => {
    it('lists the last 24 hours with page-only filters and counts', async () => {
        const native = scriptedNative(ready())
        const requests = service(native)
        const user = await openSessions(native)
        expect(await screen.findByText(/Sessions 1–3 of 3/)).toBeInTheDocument()
        expect([...requests.values()][0]).toEqual({ kind: 'page', resource: 'sessions', offset: 0, limit: 25 })
        expect(screen.getByText('The list shows the last 24 hours. Filters and counts cover this page only.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Malicious 2' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Benign 1' })).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Benign 1' }))
        expect(screen.getAllByRole('row').filter((r) => r.getAttribute('data-slot') === 'sessions-row')).toHaveLength(1)
        await user.click(screen.getByRole('button', { name: 'All 3' }))
        await user.selectOptions(screen.getByRole('combobox', { name: 'Threat category' }), 'dlp')
        const rows = screen.getAllByRole('row').filter((r) => r.getAttribute('data-slot') === 'sessions-row')
        expect(rows).toHaveLength(1)
        expect(within(rows[0] as HTMLElement).getByText('ses-3')).toBeInTheDocument()
    })

    it('shows content only after an explicit reveal and forgets it on Hide and on leaving', async () => {
        const native = scriptedNative(ready())
        const requests = service(native)
        const user = await openSessions(native)
        await user.click(await screen.findByRole('button', { name: 'Open session ses-1' }))
        expect(await screen.findByText(/2 scans/)).toBeInTheDocument()
        expect(screen.queryByText(PROMPT)).toBeNull()
        expect([...requests.values()].some((r) => r.kind === 'content')).toBe(false)
        await user.click(screen.getByRole('button', { name: 'Details for scan scn-1-0' }))
        expect(await screen.findByText(/"verdict": "malicious"/)).toBeInTheDocument()
        expect(screen.queryByText(PROMPT)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Show content of scan scn-1-0' }))
        expect(await screen.findByText(PROMPT)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Hide content of scan scn-1-0' }))
        expect(screen.queryByText(PROMPT)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Show content of scan scn-1-0' }))
        expect(await screen.findByText(PROMPT)).toBeInTheDocument()
        // Leaving Sessions drops revealed content; coming back shows none.
        await user.click(within(screen.getByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Administration/ }))
        await user.click(within(screen.getByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Sessions/ }))
        await screen.findByRole('heading', { level: 1, name: 'Investigate sessions' })
        expect(screen.queryByText(PROMPT)).toBeNull()
    })

    it('says what each search covered and never claims more', async () => {
        const searches: Record<string, ManagementResult> = {
            'ses-404': { kind: 'search', search: { kind: 'sessionId', query: 'ses-404', sessions: '[]', scanned: 4000, complete: false, matchLimit: false, scan: null, cached: false } },
            prompt_injection: { kind: 'search', search: { kind: 'detector', query: 'prompt_injection', sessions: JSON.stringify(Array.from({ length: 100 }, (_, n) => row(n, 'prompt_injection', 'block'))), scanned: 700, complete: false, matchLimit: true, scan: null, cached: false } },
            'scn-1': { kind: 'search', search: { kind: 'scanId', query: 'scn-1', sessions: '[]', scanned: 0, complete: true, matchLimit: false, scan: JSON.stringify({ scan_id: 'scn-1', report_id: 'R-1' }), cached: false } },
        }
        const native = scriptedNative(ready())
        service(native, (request) => (request.kind === 'search' ? (searches[request.query] as ManagementResult) : DEFAULT(request)))
        const user = await openSessions(native)
        const box = screen.getByRole('textbox', { name: 'Search by session ID, scan ID or detector' })
        await user.type(box, 'ses-404{Enter}')
        expect(await screen.findByText('Not among the newest 4,000 sessions of the last 30 days. Older sessions were not read, so it may still exist.')).toBeInTheDocument()
        await user.clear(box)
        await user.type(box, 'prompt_injection{Enter}')
        expect(await screen.findByText(/100 sessions reporting prompt_injection among 700 sessions read from the last 24 hours\. Stopped at 100 matches; more may exist\./)).toBeInTheDocument()
        await user.clear(box)
        await user.type(box, 'scn-1{Enter}')
        expect(await screen.findByText('Scan found. Its metadata is shown; content stays hidden.')).toBeInTheDocument()
        expect(screen.getByText(/"report_id": "R-1"/)).toBeInTheDocument()
    })

    it('sends a report as a scan reference and the operator’s words only', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openSessions(native)
        await user.click(await screen.findByRole('button', { name: 'Open session ses-1' }))
        await user.click(await screen.findByRole('button', { name: 'Show content of scan scn-1-0' }))
        await screen.findByText(PROMPT)
        await user.click(screen.getByRole('button', { name: 'Report scan scn-1-0' }))
        const form = screen.getByRole('form', { name: 'Report scan scn-1-0' })
        // A malicious verdict suggests a false positive.
        expect(within(form).getByRole('radio', { name: 'False positive' })).toBeChecked()
        await user.click(within(form).getByRole('button', { name: 'Save report' }))
        expect(within(form).getByRole('alert')).toHaveTextContent('Say why this verdict is wrong')
        expect(sent(native, 'saveReport')).toEqual([])
        await user.selectOptions(within(form).getByRole('combobox', { name: 'Category' }), 'Wrong detector')
        await user.type(within(form).getByRole('textbox', { name: 'Why is this verdict wrong?' }), 'Order numbers')
        await user.click(within(form).getByRole('button', { name: 'Save report' }))
        expect(sent(native, 'saveReport')).toEqual([
            {
                kind: 'saveReport',
                context: CONTEXT,
                target: { scanId: 'scn-1-0', subRequest: 0, sessionId: 'ses-1', appId: 'app-1', appName: 'Bot 1' },
                input: { kind: 'false_positive', category: 'Wrong detector', reason: 'Order numbers' },
            },
        ])
        const reportBytes = native.sent.filter((s) => s.includes('saveReport')).join('\n')
        expect(reportBytes).not.toContain(PROMPT)
    })

    it('opens, removes, undoes and exports reports through the desktop', async () => {
        const native = scriptedNative(ready(sessionsView({ reports: { state: 'ready', failure: null, items: [REPORT], undo: null, problem: null, unsaved: false } })))
        service(native)
        const user = await openSessions(native)
        // The tenant's reports open with Sessions.
        expect(sent(native, 'openReports')).toEqual([{ kind: 'openReports', context: CONTEXT }])
        await user.click(screen.getByRole('tab', { name: /FP\/FN reports \(1\)/ }))
        expect(screen.getByText('Order numbers')).toBeInTheDocument()
        expect(screen.getByText(/they suppress nothing and change no policy/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Remove report for scn-1-0' }))
        expect(sent(native, 'removeReport')).toEqual([{ kind: 'removeReport', context: CONTEXT, scanId: 'scn-1-0', subRequest: 0 }])
        await native.push(ready(sessionsView({ reports: { state: 'ready', failure: null, items: [], undo: { scanId: 'scn-1-0', subRequest: 0 }, problem: null, unsaved: false } })))
        await user.click(screen.getByRole('button', { name: 'Undo' }))
        expect(sent(native, 'undoReport')).toEqual([{ kind: 'undoReport', context: CONTEXT }])
        await native.push(ready(sessionsView({ reports: { state: 'ready', failure: null, items: [REPORT], undo: null, problem: null, unsaved: true } })))
        expect(screen.getByText(/could not be saved on this Mac/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Export CSV…' }))
        expect(sent(native, 'exportReports')).toEqual([{ kind: 'exportReports', context: CONTEXT }])
        await native.push(
            ready(sessionsView({ reports: { state: 'ready', failure: null, items: [REPORT], undo: null, problem: null, unsaved: false }, export: { kind: 'reports', state: 'saved', name: 'fpfn-reports-prod-eu-20261018-1000.csv' } }))
        )
        expect(screen.getByText('Saved fpfn-reports-prod-eu-20261018-1000.csv.')).toBeInTheDocument()
        // Editing sends only the scan reference with the new words.
        await user.click(screen.getByRole('button', { name: 'Edit report for scn-1-0' }))
        const form = screen.getByRole('form', { name: 'Report scan scn-1-0' })
        await user.click(within(form).getByRole('radio', { name: 'False negative' }))
        await user.click(within(form).getByRole('button', { name: 'Save report' }))
        expect(sent(native, 'saveReport').at(-1)).toMatchObject({
            target: { scanId: 'scn-1-0', subRequest: 0, sessionId: '', appId: '', appName: '' },
            input: { kind: 'false_negative', category: '', reason: 'Order numbers' },
        })
    })

    it('reports an unreadable report file without offering to overwrite it', async () => {
        const native = scriptedNative(ready(sessionsView({ reports: { state: 'failed', failure: 'unreadable', items: [], undo: null, problem: null, unsaved: false } })))
        service(native)
        const user = await openSessions(native)
        await user.click(screen.getByRole('tab', { name: 'FP/FN reports' }))
        expect(screen.getByRole('alert')).toHaveTextContent('could not be read. It was kept unchanged.')
        expect(screen.queryByRole('button', { name: 'Export CSV…' })).toBeNull()
    })

    it('clears and exports the investigation cache and says how it is kept', async () => {
        const native = scriptedNative(ready(sessionsView({ investigation: { storage: 'protected', sessions: 2, transactions: 1, contents: 1 } })))
        service(native)
        const user = await openSessions(native)
        expect(screen.getByText(/2 sessions, 1 transactions and 1 revealed scans\. Sealed on this Mac under a key in your Keychain/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Export metadata…' }))
        await user.click(screen.getByRole('button', { name: 'Clear cache' }))
        expect(sent(native, 'exportInvestigation')).toEqual([{ kind: 'exportInvestigation', context: CONTEXT }])
        expect(sent(native, 'clearInvestigation')).toEqual([{ kind: 'clearInvestigation', context: CONTEXT }])
        await native.push(ready(sessionsView({ investigation: { storage: 'memory', sessions: 0, transactions: 0, contents: 0 }, export: { kind: 'investigation', state: 'cancelled', name: null } })))
        expect(screen.getByText('Export cancelled. Nothing was saved.')).toBeInTheDocument()
        expect(screen.getByText(/Kept in memory for this credential/)).toBeInTheDocument()
    })

    it('labels the exception patterns preview as synthetic and inert', async () => {
        const native = scriptedNative(ready())
        const requests = service(native)
        const user = await openSessions(native)
        const before = requests.size
        await user.click(screen.getByRole('tab', { name: 'Exception patterns' }))
        expect(screen.getByText('Synthetic data · Memory only · No live integrations')).toBeInTheDocument()
        expect(requests.size).toBe(before)
    })

    it('asks for a management client before reading sessions', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        service(native)
        await openSessions(native)
        expect(screen.getByText(/has no ready management client/)).toBeInTheDocument()
        expect(sent(native, 'management')).toEqual([])
    })

    it('keeps existing reports usable without a management client', async () => {
        const native = scriptedNative({
            ...withTenant(baseSnapshot, tenant(), CONTEXT),
            sessions: sessionsView({ reports: { state: 'ready', failure: null, items: [REPORT], undo: null, problem: null, unsaved: false } }),
        })
        service(native)
        const user = await openSessions(native)
        expect(sent(native, 'openReports')).toEqual([{ kind: 'openReports', context: CONTEXT }])
        await user.click(screen.getByRole('tab', { name: /FP\/FN reports \(1\)/ }))
        expect(screen.getByText('Order numbers')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Edit report for scn-1-0' }))
        const form = screen.getByRole('form', { name: 'Report scan scn-1-0' })
        await user.click(within(form).getByRole('button', { name: 'Save report' }))
        await user.click(screen.getByRole('button', { name: 'Remove report for scn-1-0' }))
        await user.click(screen.getByRole('button', { name: 'Export CSV…' }))
        expect(sent(native, 'saveReport')).toHaveLength(1)
        expect(sent(native, 'removeReport')).toHaveLength(1)
        expect(sent(native, 'exportReports')).toHaveLength(1)
        expect(sent(native, 'management')).toEqual([])
    })
})
