/**
 * Runtime validation of native shell snapshots: malformed C02 fields are
 * contract violations, never rendered.
 */
import { describe, expect, it } from 'vitest'
import { ContractViolation, parseShellSnapshot, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

const valid: ShellSnapshot = withTenant(baseSnapshot, tenant())

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
        ['active context with a malformed reference', { ...valid, active: { tenant: tenant().id, context: 'not a context' } }],
    ])('rejects %s', (_case, snapshot) => {
        expect(() => parseShellSnapshot(snapshot)).toThrow(ContractViolation)
    })
})
