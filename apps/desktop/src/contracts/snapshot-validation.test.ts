/**
 * Runtime validation of native shell snapshots: malformed C02 fields are
 * contract violations, never rendered.
 */
import { describe, expect, it } from 'vitest'
import { ContractViolation, parseShellSnapshot, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

const valid: ShellSnapshot = withTenant(baseSnapshot, tenant())

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

describe('shell snapshot validation', () => {
    it('accepts a well-formed snapshot with C02 fields', () => {
        expect(parseShellSnapshot(valid)).toEqual(valid)
        expect(parseShellSnapshot(review({}))).toBeTruthy()
        expect(parseShellSnapshot(withScan({}))).toBeTruthy()
        expect(parseShellSnapshot(withScan({ progress: 'running', outcome: null, result: null }))).toBeTruthy()
        expect(
            parseShellSnapshot(withScan({ outcome: 'rejected', result: null, failure: { kind: 'unauthorized', status: 401 } }))
        ).toBeTruthy()
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
    ])('rejects %s', (_case, snapshot) => {
        expect(() => parseShellSnapshot(snapshot)).toThrow(ContractViolation)
    })
})
