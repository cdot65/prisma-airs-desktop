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
    items: [inbox], gaps: [], gateway: null,
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
    workspace: null,
    references: [],
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
    workspace: null,
    provisioning: null,
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
            scans: { rows: [{ id: SCAN, status: 'COMPLETED', jobType: null, targetId: null, targetType: null, createdAt: null }], total: 1, capped: false, complete: true },
            scansFailure: null,
        }
        const supply = {
            readAt: 1,
            models: null,
            modelsFailure: 'denied',
            skillStatistics: { uniqueSkills: 1, vulnerabilities: 0 },
            skillStatisticsFailure: null,
            skills: { rows: [{ pseudonym: 'a1b2c3d4', status: null, outcome: null, vulnerabilities: null, createdAt: null }], total: null, capped: false, complete: true },
            skillsFailure: null,
        }
        const attacks = {
            scanId: SCAN,
            page: 1,
            rows: [{ number: 101, threat: true, severity: null, attackType: null, subCategory: 'Jailbreak', multiTurn: null }],
            total: 101,
            capped: false,
            complete: true,
        }
        const bad = (value: unknown) => expect(() => parseManagementResult(value)).toThrow(ContractViolation)

        it('accepts v1-shaped reads', () => {
            expect(parseManagementResult({ kind: 'redTeam', observation: redTeam })).toBeTruthy()
            expect(parseManagementResult({ kind: 'supplyChain', observation: supply })).toBeTruthy()
            expect(parseManagementResult({ kind: 'attacks', attacks })).toBeTruthy()
            expect(parseManagementResult({ kind: 'attacks', attacks: { ...attacks, capped: true, complete: false } })).toBeTruthy()
        })

        it('rejects a total without its exactness, or a capped total called complete', () => {
            bad({ kind: 'attacks', attacks: { ...attacks, capped: undefined } })
            bad({ kind: 'attacks', attacks: { ...attacks, capped: true } })
            bad({ kind: 'redTeam', observation: { ...redTeam, scans: { ...redTeam.scans, capped: true } } })
            bad({ kind: 'supplyChain', observation: { ...supply, skills: { ...supply.skills, capped: 'no' } } })
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

    describe('AI Gateway (C16)', () => {
        const WS = '16f7e90d-382a-4e78-b577-1b01eb5f8297'
        const bad = (value: unknown) => expect(() => parseManagementResult(value)).toThrow(ContractViolation)
        const list = { resource: 'providers', workspace: WS, rows: '[{"id":"p1"}]', count: 1, total: 3, complete: false }
        const prerequisites = {
            workspace: WS, routes: { ...list, resource: 'routes' }, routesFailure: null, providers: null, providersFailure: 'denied',
            integrations: { ...list, resource: 'integrations', workspace: null }, integrationsFailure: null, serviceKeys: null,
            serviceKeysFailure: 'timeout', inferenceKeys: 0, scopeName: 'scope-a', scope: 'unbound', dataPlane: 'granted',
            adminPlane: 'granted', iamPlane: 'granted',
        }
        const gateway = (g: unknown) => ({ kind: 'gateway', gateway: g })

        it('accepts what Rust issues', () => {
            expect(parseManagementResult(gateway({ kind: 'list', list }))).toBeTruthy()
            expect(parseManagementResult(gateway({ kind: 'detail', detail: { resource: 'routes', id: 'r1', found: 'incomplete', item: null } }))).toBeTruthy()
            expect(parseManagementResult(gateway({ kind: 'models', models: { integration: 'i1', models: '[{"slug":"o3"}]', count: 1, total: null, complete: true, allowAll: false } }))).toBeTruthy()
            expect(parseManagementResult(gateway({ kind: 'prerequisites', prerequisites }))).toBeTruthy()
        })

        it('rejects inventories, details and prerequisites Rust never issues', () => {
            bad(gateway({ kind: 'list', list: { ...list, count: 2 } }))
            bad(gateway({ kind: 'list', list: { ...list, total: 0 } }))
            bad(gateway({ kind: 'list', list: { ...list, workspace: null } }))
            bad(gateway({ kind: 'list', list: { ...list, resource: 'integrations' } }))
            bad(gateway({ kind: 'list', list: { ...list, resource: 'secrets' } }))
            bad(gateway({ kind: 'detail', detail: { resource: 'routes', id: '../r', found: 'found', item: '{}' } }))
            bad(gateway({ kind: 'detail', detail: { resource: 'routes', id: 'r1', found: 'absent', item: '{}' } }))
            bad(gateway({ kind: 'prerequisites', prerequisites: { ...prerequisites, routesFailure: 'denied' } }))
            bad(gateway({ kind: 'prerequisites', prerequisites: { ...prerequisites, providersFailure: null } }))
            bad(gateway({ kind: 'prerequisites', prerequisites: { ...prerequisites, scope: 'maybe' } }))
        })

        it('validates the workspace choice in the snapshot', () => {
            const workspace = { id: WS, name: 'Team A', slug: 'team-a', scopeName: 'scope-a' }
            const view = { workspaces: [workspace], complete: true, selected: { reference: 'gw-aaaaaaaaaaaaaaaaaaaaaaaa', workspace, restored: true }, remembered: null, rememberedMissing: false, local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false }, playground: null, files: null }
            expect(parseShellSnapshot({ ...valid, gateway: view })).toBeTruthy()
            for (const broken of [
                { ...view, selected: { ...view.selected, reference: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' } },
                { ...view, workspaces: [{ ...workspace, id: 'a b' }] },
                { ...view, rememberedMissing: true },
                { ...view, complete: undefined },
            ]) {
                expect(() => parseShellSnapshot({ ...valid, gateway: broken })).toThrow(ContractViolation)
            }
        })
    })

    describe('Gateway changes (C17)', () => {
        const workspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-a' }
        const gatewayReview = {
            ...changeReview,
            kind: 'gatewayProviders',
            action: 'delete',
            target: 'p1',
            after: null,
            workspace,
            references: ['Route: default-route'],
        }
        const provisioning = {
            scopeName: 'scope-c', scope: 'completed', workspace: 'unknown', binding: 'notStarted', workspaceId: null,
            workspaceSlug: null, cleanup: 'retained', scopeRead: 'found', boundRead: null, workspaceRead: 'incomplete',
        }
        const provisioned = { ...changeResult, kind: 'gatewayWorkspaces', action: 'create', target: '', secret: 'none', readback: 'differs', provisioning }
        const changes = (review: unknown, results: unknown[] = []) => ({ ...valid, changes: { staged: null, review, results } })

        it('accepts what Rust issues', () => {
            expect(parseShellSnapshot(changes(gatewayReview))).toBeTruthy()
            expect(parseShellSnapshot(changes(null, [provisioned, { ...changeResult, kind: 'gatewayIntegrations', action: 'models', workspace }]))).toBeTruthy()
        })

        it('rejects references, steps and states Rust never issues', () => {
            for (const broken of [
                changes({ ...gatewayReview, action: 'update' }),
                changes({ ...gatewayReview, state: 'failed', failure: { kind: 'problem', problem: 'referenced' } }),
                changes({ ...gatewayReview, references: [7] }),
                changes({ ...gatewayReview, kind: 'gatewaySecrets' }),
                changes({ ...gatewayReview, workspace: { ...workspace, id: 'a b' } }),
                changes(null, [{ ...provisioned, kind: 'gatewayProviders' }]),
                changes(null, [{ ...provisioned, provisioning: { ...provisioning, scope: 'rolledBack' } }]),
                changes(null, [{ ...provisioned, provisioning: { ...provisioning, workspaceRead: 'differs' } }]),
                changes(null, [{ ...changeResult, readback: 'probably' }]),
                changes(null, [{ ...provisioned, provisioning: { ...provisioning, cleanup: 'undone' } }]),
                changes(null, [{ ...provisioned, provisioning: { ...provisioning, cleanup: undefined } }]),
                changes(null, [{ ...changeResult, failure: { kind: 'workspaceMoved' } }]),
            ]) {
                expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
            }
        })

        it('accepts a stopped readback, a workspace switch, an unidentified workspace and each cleanup', () => {
            const results = [
                { ...changeResult, readback: 'stopped' },
                { ...changeResult, outcome: 'notSent', readback: null, failure: { kind: 'workspaceChanged' } },
                ...['notNeeded', 'retained', 'deleted', 'failed', 'unknown'].map((cleanup) => ({
                    ...provisioned,
                    provisioning: { ...provisioning, cleanup, workspaceRead: 'ambiguous' },
                })),
            ]
            expect(parseShellSnapshot(changes(null, results))).toBeTruthy()
        })

        describe('work kept on this Mac', () => {
            const draft = {
                id: `gd-${'a'.repeat(24)}`, workspace, kind: 'gatewayProviders', action: 'update', target: 'p1',
                body: '{"note": "n"}', savedAt: '2026-10-20T00:00:00Z',
            }
            const record = {
                id: `gr-${'b'.repeat(24)}`, workspace: null, kind: 'gatewayWorkspaces', action: 'create', target: '',
                name: null, body: '{}', outcome: 'unknown', readback: 'incomplete', provisioning, settledAt: '2026-10-20T00:00:00Z',
            }
            const local = { state: 'ready', failure: null, drafts: [draft], records: [record], playground: [], runs: [], files: [], problem: null, unsaved: false }
            const gateway = (l: unknown) => ({
                ...valid,
                gateway: { workspaces: null, complete: false, selected: null, remembered: null, rememberedMissing: false, local: l, playground: null, files: null },
            })

            it('accepts what Rust issues', () => {
                expect(parseShellSnapshot(gateway(local))).toBeTruthy()
                expect(parseShellSnapshot(gateway({ ...local, state: 'failed', failure: 'unreadable', drafts: [], records: [] }))).toBeTruthy()
                expect(parseShellSnapshot(gateway({ ...local, problem: 'credential' }))).toBeTruthy()
                expect(parseShellSnapshot(gateway({ ...local, problem: 'incomplete' }))).toBeTruthy()
            })

            it('rejects kept work Rust never issues', () => {
                for (const broken of [
                    { ...local, drafts: [{ ...draft, id: 'rv-1' }] },
                    { ...local, drafts: [{ ...draft, kind: 'profiles' }] },
                    { ...local, drafts: [{ ...draft, body: 7 }] },
                    { ...local, drafts: [{ ...draft, workspace: { ...workspace, id: 'a b' } }] },
                    { ...local, records: [{ ...record, kind: 'gatewayProviders' }] },
                    { ...local, records: [{ ...record, outcome: 'rolledBack' }] },
                    { ...local, state: 'failed', failure: null },
                    { ...local, failure: 'unreadable' },
                    { ...local, problem: 'secret' },
                    { ...local, unsaved: undefined },
                    undefined,
                ]) {
                    expect(() => parseShellSnapshot(gateway(broken))).toThrow(ContractViolation)
                }
            })
        })
    })
})

describe('Gateway inference and observation (C18)', () => {
    const WS = '16f7e90d-382a-4e78-b577-1b01eb5f8297'
    const workspace = { id: WS, name: 'Team A', slug: 'team-a', scopeName: 'scope-a' }
    const series = { points: [{ x: '2026-10-20T00:00:00Z', y: 4 }], total: 4, errors: 1, partial: false }
    const usage = {
        workspace: WS, days: 7, requests: series, requestsFailure: null, cost: null, costFailure: 'unavailable', tokens: series,
        tokensFailure: null, latency: { ...series, errors: null }, latencyFailure: null,
        models: { rows: [{ model: 'gpt-4o', provider: 'openai', requests: 3, cost: null, totalTokens: 9, successRate: 0.9 }], total: 1, complete: true, partial: false },
        modelsFailure: null,
    }
    const row = { id: 'log-1', createdAt: '2026-10-20T00:00:00Z', model: 'gpt-4o', provider: 'openai', status: 446, units: 1, cost: 0.2, traceId: 'airs-1', cacheStatus: null, apiKeyId: null }
    const logs = { workspace: 'team-a', days: 7, page: 0, trace: 'airs-1', status: null, rows: [row], total: 1, more: false, capped: false, partial: false }
    const policy = { id: 'ul-1', name: 'n', status: 'active', limitType: 'cost', groupBy: ['api_key'], creditLimit: 5000, alertThreshold: 80, periodicReset: 'monthly', nextResetAt: null, value: null, unit: null, target: null }
    const limits = { workspace: WS, usage: { policies: [policy], total: 1, complete: true }, usageFailure: null, rate: null, rateFailure: 'denied' }
    const observed = (o: unknown) => ({ kind: 'gateway', gateway: { kind: 'observed', observed: o } })
    const bad = (value: unknown) => expect(() => parseManagementResult(value)).toThrow(ContractViolation)
    const run = {
        operation: 'op-12', workspace, api: 'chat', model: '@openai-prod/gpt-4o', route: 'cfg-1', file: null, trace: 'airs-0123456789abcdef', state: 'stopped',
        output: 'partial', outcome: 'unknown', failure: null, sent: true, answered: true, servedModel: 'gpt-4o', responseId: 'c1',
        finish: null, usage: { prompt: 3, completion: null, total: null },
    }
    const kept = {
        id: `gi-${'c'.repeat(24)}`, workspace, api: 'chat', model: 'm', route: null, trace: 'airs-1', end: 'stopped', outcome: 'unknown', failure: null,
        answered: true, outputBytes: 7, usage: null, settledAt: '2026-10-20T00:00:00Z',
    }
    const draft = { id: `gp-${'d'.repeat(24)}`, workspace, api: 'responses', model: 'm', route: null, system: null, prompt: 'p', file: null, savedAt: '2026-10-20T00:00:00Z' }
    const file = { id: 'file-abc123', filename: 'notes.txt', bytes: 12, createdAt: 1760000000, purpose: 'assistants', status: 'processed' }
    const action = { action: 'upload', filename: 'notes.txt', file: 'file-abc123', bytes: 12, trace: 'airs-0123456789abcdef', outcome: 'completed', failure: null, sent: true }
    const files = {
        workspace, files: [file], more: false, reading: false, readFailure: null,
        chosen: { token: `gf-${'e'.repeat(24)}`, name: 'report.csv', bytes: 8 }, choosing: false, chooseFailure: null, action,
    }
    const keptFile = {
        id: `gf-${'f'.repeat(24)}`, workspace, action: 'upload', filename: 'report.csv', file: null, bytes: 8, trace: 'airs-1', outcome: 'unknown',
        failure: 'unavailable', settledAt: '2026-10-20T00:00:00Z',
    }
    const gateway = (extra: Record<string, unknown>, local: Record<string, unknown> = {}) => ({
        ...valid,
        gateway: {
            ...valid.gateway,
            local: { ...valid.gateway.local, state: 'ready', playground: [draft], runs: [kept], ...local },
            ...extra,
        },
    })
    const key = { state: 'ready', mode: 'keychain', failure: null, target: { workspace, endpoint: 'https://gw.example/v1' } }

    it('accepts what Rust issues', () => {
        expect(parseManagementResult(observed({ kind: 'usage', usage }))).toBeTruthy()
        expect(parseManagementResult(observed({ kind: 'logs', logs }))).toBeTruthy()
        expect(parseManagementResult(observed({ kind: 'logs', logs: { ...logs, capped: true } }))).toBeTruthy()
        expect(parseManagementResult(observed({ kind: 'limits', limits }))).toBeTruthy()
        expect(parseShellSnapshot(gateway({ playground: run }))).toBeTruthy()
        expect(parseShellSnapshot(gateway({ playground: { ...run, state: 'running', outcome: null } }))).toBeTruthy()
        expect(parseShellSnapshot(gateway({ playground: { ...run, api: 'responses', file: 'file-abc123', state: 'incomplete', outcome: 'completed', finish: 'max_output_tokens' } }))).toBeTruthy()
        expect(parseShellSnapshot(gateway({ files }, { files: [keptFile], playground: [{ ...draft, file: 'file-abc123' }] }))).toBeTruthy()
        expect(parseShellSnapshot(gateway({ files: { ...files, files: null, chosen: null, action: { ...action, outcome: null, file: null } } }))).toBeTruthy()
        // A full list after uploads (the bound, with more reported), and times
        // at and past the renderable range (the latter as unknown).
        const full = Array.from({ length: 200 }, (_, i) => ({ ...file, id: `file-${i}`, createdAt: i === 0 ? null : i === 1 ? 8_640_000_000_000 : 1760000000 }))
        expect(parseShellSnapshot(gateway({ files: { ...files, files: full, more: true } }))).toBeTruthy()
        expect(parseShellSnapshot({ ...valid, tenants: [{ ...tenant(), inference: key }] })).toBeTruthy()
    })

    it('rejects observations, runs and keys Rust never issues', () => {
        bad(observed({ kind: 'usage', usage: { ...usage, costFailure: null } }))
        bad(observed({ kind: 'usage', usage: { ...usage, days: 2 } }))
        bad(observed({ kind: 'usage', usage: { ...usage, requests: { ...series, partial: true } } }))
        bad(observed({ kind: 'usage', usage: { ...usage, requests: { ...series, total: -1 } } }))
        bad(observed({ kind: 'logs', logs: { ...logs, rows: [{ ...row, traceId: 'other' }] } }))
        bad(observed({ kind: 'logs', logs: { ...logs, page: 40 } }))
        bad(observed({ kind: 'logs', logs: { ...logs, more: true, capped: true } }))
        bad(observed({ kind: 'logs', logs: { ...logs, capped: undefined } }))
        bad(observed({ kind: 'limits', limits: { ...limits, rateFailure: null } }))
        bad(observed({ kind: 'secrets' }))
        for (const broken of [
            gateway({ playground: { ...run, state: 'running' } }),
            gateway({ playground: { ...run, outcome: null } }),
            gateway({ playground: { ...run, output: 'x'.repeat(64 * 1024 + 1) } }),
            gateway({ playground: { ...run, trace: 'a b' } }),
            gateway({ playground: { ...run, failure: 'exploded' } }),
            gateway({ playground: { ...run, api: 'files' } }),
            gateway({}, { playground: [{ ...draft, api: undefined }] }),
            gateway({}, { runs: [{ ...kept, output: 'partial' }] }),
            gateway({}, { runs: [{ ...kept, id: 'gx-1' }] }),
            gateway({}, { playground: [{ ...draft, route: 'a b' }] }),
            gateway({ playground: { ...run, file: 'file-abc123' } }),
            gateway({ playground: { ...run, state: 'cut' } }),
            gateway({}, { playground: [{ ...draft, file: '../etc' }] }),
            gateway({ files: { ...files, chosen: { ...files.chosen, path: '/Users/x/report.csv' } } }),
            gateway({ files: { ...files, chosen: { ...files.chosen, token: 'report.csv' } } }),
            gateway({ files: { ...files, chosen: { ...files.chosen, bytes: 4 * 1024 * 1024 + 1 } } }),
            gateway({ files: { ...files, files: [{ ...file, content: 'CANARY' }] } }),
            gateway({ files: { ...files, files: [{ ...file, id: 'a b' }] } }),
            gateway({ files: { ...files, files: [{ ...file, createdAt: 8_640_000_000_001 }] } }),
            gateway({ files: { ...files, files: [{ ...file, createdAt: -1 }] } }),
            gateway({ files: { ...files, more: true, files: Array.from({ length: 201 }, (_, i) => ({ ...file, id: `file-${i}` })) } }),
            gateway({ files: { ...files, action: { ...action, action: 'read' } } }),
            gateway({ files: { ...files, chooseFailure: 'exploded' } }),
            gateway({}, { files: [{ ...keptFile, content: 'a,b' }] }),
            gateway({}, { files: [{ ...keptFile, trace: 'a b' }] }),
            { ...valid, tenants: [{ ...tenant(), inference: { ...key, target: null } }] },
            { ...valid, tenants: [{ ...tenant(), inference: { ...key, target: { ...key.target, endpoint: 'https://gw.example/v1?key=x' } } }] },
            { ...valid, tenants: [{ ...tenant(), inference: undefined }] },
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})

describe('native Harness (C19)', () => {
    const workspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
    const engine = { available: true, protocol: 'airs-desktop/1', runtimeVersion: '0.1.7-desktop.2', engineCommit: '9331a5839bc6a763d94dee7584746f58bbbe1181' }
    const conversation = {
        id: 'op-7', workspace, endpoint: 'https://gateway.example/rt/v1', phase: 'ready', failure: null, stop: null,
        entries: [
            { id: 'u-1', role: 'operator', text: 'Say hello', state: 'complete' },
            { id: 'u-1/msg_1', role: 'assistant', text: 'Hello', state: 'complete' },
        ],
        lastTurn: 'completed', usage: { input: 10, output: 6, total: 16 }, cleanup: null, runtimeVersion: '0.1.7-desktop.2',
        source: 'workspaceKey', historyId: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d', savedAt: 1790000000, saveFailure: null,
        unsent: null, signInFailure: null, project: null, write: false, approvals: [], proposals: [], mcp: [],
    }
    const harness = (c: Record<string, unknown> | null, e: Record<string, unknown> = {}) => ({
        ...valid,
        harness: { engine: { ...engine, ...e }, conversation: c, signIn: null, history: null, project: null, projectRefused: null, mcp: null },
    })

    it('accepts what Rust issues', () => {
        expect(parseShellSnapshot(harness(null, { available: false }))).toBeTruthy()
        expect(parseShellSnapshot(harness(conversation))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, phase: 'ended', stop: 'sleep', cleanup: 'forced' }))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, phase: 'failed', failure: 'exited', cleanup: 'graceful' }))).toBeTruthy()
        // The bounds themselves: 200 entries, a 256 KiB answer.
        const many = Array.from({ length: 200 }, (_, i) => ({ id: `u-${i}`, role: 'operator', text: 'x', state: 'complete' }))
        expect(parseShellSnapshot(harness({ ...conversation, entries: many }))).toBeTruthy()
        const long = [{ id: 'u-1/m', role: 'assistant', text: 'y'.repeat(256 * 1024), state: 'truncated' }]
        expect(parseShellSnapshot(harness({ ...conversation, entries: long, lastTurn: 'truncated' }))).toBeTruthy()
    })

    it('holds token counts to the safe integers the core publishes (MAX_SAFE_COUNT)', () => {
        const max = Number.MAX_SAFE_INTEGER
        expect(parseShellSnapshot(harness({ ...conversation, usage: { input: max, output: max, total: max } }))).toBeTruthy()
        // The core never publishes more (session.rs MAX_SAFE_COUNT), so a
        // larger value is a contract violation, as JSON beyond 2^53 cannot be
        // read exactly here.
        for (const usage of [
            { input: max + 1, output: 0, total: 0 },
            JSON.parse('{"input": 18446744073709551615, "output": 0, "total": 0}'),
        ]) {
            expect(() => parseShellSnapshot(harness({ ...conversation, usage }))).toThrow(ContractViolation)
        }
    })

    it('rejects conversations, entries and engines Rust never issues', () => {
        const entry = conversation.entries[1]
        for (const broken of [
            { ...valid, harness: undefined },
            harness(null, { protocol: 'airs-desktop/2' }),
            harness(null, { engineCommit: '9331a58' }),
            harness(null, { available: 'yes' }),
            harness({ ...conversation, id: 'conversation-1' }),
            harness({ ...conversation, endpoint: 'http://gateway.example/rt/v1' }),
            harness({ ...conversation, phase: 'paused' }),
            harness({ ...conversation, phase: 'failed', failure: null }),
            harness({ ...conversation, phase: 'ended', cleanup: null }),
            harness({ ...conversation, failure: 'gatewaySaid: boom' }),
            harness({ ...conversation, stop: 'crashed' }),
            harness({ ...conversation, lastTurn: 'maybe' }),
            harness({ ...conversation, cleanup: 'eventually' }),
            harness({ ...conversation, usage: { input: -1, output: 0, total: 0 } }),
            harness({ ...conversation, entries: Array.from({ length: 201 }, (_, i) => ({ ...entry, id: `e-${i}` })) }),
            harness({ ...conversation, entries: [{ ...entry, text: 'y'.repeat(256 * 1024 + 1) }] }),
            harness({ ...conversation, entries: [{ ...entry, role: 'operator', text: 'x'.repeat(32 * 1024 + 1) }] }),
            harness({ ...conversation, entries: Array.from({ length: 5 }, (_, i) => ({ ...entry, id: `e-${i}`, text: 'z'.repeat(256 * 1024) })) }),
            harness({ ...conversation, entries: [{ ...entry, role: 'system' }] }),
            harness({ ...conversation, entries: [{ ...entry, state: 'pending' }] }),
            harness({ ...conversation, entries: [{ ...entry, apiKey: 'gw-x' }] }),
            harness({ ...conversation, entries: [{ ...entry, id: '' }] }),
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})

describe('Harness company sign-in and protected history (C20)', () => {
    const workspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
    const engine = { available: true, protocol: 'airs-desktop/1', runtimeVersion: '0.1.7-desktop.2', engineCommit: '9331a5839bc6a763d94dee7584746f58bbbe1181' }
    const id = '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d'
    const signIn = {
        workspace, endpoint: 'https://gateway.example/rt/v1', issuer: 'https://id.example/oauth2', clientId: 'desktop',
        audience: 'https://gateway.example', selected: true, account: 'alice@example.com', step: null, failure: null,
    }
    const saved = { id, title: 'Remember periwinkle', updated: 1790000000, entries: 4, source: 'companySignIn' }
    const opened = {
        id, title: 'Remember periwinkle', updated: 1790000000, source: 'workspaceKey', notContinuable: 'otherCredential',
        entries: [{ id: 'u-1', role: 'operator', text: 'Remember periwinkle', state: 'complete' }],
    }
    const history = {
        local: false, state: 'ready', conversations: [saved], unreadable: [], failure: null, full: false, opened, busy: false, problem: null,
        export: { kind: 'harnessConversation', state: 'saved', name: 'harness-x.md' },
    }
    const harness = (s: unknown, h: unknown) => ({ ...valid, harness: { engine, conversation: null, signIn: s, history: h, project: null, projectRefused: null, mcp: null } })

    it('accepts what Rust issues', () => {
        expect(parseShellSnapshot(harness(signIn, history))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...signIn, account: null, step: 'browser' }, { ...history, opened: null, export: null }))).toBeTruthy()
        expect(parseShellSnapshot(harness(null, { ...history, state: 'failed', failure: 'locked', conversations: [] }))).toBeTruthy()
        const sixteen = Array.from({ length: 16 }, (_, i) => ({ ...saved, id: id.replace(/d$/, i.toString(16)) }))
        expect(parseShellSnapshot(harness(null, { ...history, conversations: sixteen, full: true }))).toBeTruthy()
    })

    it('rejects sign-ins, saved conversations and exports Rust never issues', () => {
        for (const broken of [
            harness({ ...signIn, issuer: 'http://id.example' }, null),
            harness({ ...signIn, endpoint: 'http://gateway.example' }, null),
            harness({ ...signIn, step: 'waiting' }, null),
            harness({ ...signIn, failure: 'idpSaid: no' }, null),
            harness({ ...signIn, selected: 'yes' }, null),
            harness(null, { ...history, conversations: [{ ...saved, id: 'cr-1' }] }),
            harness(null, { ...history, conversations: [{ ...saved, title: 't'.repeat(81) }] }),
            harness(null, { ...history, conversations: [{ ...saved, entries: 201 }] }),
            harness(null, { ...history, conversations: [{ ...saved, source: 'password' }] }),
            harness(null, { ...history, conversations: Array.from({ length: 17 }, () => saved) }),
            harness(null, { ...history, state: 'failed', failure: null }),
            harness(null, { ...history, failure: 'gone' }),
            harness(null, { ...history, problem: 'vanished' }),
            harness(null, { ...history, unreadable: ['../escape'] }),
            harness(null, { ...history, local: 'yes' }),
            harness(null, { ...history, opened: { ...opened, notContinuable: 'maybe' } }),
            harness(null, { ...history, opened: { ...opened, entries: [{ ...opened.entries[0], role: 'system' }] } }),
            harness(null, { ...history, export: { kind: 'cases', state: 'saved', name: 'x' } }),
            { ...valid, harness: { engine, conversation: null, signIn: null } },
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})

describe('Harness tools and project (C21)', () => {
    const workspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
    const engine = { available: true, protocol: 'airs-desktop/1', runtimeVersion: '0.1.7-desktop.2', engineCommit: '9331a5839bc6a763d94dee7584746f58bbbe1181' }
    const PROJECT = '/Users/someone/work/agent-demo'
    const approval = { id: '0123456789abcdef01234567', kind: 'command', command: "/bin/zsh -c 'ls'", cwd: PROJECT, reason: null, files: [], server: null, tool: null, arguments: null }
    const proposal = {
        id: 'P-1',
        summary: 'Block prompt injection in the default profile',
        draft: { kind: 'profiles', action: 'update', target: 'default', body: '{"name":"default"}' },
        state: 'staged',
        review: null,
    }
    const conversation = {
        id: 'op-7', workspace, endpoint: 'https://gateway.example/rt/v1', phase: 'answering', failure: null, stop: null,
        entries: [
            { id: 'u-1', role: 'operator', text: 'List the folder', state: 'complete' },
            { id: 'u-1/call-1', role: 'command', text: "/bin/zsh -c 'ls'", state: 'streaming' },
            { id: 'u-1/call-2', role: 'tool', text: 'Read the security profiles', state: 'complete' },
            { id: 'u-1/call-3', role: 'fileChange', text: `${PROJECT}/notes.txt`, state: 'declined' },
        ],
        lastTurn: null, usage: null, cleanup: null, runtimeVersion: '0.1.7-desktop.2',
        source: 'workspaceKey', historyId: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d', savedAt: null, saveFailure: null,
        unsent: null, signInFailure: null, project: PROJECT, write: true, approvals: [approval], proposals: [proposal], mcp: [],
    }
    const harness = (c: Record<string, unknown> | null, project: unknown = { path: PROJECT, write: true }) => ({
        ...valid,
        harness: { engine, conversation: c, signIn: null, history: null, project, projectRefused: null, mcp: null },
    })

    it('accepts the tool, approval and proposal views Rust issues', () => {
        expect(parseShellSnapshot(harness(null, null))).toBeTruthy()
        expect(parseShellSnapshot(harness(conversation))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, approvals: [{ ...approval, kind: 'fileChange', command: null, files: [`${PROJECT}/a.txt`] }] }))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, phase: 'confirmWrite', approvals: [] }))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, proposals: [{ ...proposal, state: 'inReview', review: 'rv-1' }] }))).toBeTruthy()
        expect(parseShellSnapshot(harness({ ...conversation, proposals: [{ ...proposal, state: 'closed', review: 'rv-1' }] }))).toBeTruthy()
        // Without a folder: tools and proposals only.
        expect(parseShellSnapshot(harness({ ...conversation, project: null, write: false, approvals: [] }, null))).toBeTruthy()
        const eight = Array.from({ length: 8 }, (_, i) => ({ ...approval, id: `${i}`.repeat(24) }))
        expect(parseShellSnapshot(harness({ ...conversation, approvals: eight }))).toBeTruthy()
    })

    it('refuses tool, approval and proposal views Rust never issues', () => {
        for (const broken of [
            harness(conversation, { path: 'relative/path', write: false }),
            harness(conversation, { path: PROJECT, write: 'yes' }),
            harness(conversation, { path: PROJECT, write: true, extra: 1 }),
            { ...harness(conversation), harness: { ...harness(conversation).harness, projectRefused: '' } },
            // Changes are only ever allowed in a folder.
            harness({ ...conversation, project: null, approvals: [] }),
            // Approvals need a folder; file changes need changes allowed.
            harness({ ...conversation, project: null, write: false }),
            harness({ ...conversation, write: false, approvals: [{ ...approval, kind: 'fileChange', files: ['x'] }] }),
            harness({ ...conversation, approvals: [{ ...approval, id: 'not-a-token' }] }),
            harness({ ...conversation, approvals: [{ ...approval, kind: 'network' }] }),
            harness({ ...conversation, approvals: [{ ...approval, files: ['x'] }] }),
            harness({ ...conversation, approvals: [{ ...approval, command: 'x'.repeat(8 * 1024 + 1) }] }),
            // A file change names every file whole (C21 review R2-F3).
            harness({ ...conversation, approvals: [{ ...approval, kind: 'fileChange', command: null, files: [] }] }),
            harness({ ...conversation, approvals: [{ ...approval, kind: 'fileChange', command: null, files: [`${PROJECT}/${'p'.repeat(1024)}`] }] }),
            harness({ ...conversation, approvals: Array.from({ length: 9 }, () => approval) }),
            // Gateway changes are never proposed, nor anything but create, update or delete.
            harness({ ...conversation, proposals: [{ ...proposal, draft: { ...proposal.draft, kind: 'gatewayRoutes' } }] }),
            harness({ ...conversation, proposals: [{ ...proposal, draft: { ...proposal.draft, action: 'rotate' } }] }),
            harness({ ...conversation, proposals: [{ ...proposal, draft: { ...proposal.draft, body: '[1]' } }] }),
            harness({ ...conversation, proposals: [{ ...proposal, summary: ' ' }] }),
            harness({ ...conversation, proposals: [{ ...proposal, state: 'inReview' }] }),
            harness({ ...conversation, proposals: [{ ...proposal, review: 'rv-1' }] }),
            harness({ ...conversation, proposals: [{ ...proposal, state: 'closed' }] }),
            harness({ ...conversation, proposals: [proposal, proposal] }),
            harness({ ...conversation, proposals: [{ ...proposal, id: 'P-9' }] }),
            harness({ ...conversation, phase: 'confirmWrite', write: false, approvals: [] }),
            harness({ ...conversation, entries: [{ id: 'x', role: 'system', text: 'x', state: 'complete' }] }),
            harness({ ...conversation, approvals: undefined }),
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})

describe('workspace MCP servers (C22)', () => {
    const workspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
    const engine = { available: true, protocol: 'airs-desktop/1', runtimeVersion: '0.1.7-desktop.2', engineCommit: '9331a5839bc6a763d94dee7584746f58bbbe1181' }
    const server = {
        id: 'srv-tools-0001', name: 'Tools', namespace: 'tools', url: 'https://gw.example/tools/mcp', status: 'active',
        supported: true, enabled: true, signedIn: true, stale: false, expired: null, step: null, failure: null,
    }
    const mcp = { workspace: workspace.id, state: 'ready', failure: null, complete: true, servers: [server] }
    const approval = {
        id: '0123456789abcdef01234567', kind: 'mcpTool', command: null, cwd: null, reason: null, files: [],
        server: 'tools', tool: 'lookup', arguments: '{"query":"record-1"}',
    }
    const conversation = {
        id: 'op-7', workspace, endpoint: 'https://gateway.example/rt/v1', phase: 'answering', failure: null, stop: null,
        entries: [{ id: 'u-1/c1', role: 'mcp', text: 'tools · lookup', state: 'streaming' }],
        lastTurn: null, usage: null, cleanup: null, runtimeVersion: '0.1.7-desktop.2',
        source: 'workspaceKey', historyId: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d', savedAt: null, saveFailure: null,
        unsent: null, signInFailure: null, project: null, write: false, approvals: [approval], proposals: [],
        mcp: [{ name: 'tools', status: 'ready', signedOut: false }],
    }
    const harness = (c: Record<string, unknown> | null, m: unknown = mcp) => ({
        ...valid,
        harness: { engine, conversation: c, signIn: null, history: null, project: null, projectRefused: null, mcp: m },
    })

    it('accepts the MCP views Rust issues', () => {
        expect(parseShellSnapshot(harness(null, null))).toBeTruthy()
        expect(parseShellSnapshot(harness(conversation))).toBeTruthy()
        expect(parseShellSnapshot(harness(null, { ...mcp, state: 'failed', failure: 'unavailable', servers: [] }))).toBeTruthy()
        const odd = { ...server, id: 'odd id!', namespace: 'odd', supported: false, enabled: false, signedIn: false }
        expect(parseShellSnapshot(harness(null, { ...mcp, servers: [server, odd] }))).toBeTruthy()
        const eight = Array.from({ length: 8 }, (_, i) => ({ ...server, id: `srv-${i}`, namespace: `s${i}` }))
        expect(parseShellSnapshot(harness(null, { ...mcp, servers: eight }))).toBeTruthy()
    })

    it('refuses MCP views Rust never issues', () => {
        const nine = Array.from({ length: 9 }, (_, i) => ({ ...server, id: `srv-${i}`, namespace: `s${i}` }))
        for (const broken of [
            harness(null, { ...mcp, servers: nine }),
            harness(null, { ...mcp, servers: [{ ...server, namespace: 'Bad-Name' }] }),
            harness(null, { ...mcp, servers: [{ ...server, url: 'http://gw.example/tools/mcp' }] }),
            harness(null, { ...mcp, servers: [{ ...server, supported: false }] }),
            harness(null, { ...mcp, servers: [{ ...server, stale: true }] }),
            harness(null, { ...mcp, servers: [{ ...server, id: 'odd id!' }] }),
            harness(null, { ...mcp, servers: [server, server] }),
            harness(null, { ...mcp, servers: [{ ...server, expired: 'gone' }] }),
            harness(null, { ...mcp, state: 'failed', failure: null }),
            harness({ ...conversation, approvals: [{ ...approval, server: null }] }),
            harness({ ...conversation, approvals: [{ ...approval, kind: 'command', command: 'ls' }] }),
            harness({ ...conversation, approvals: [{ ...approval, command: 'ls' }] }),
            harness({ ...conversation, approvals: [{ ...approval, server: 'Not A Name' }] }),
            harness({ ...conversation, mcp: [{ name: 'tools', status: 'connected', signedOut: false }] }),
            harness({ ...conversation, mcp: [{ name: 'tools', status: 'ready', signedOut: false, error: 'x' }] }),
            harness({ ...conversation, entries: [{ id: 'x', role: 'mcpServer', text: 'x', state: 'complete' }] }),
            harness({ ...conversation, mcp: undefined }),
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
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

    describe('Send (C15)', () => {
        const record = {
            id: 'run-0123456789abcdef',
            request: 'list',
            relative: 'items/list.request.toml',
            at: 1_760_000_000,
            method: 'GET',
            url: 'http://127.0.0.1:8080/items',
            class: 'anonymous',
            operation: null,
            state: 'completed',
            failure: null,
            uncertain: false,
            status: 200,
            headers: [['content-type', 'application/json']],
            redirect: null,
            elapsedMs: 4,
            bytes: 2,
            stored: true,
        }
        const review = {
            review: 'rv-00000000000000aa',
            request: 'list',
            operation: 'IAM/Delete',
            method: 'DELETE',
            url: 'https://127.0.0.1/x',
            headers: [],
            body: '',
            bodyBytes: 0,
            files: ['data/a.json'],
            destination: 'https://127.0.0.1',
            expires: 1,
        }

        it('accepts what Rust issues', () => {
            for (const result of [
                { kind: 'ran', run: { record, preview: '{}', previewBytes: 2, tokenDropped: false } },
                { kind: 'reviewRequired', review },
                { kind: 'sendRefused', refusal: { kind: 'render', problem: { kind: 'unresolved', name: 'host' } } },
                { kind: 'sendRefused', refusal: { kind: 'secret', reference: 'dev', failure: 'locked' } },
                { kind: 'sendRefused', refusal: { kind: 'unknownEndpoint' } },
                { kind: 'history', runs: [record], more: false },
                { kind: 'responseSaved', bytes: 2, name: 'a-response.json' },
                { kind: 'saveResponseFailed', failure: 'destination' },
            ]) {
                expect(parseWorkbenchResult(result)).toEqual(result)
            }
            const running = [{ run: 'run-00000000000000bb', request: 'list', method: 'POST', bytes: 1 }]
            expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, running, review, network: 'live' }))).not.toThrow()
        })

        it('rejects runs, reviews and refusals Rust never issues', () => {
            bad({ kind: 'ran', run: { record: { ...record, id: 'run-1' }, preview: null, previewBytes: 0, tokenDropped: false } })
            bad({ kind: 'ran', run: { record: { ...record, class: 'admin' }, preview: null, previewBytes: 0, tokenDropped: false } })
            bad({ kind: 'ran', run: { record: { ...record, relative: '../x' }, preview: null, previewBytes: 0, tokenDropped: false } })
            bad({ kind: 'reviewRequired', review: { ...review, review: 'approved' } })
            bad({ kind: 'reviewRequired', review: { ...review, files: ['/etc/hosts'] } })
            bad({ kind: 'sendRefused', refusal: { kind: 'preapproved' } })
            bad({ kind: 'sendRefused', refusal: { kind: 'secret', reference: 'dev', failure: 'leaked' } })
            const run = { run: 'run-00000000000000bb', request: 'list', method: 'POST', bytes: 1 }
            expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, running: [run, run, run, run, run] }))).toThrow(ContractViolation)
            expect(() => parseShellSnapshot(snapshot({ ...baseSnapshot.workbench, network: 'anywhere' }))).toThrow(ContractViolation)
        })
    })
})

describe('reviewed v1 import (C23)', () => {
    const plan = {
        review: 'rv-0123456789abcdef',
        items: [
            {
                index: 0,
                category: 'cases',
                label: 'Prod',
                target: 'Ours',
                status: { kind: 'ready' },
                counts: { new: 1, present: 0, conflict: 1, skipped: 0 },
                partial: null,
            },
            {
                index: 1,
                category: 'conversations',
                label: 'Prod',
                target: null,
                status: { kind: 'blocked', problem: 'keychainLocked' },
                counts: { new: 0, present: 0, conflict: 0, skipped: 0 },
                partial: null,
            },
        ],
    }
    const receipt = {
        id: 'imp-0123456789abcdef',
        created: 1_790_000_000,
        state: 'applying',
        interrupted: true,
        items: [{ category: 'cases', label: 'Prod', state: 'writing', written: 2, kept: 0 }],
        canResume: true,
        canRollBack: true,
    }
    const view = {
        ...baseSnapshot.v1Import,
        source: 'ready',
        tenants: [{ index: 0, label: 'Prod', tsgId: '1234567890', sessionOnly: false, matches: ['tn-0123456789abcdef'] }],
        categories: [{ category: 'cases', found: null, problem: null }],
        notMapped: ['navigation'],
        plan,
        receipts: [receipt],
    }
    const snapshot = (v: unknown) => ({ ...valid, v1Import: v })

    it('accepts what Rust issues', () => {
        expect(parseShellSnapshot(snapshot(baseSnapshot.v1Import))).toBeTruthy()
        expect(parseShellSnapshot(snapshot(view))).toBeTruthy()
        expect(parseShellSnapshot(snapshot({ ...view, plan: null, activity: 'rollingBack', problem: 'sourceChanged' }))).toBeTruthy()
    })

    it('rejects categories, statuses, receipts and bounds Rust never issues', () => {
        const item = plan.items[0]
        for (const broken of [
            { ...valid, v1Import: undefined },
            snapshot({ ...view, source: 'somewhere' }),
            snapshot({ ...view, categories: [{ category: 'passwords', found: 1, problem: null }] }),
            snapshot({ ...view, problem: 'gatewaySaid: boom' }),
            snapshot({ ...view, activity: 'syncing' }),
            snapshot({ ...view, tenants: [{ ...view.tenants[0], matches: ['tenant-1'] }] }),
            snapshot({ ...view, plan: { ...plan, review: 'review-1' } }),
            snapshot({ ...view, plan: { ...plan, items: [{ ...item, status: { kind: 'blocked' } }] } }),
            snapshot({ ...view, plan: { ...plan, items: [{ ...item, status: { kind: 'ready', problem: 'missing' } }] } }),
            snapshot({ ...view, plan: { ...plan, items: [{ ...item, counts: { new: -1, present: 0, conflict: 0, skipped: 0 } }] } }),
            snapshot({ ...view, receipts: [{ ...receipt, id: 'imp-1' }] }),
            snapshot({ ...view, receipts: [{ ...receipt, state: 'paused' }] }),
            snapshot({ ...view, receipts: Array.from({ length: 9 }, () => receipt) }),
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})

describe('local diagnostics (C24)', () => {
    const view = { available: true, events: 12, retentionDays: 14, export: 'saved' }
    const snapshot = (v: unknown) => ({ ...valid, diagnostics: v })

    it('accepts what Rust issues', () => {
        expect(parseShellSnapshot(snapshot(view))).toBeTruthy()
        expect(parseShellSnapshot(snapshot({ ...view, available: false, events: 0, export: null }))).toBeTruthy()
    })

    it('rejects a view Rust never issues', () => {
        for (const broken of [
            { ...valid, diagnostics: undefined },
            snapshot({ ...view, events: -1 }),
            snapshot({ ...view, retentionDays: 365 }),
            snapshot({ ...view, export: 'uploaded' }),
            snapshot({ ...view, path: '/Users/someone/Library/Logs' }),
        ]) {
            expect(() => parseShellSnapshot(broken)).toThrow(ContractViolation)
        }
    })
})
