/**
 * Runtime validation of native shell snapshots: malformed C02 fields are
 * contract violations, never rendered.
 */
import { describe, expect, it } from 'vitest'
import {
    ContractViolation,
    parseManagementResult,
    parseShellSnapshot,
    parseWorkbenchResult,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

const valid: ShellSnapshot = withTenant(baseSnapshot, tenant())

const caseView = {
    id: '11111111-2222-4333-8444-555555555555', product: 'model-security', name: 'Spec', profile: '', prompt: 'p', response: '',
    appName: '', expected: '', payload: '{}',
}
const inbox = {
    id: 'runtime-activity', title: 't', detail: 'd', source: 's', action: false, tone: 'ok', destination: 'sessions', actionLabel: 'a',
    facts: [{ label: 'Sessions', value: '1' }], perMille: 0,
}
const runtimeRead = {
    readAt: 1792000000, chart: { bucketSeconds: 3600, buckets: [{ time: '2026-10-19T00:00:00Z', total: 1, violating: 0, detections: null }] },
    chartFailure: null, summary: { sessions: 1, violating: 0, benign: 1, detectionEvents: null }, keysComplete: true, keysFailure: null,
    items: [inbox], gaps: [],
}
const report = {
    scanId: 'scn-1', scanSubReqId: 0, sessionId: 'ses-1', applicationId: 'app-1', application: 'Bot', kind: 'false_positive',
    verdict: 'malicious', action: 'Blocked', detectors: ['dlp'], profile: 'P', category: '', reason: 'r', reportedAt: '2026-10-18T10:00:00Z',
}

const scan = {
    operation: 'op-1',
    progress: 'settled',
    outcome: 'completed',
    stop: null,
    result: {
        scanId: 'S',
        reportId: 'R',
        trId: null,
        profileName: 'P',
        profileId: null,
        category: 'future-category',
        action: 'allow',
        verdict: 'unknown',
        timeout: false,
        serviceError: false,
        contentErrors: [{ contentType: 'prompt', feature: 'dlp', status: 'error' }],
        promptDetected: [{ detector: 'future_detector', detected: true }],
        responseDetected: [],
    },
    failure: null,
    discarded: false,
}
const signedIn = { state: 'signedIn', displayName: 'Ada', accountId: 'user_2fixture', learner: 'lx-1' }
const download = {
    courseId: 'runtime-security',
    title: 'Runtime Security',
    revision: 'rev-1',
    downloadedAt: '2026-10-18T12:00:00.000Z',
    leaseExpiresAt: '2026-10-25T12:00:00.000Z',
    status: 'current',
    units: 3,
    firstUnit: 'm-rt-1-lesson-01',
}
const localDraft = { attemptId: 'att_1', baseVersion: 2, savedAt: '2026-10-18T12:00:00.000Z' }
const offline = (overrides: Record<string, unknown>) => ({
    ...valid,
    academy: signedIn,
    academyOffline: { state: 'ready', reachable: false, downloads: [download], localDrafts: [localDraft], ...overrides },
})
const withScan = (overrides: Record<string, unknown>) => ({ ...valid, scans: [{ ...scan, ...overrides }] })

function review(overrides: Record<string, unknown>) {
    return {
        ...valid,
        import: {
            state: 'review',
            review: 'rv-0011223344556677',
            format: 'airsCliRegistry',
            fileName: 'tenants.json',
            candidates: [
                {
                    index: 0,
                    label: 'A',
                    tsgId: null,
                    destination: { kind: 'region', region: 'europe' },
                    defaultDestination: false,
                    problem: null,
                    existingLabel: false,
                },
            ],
            discarded: { secretFields: 0, fileReferences: 0, otherFields: 0 },
            ...overrides,
        },
    }
}

const changeReview = {
    review: 'rv-0011223344556677',
    kind: 'topics',
    action: 'update',
    target: 't-1',
    state: 'ready',
    failure: null,
    before: '{"topic_name":"Finance"}',
    after: '{"topic_name":"Finance"}',
    sourceKey: null,
    impact: 'Applies only to this tenant.',
    confirmation: 'Finance',
}
const changeResult = {
    operation: 'op-3',
    kind: 'apiKeys',
    action: 'rotate',
    target: 'key-1',
    name: 'Support key',
    settled: true,
    outcome: 'completed',
    failure: null,
    response: '{"api_key_id":"key-1"}',
    readback: 'found',
    current: null,
    secret: 'held',
}

describe('shell snapshot validation', () => {
    it('accepts a well-formed snapshot with C02 fields', () => {
        expect(
            parseShellSnapshot({
                ...valid,
                library: { ...valid.library, state: 'ready', cases: [caseView], notice: { kind: 'examplesAdded', added: 11 } },
                overview: { read: runtimeRead, refreshing: false, failure: null, failedAt: null, fresh: true, attention: [], held: [], face: 'allClear' },
            })
        ).toBeTruthy()
        expect(
            parseShellSnapshot({
                ...valid,
                sessions: {
                    reports: { state: 'ready', failure: null, items: [report], undo: { scanId: 'scn-1', subRequest: 0 }, problem: 'reason', unsaved: true },
                    investigation: { storage: 'protected', sessions: 1, transactions: 0, contents: 1 },
                    export: { kind: 'investigation', state: 'saved', name: 'investigation.json' },
                },
            })
        ).toBeTruthy()
        expect(parseShellSnapshot(valid)).toEqual(valid)
        expect(parseShellSnapshot(review({}))).toBeTruthy()
        expect(parseShellSnapshot(withScan({}))).toBeTruthy()
        expect(parseShellSnapshot(withScan({ progress: 'running', outcome: null, result: null }))).toBeTruthy()
        expect(
            parseShellSnapshot(withScan({ outcome: 'rejected', result: null, failure: { kind: 'unauthorized', status: 401 } }))
        ).toBeTruthy()
        expect(parseShellSnapshot(offline({}))).toBeTruthy()
        expect(parseShellSnapshot({ ...valid, changes: { staged: null, review: changeReview, results: [changeResult] } })).toBeTruthy()
        expect(
            parseShellSnapshot({
                ...valid,
                changes: { review: null, results: [], staged: { state: 'ready', token: 'sf-0011223344556677', name: 'k.txt', bytes: 3, lines: 1, failure: null } },
            })
        ).toBeTruthy()
        expect(
            parseShellSnapshot({ ...valid, changes: { staged: null, review: { ...changeReview, state: 'failed', failure: { kind: 'conflict' } }, results: [] } })
        ).toBeTruthy()
        expect(parseShellSnapshot(offline({ state: 'empty', downloads: [], localDrafts: [] }))).toBeTruthy()
        expect(parseShellSnapshot(offline({ downloads: [{ ...download, status: 'withdrawn' }] }))).toBeTruthy()
    })

    it.each([
        ['discarded counts missing', review({ discarded: {} })],
        ['negative discarded count', review({ discarded: { secretFields: -1, fileReferences: 0, otherFields: 0 } })],
        ['fractional candidate index', review({ candidates: [{ ...review({}).import.candidates[0], index: 0.5 }] })],
        ['reset failure as a bare reason', { ...valid, resetFailure: 'locked' }],
        ['unknown reset stage', { ...valid, resetFailure: { stage: 'everything', reason: 'locked' } }],
        ['durability flag missing', { ...valid, durabilityUnconfirmed: undefined }],
        [
            'endpoint review with a non-tenant id',
            {
                ...valid,
                endpointReview: {
                    review: 'rv-0011223344556677',
                    tenant: '../../etc',
                    fromOrigin: 'https://a.example',
                    to: { kind: 'region', region: 'europe' },
                    toOrigin: 'https://b.example',
                    removesScanCredential: true,
                },
            },
        ],
        ['tenant with an unknown scan state', { ...valid, tenants: [{ ...tenant(), scan: { state: 'exfiltrated', mode: null, failure: null } }] }],
        ['a tenant without its management view', { ...valid, tenants: [{ ...tenant(), management: undefined }] }],
        [
            'an unknown management state',
            { ...valid, tenants: [{ ...tenant(), management: { ...tenant().management, state: 'exfiltrated' } }] },
        ],
        [
            'a non-HTTPS management token origin',
            { ...valid, tenants: [{ ...tenant(), management: { ...tenant().management, tokenOrigin: 'http://auth.example' } }] },
        ],
        ['changes missing', { ...valid, changes: undefined }],
        ['sessions missing', { ...valid, sessions: undefined }],
        ['library missing', { ...valid, library: undefined }],
        ['a case of an unknown product', { ...valid, library: { ...valid.library, cases: [{ ...caseView, product: 'prisma-access' }] } }],
        ['a Runtime case with a payload', { ...valid, library: { ...valid.library, cases: [{ ...caseView, product: 'runtime-security' }] } }],
        ['a failed library without its reason', { ...valid, library: { ...valid.library, state: 'failed' } }],
        ['a library export of another kind', { ...valid, library: { ...valid.library, export: { kind: 'reports', state: 'saving', name: null } } }],
        ['overview missing', { ...valid, overview: undefined }],
        ['all clear without a fresh read', { ...valid, overview: { ...valid.overview, face: 'allClear' } }],
        [
            'a chart with its failure',
            { ...valid, overview: { ...valid.overview, read: { ...runtimeRead, chartFailure: 'unavailable' } } },
        ],
        [
            'an inbox item of an unknown tone',
            { ...valid, overview: { ...valid.overview, read: { ...runtimeRead, items: [{ ...inbox, tone: 'fine' }] } } },
        ],
        [
            'a negative session count',
            { ...valid, overview: { ...valid.overview, read: { ...runtimeRead, summary: { ...runtimeRead.summary, sessions: -1 } } } },
        ],
        ['an unknown reports state', { ...valid, sessions: { ...valid.sessions, reports: { ...valid.sessions.reports, state: 'syncing' } } }],
        ['failed reports without a reason', { ...valid, sessions: { ...valid.sessions, reports: { ...valid.sessions.reports, state: 'failed' } } }],
        [
            'a report with an unknown kind',
            { ...valid, sessions: { ...valid.sessions, reports: { ...valid.sessions.reports, items: [{ ...report, kind: 'suppression' }] } } },
        ],
        [
            'a report with an unknown verdict',
            { ...valid, sessions: { ...valid.sessions, reports: { ...valid.sessions.reports, items: [{ ...report, verdict: 'allow' }] } } },
        ],
        ['an unknown investigation storage', { ...valid, sessions: { ...valid.sessions, investigation: { storage: 'cloud', sessions: 0, transactions: 0, contents: 0 } } }],
        ['a saved export without its name', { ...valid, sessions: { ...valid.sessions, export: { kind: 'reports', state: 'saved', name: null } } }],
        ['a cancelled export with a name', { ...valid, sessions: { ...valid.sessions, export: { kind: 'reports', state: 'cancelled', name: 'x.csv' } } }],
        [
            'a staged file whose token is a path',
            { ...valid, changes: { review: null, results: [], staged: { state: 'ready', token: '/Users/x/k.txt', name: 'k.txt', bytes: 3, lines: 1, failure: null } } },
        ],
        [
            'a failed staged file without its reason',
            { ...valid, changes: { review: null, results: [], staged: { state: 'failed', token: null, name: null, bytes: null, lines: null, failure: null } } },
        ],
        ['a review of an unknown DLP kind', { ...valid, changes: { staged: null, review: { ...changeReview, kind: 'deploymentProfiles' }, results: [] } }],
        [
            'a failed review without its failure',
            { ...valid, changes: { staged: null, review: { ...changeReview, state: 'failed' }, results: [] } },
        ],
        ['a review with an unknown action', { ...valid, changes: { staged: null, review: { ...changeReview, action: 'forceDelete' }, results: [] } }],
        ['a review whose preview is not JSON', { ...valid, changes: { staged: null, review: { ...changeReview, after: '<b>x</b>' }, results: [] } }],
        ['an unsettled change with an outcome', { ...valid, changes: { staged: null, review: null, results: [{ ...changeResult, settled: false }] } }],
        ['a change with an unknown secret state', { ...valid, changes: { staged: null, review: null, results: [{ ...changeResult, secret: 'revealed' }] } }],
        ['a change failure with server text', { ...valid, changes: { staged: null, review: null, results: [{ ...changeResult, failure: { kind: 'service', failure: 'Forbidden: x' } }] } }],
        ['scans missing', { ...valid, scans: undefined }],
        ['a scan with a malformed operation', withScan({ operation: 'op-x' })],
        ['a result on a scan that did not complete', withScan({ outcome: 'unknown' })],
        ['a settled scan without an outcome', withScan({ outcome: null, result: null })],
        ['a running scan with an outcome', withScan({ progress: 'running', result: null, outcome: 'unknown' })],
        ['an unknown verdict value', withScan({ result: { ...scan.result, verdict: 'probablyFine' } })],
        ['an unknown failure kind', withScan({ outcome: 'rejected', result: null, failure: { kind: 'teapot', status: 418 } })],
        ['a non-HTTP failure status', withScan({ outcome: 'rejected', result: null, failure: { kind: 'httpOther', status: 42 } })],
        ['a detection without its flag', withScan({ result: { ...scan.result, promptDetected: [{ detector: 'dlp' }] } })],
        ['academy missing', { ...valid, academy: undefined }],
        ['an unknown academy state', { ...valid, academy: { state: 'hacked' } }],
        ['a non-HTTPS verification page', { ...valid, academy: { state: 'awaitingApproval', userCode: 'A', verificationUri: 'http://x', expiresIn: 1 } }],
        ['a malformed learner reference', { ...valid, academy: { state: 'signedIn', displayName: 'A', learner: 'cx-1' } }],
        ['an unknown academy failure', { ...valid, academy: { state: 'failed', failure: 'oops', vault: null, displayName: null } }],
        ['active context with a malformed reference', { ...valid, active: { tenant: tenant().id, context: 'not a context' } }],
        ['a missing Academy origin', { ...valid, academyOrigin: undefined }],
        ['a non-HTTPS Academy origin', { ...valid, academyOrigin: 'http://academy.example' }],
        ['an Academy origin with a path', { ...valid, academyOrigin: 'https://academy.example/verify' }],
        ['an Academy origin with credentials', { ...valid, academyOrigin: 'https://user@academy.example' }],
        ['offline work missing', { ...valid, academyOffline: undefined }],
        ['an unknown offline state', offline({ state: 'decrypted' })],
        ['offline work shown while signed out', { ...valid, academyOffline: offline({}).academyOffline }],
        ['downloads listed before offline work opens', offline({ state: 'opening' })],
        ['an unknown download status', offline({ downloads: [{ ...download, status: 'cached' }] })],
        ['a duplicated download', offline({ downloads: [download, download] })],
        ['a download with a path for an id', offline({ downloads: [{ ...download, courseId: '../x' }] })],
        ['a download starting at a unit path', offline({ downloads: [{ ...download, firstUnit: '../units' }] })],
        ['a download with a non-instant lease', offline({ downloads: [{ ...download, leaseExpiresAt: 'tomorrow' }] })],
        ['a local draft with a fractional version', offline({ localDrafts: [{ ...localDraft, baseVersion: 1.5 }] })],
        ['a reachability flag that is not a flag', offline({ reachable: 'yes' })],
    ])('rejects %s', (_case, snapshot) => {
        expect(() => parseShellSnapshot(snapshot)).toThrow(ContractViolation)
    })
})

describe('management result validation', () => {
    const search = { kind: 'detector', query: 'dlp', sessions: '[{"session_id":"s"}]', scanned: 100, complete: true, matchLimit: false, scan: null, cached: false }
    const page = {
        resource: 'profiles',
        offset: 0,
        limit: 25,
        items: '[{"profile_id":"p-1","profile_name":"A"}]',
        count: 1,
        nextOffset: null,
        complete: true,
        total: null,
    }

    it('accepts well-formed results', () => {
        expect(parseManagementResult({ kind: 'verified' })).toEqual({ kind: 'verified' })
        expect(parseManagementResult({ kind: 'page', page })).toBeTruthy()
        expect(parseManagementResult({ kind: 'page', page: { ...page, complete: false, nextOffset: 25 } })).toBeTruthy()
        expect(
            parseManagementResult({ kind: 'detail', detail: { resource: 'topics', id: 't', found: 'found', item: '{"topic_id":"t"}' } })
        ).toBeTruthy()
        expect(parseManagementResult({ kind: 'detail', detail: { resource: 'topics', id: 't', found: 'incomplete', item: null } })).toBeTruthy()
        expect(parseManagementResult({ kind: 'failed', failure: 'denied', outcome: 'rejected' })).toBeTruthy()
        expect(parseManagementResult({ kind: 'page', page: { ...page, resource: 'sessions' } })).toBeTruthy()
        expect(parseManagementResult({ kind: 'session', session: { session: '{"session_id":"s"}', actions: 1, more: false } })).toBeTruthy()
        expect(parseManagementResult({ kind: 'transaction', transaction: '{"scan_id":"x"}' })).toBeTruthy()
        expect(parseManagementResult({ kind: 'content', content: { prompt: 'p', response: null } })).toBeTruthy()
        expect(parseManagementResult({ kind: 'search', search })).toBeTruthy()
        expect(parseManagementResult({ kind: 'search', search: { ...search, kind: 'scanId', scan: '{"scan_id":"x"}' } })).toBeTruthy()
    })

    it.each([
        ['an unknown kind', { kind: 'raw', body: '<html>' }],
        ['an unknown resource', { kind: 'page', page: { ...page, resource: 'secrets' } }],
        ['a page larger than the native maximum', { kind: 'page', page: { ...page, limit: 101 } }],
        ['more items than the limit', { kind: 'page', page: { ...page, limit: 1, count: 2, items: '[{},{}]' } }],
        ['a count that disagrees with the items', { kind: 'page', page: { ...page, count: 3 } }],
        ['items that are not JSON', { kind: 'page', page: { ...page, items: 'not json' } }],
        ['items that are not objects', { kind: 'page', page: { ...page, items: '["a"]' } }],
        ['a complete page with a continuation', { kind: 'page', page: { ...page, nextOffset: 25 } }],
        ['an incomplete page without a continuation', { kind: 'page', page: { ...page, complete: false } }],
        ['a continuation that does not advance', { kind: 'page', page: { ...page, complete: false, nextOffset: 0 } }],
        ['an absent detail with an item', { kind: 'detail', detail: { resource: 'topics', id: 't', found: 'absent', item: '{}' } }],
        ['a found detail without an item', { kind: 'detail', detail: { resource: 'topics', id: 't', found: 'found', item: null } }],
        ['an unknown failure', { kind: 'failed', failure: 'teapot', outcome: 'rejected' }],
        ['a failure without an outcome', { kind: 'failed', failure: 'denied' }],
        ['a session that is not JSON', { kind: 'session', session: { session: '<b>', actions: 1, more: false } }],
        ['a transaction that is not an object', { kind: 'transaction', transaction: '[]' }],
        ['content that is not text', { kind: 'content', content: { prompt: 1, response: null } }],
        ['an unknown search kind', { kind: 'search', search: { ...search, kind: 'fullText' } }],
        ['search sessions that are not objects', { kind: 'search', search: { ...search, sessions: '["s"]' } }],
        ['a search without its coverage', { kind: 'search', search: { ...search, complete: undefined } }],
    ])('rejects %s', (_, value) => {
        expect(() => parseManagementResult(value)).toThrow(ContractViolation)
    })

    describe('Red Team and Model/skill observation', () => {
        const SCAN = '11111111-2222-4333-8444-555555555555'
        const redTeam = {
            readAt: 1,
            window: 30,
            statistics: { scans: 2, targets: 1, status: [{ label: 'COMPLETED', count: 2 }], risk: null, targetTypes: null, noScans: false },
            statisticsFailure: null,
            scans: { rows: [{ id: SCAN, status: 'COMPLETED', jobType: null, targetId: null, targetType: null, createdAt: null }], total: 1, complete: true },
            scansFailure: null,
        }
        const supply = {
            readAt: 1,
            models: null,
            modelsFailure: 'denied',
            skillStatistics: { uniqueSkills: 1, vulnerabilities: 0 },
            skillStatisticsFailure: null,
            skills: { rows: [{ pseudonym: 'a1b2c3d4', status: null, outcome: null, vulnerabilities: null, createdAt: null }], total: null, complete: true },
            skillsFailure: null,
        }
        const attacks = {
            scanId: SCAN,
            page: 1,
            rows: [{ number: 101, threat: true, severity: null, attackType: null, subCategory: 'Jailbreak', multiTurn: null }],
            total: 101,
            complete: true,
        }
        const bad = (value: unknown) => expect(() => parseManagementResult(value)).toThrow(ContractViolation)

        it('accepts v1-shaped reads', () => {
            expect(parseManagementResult({ kind: 'redTeam', observation: redTeam })).toBeTruthy()
            expect(parseManagementResult({ kind: 'supplyChain', observation: supply })).toBeTruthy()
            expect(parseManagementResult({ kind: 'attacks', attacks })).toBeTruthy()
        })

        it('rejects a source with both data and a failure, or with neither', () => {
            bad({ kind: 'redTeam', observation: { ...redTeam, statisticsFailure: 'denied' } })
            bad({ kind: 'redTeam', observation: { ...redTeam, scans: null } })
            bad({ kind: 'redTeam', observation: { ...redTeam, statistics: null, statisticsFailure: 'denied', scans: null, scansFailure: 'denied' } })
            bad({ kind: 'supplyChain', observation: { ...supply, models: null, modelsFailure: null } })
        })

        it('rejects windows, identifiers, pages and pseudonyms outside v1’s reads', () => {
            bad({ kind: 'redTeam', observation: { ...redTeam, window: 14 } })
            bad({ kind: 'redTeam', observation: { ...redTeam, scans: { ...redTeam.scans, rows: [{ ...redTeam.scans.rows[0], id: '../scan' }] } } })
            bad({ kind: 'redTeam', observation: { ...redTeam, scans: { ...redTeam.scans, rows: Array(101).fill(redTeam.scans.rows[0]) } } })
            bad({ kind: 'supplyChain', observation: { ...supply, skills: { ...supply.skills, rows: [{ ...supply.skills.rows[0], pseudonym: 'private-skill-name' }] } } })
            bad({ kind: 'attacks', attacks: { ...attacks, page: 10 } })
            bad({ kind: 'attacks', attacks: { ...attacks, rows: [{ ...attacks.rows[0], number: 1 }] } })
            bad({ kind: 'attacks', attacks: { ...attacks, rows: [{ ...attacks.rows[0], threat: 'yes' }] } })
        })
    })
})

describe('API Client validation', () => {
    const fields = { method: 'GET', url: 'https://a.example/', headers: '', query: '', body: '', bodyKind: 'text', settings: '' }
    const document = {
        id: 'list',
        relative: 'items/list.request.toml',
        revision: 'a'.repeat(32),
        fields,
        saved: fields,
        new: false,
        recovered: false,
        bodyEditable: true,
        schema: null,
        conflict: null,
        interrupted: false,
        metadataDirty: false,
    }
    const collection = {
        id: 'c14',
        name: 'Items API',
        folder: 'api',
        builtIn: false,
        complete: true,
        requests: [{ relative: 'items/list.request.toml', id: 'list', method: 'GET', url: 'https://a.example/' }],
        folders: ['items'],
        schemas: [],
        issues: [{ relative: 'bad.request.toml', kind: 'invalid' }],
        environments: [{ name: 'dev', origin: 'both' }],
        environment: 'dev',
        tabs: [{ id: 'list', relative: 'items/list.request.toml', method: 'GET', draft: true, new: false, recovered: false, conflict: 'changed', interrupted: false }],
        selected: 'list',
        secrets: [{ reference: 'dev-token', stored: true, managed: false }],
        unsaved: null,
        archives: 1,
    }
    const snapshot = (workbench: unknown) => ({ ...baseSnapshot, workbench }) as unknown as ShellSnapshot
    const bad = (value: unknown) => expect(() => parseWorkbenchResult(value)).toThrow(ContractViolation)

    it('accepts an open collection and every result shape', () => {
        expect(parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, collection }))).toBeTruthy()
        for (const result of [
            { kind: 'done' },
            { kind: 'document', document, notes: [{ kind: 'authRemoved', reference: 'import-1' }] },
            { kind: 'saved', document },
            { kind: 'saveRefused', refusal: { kind: 'invalid', problem: { field: 'headers', line: 2 } } },
            { kind: 'saveRefused', refusal: { kind: 'conflict' } },
            { kind: 'refused', problem: { kind: 'stale' } },
            { kind: 'refused', problem: { kind: 'curl', detail: 'shell' } },
            { kind: 'environment', name: 'dev', private: true, text: 'format_version = 1\n', revision: 'b'.repeat(64) },
            { kind: 'git', git: { state: 'ready', branch: 'main', head: 'abc', upstream: null, ahead: null, behind: null, changes: [], truncated: false } },
        ]) {
            expect(parseWorkbenchResult(result)).toEqual(result)
        }
    })

    it('rejects paths, identities and revisions outside what Rust issues', () => {
        expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, collection: { ...collection, requests: [{ ...collection.requests[0], relative: '../x' }] } }))).toThrow(
            ContractViolation
        )
        expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, collection: { ...collection, tabs: [{ ...collection.tabs[0], conflict: 'gone' }] } }))).toThrow(
            ContractViolation
        )
        expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, unprotected: 'no' }))).toThrow(ContractViolation)
        bad({ kind: 'document', document: { ...document, relative: '/etc/passwd' }, notes: [] })
        bad({ kind: 'document', document: { ...document, id: 'a b' }, notes: [] })
        bad({ kind: 'saved', document: { ...document, revision: 'not-a-revision' } })
        bad({ kind: 'saved', document: { ...document, fields: { ...fields, body: 1 } } })
        bad({ kind: 'refused', problem: { kind: 'curl', detail: 'other' } })
        bad({ kind: 'refused', problem: { kind: 'stale', detail: 'x' } })
        bad({ kind: 'saveRefused', refusal: { kind: 'invalid', problem: { field: 'cookies', line: null } } })
        bad({ kind: 'sent' })
    })
})
