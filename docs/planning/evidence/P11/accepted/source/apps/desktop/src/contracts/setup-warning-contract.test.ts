import { describe, expect, it } from 'vitest'
import { ContractViolation, parseShellSnapshot } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

const warning = { id: 'vw-12', product: 'modelSkill', reason: 'Model scans: forbidden; Skill scans: available.', recordedSecs: 1_790_000_000 }
const check = { version: 1, line: 'modelSkill', tsgId: '1762204418', origin: 'https://api.scm.paloaltonetworks.com', workspace: null }
const recheck = { checkedSecs: 1_790_000_100, state: 'warning', reason: 'Model scans still need a role.', equivalent: true, basis: 'recordedScope' }
const snapshot = (entry: unknown) => withTenant(baseSnapshot, { ...tenant(), setupWarnings: [entry] } as ReturnType<typeof tenant>)

describe('setup warning metadata boundary', () => {
    it('accepts legacy warnings and complete versioned metadata, including an honestly incomplete workspace binding', () => {
        expect(parseShellSnapshot(snapshot(warning))).toBeTruthy()
        expect(parseShellSnapshot(snapshot({ ...warning, check, recheck }))).toBeTruthy()
        expect(parseShellSnapshot(snapshot({ ...warning, check: { ...check, line: 'gateway', workspace: { id: 'workspace-id', slug: '', scopeName: '' } } }))).toBeTruthy()
        expect(parseShellSnapshot(snapshot({ ...warning, recheck: { ...recheck, state: 'pass', equivalent: false, basis: 'unknown' } }))).toBeTruthy()
    })

    it('rejects malformed check versions, lines, origins and workspace shapes', () => {
        for (const bad of [null, {}, { ...check, version: 2 }, { ...check, line: 'futureLine' }, { ...check, tsgId: '' }, { ...check, origin: 'http://untrusted.invalid' }, { ...check, origin: 'https://example.invalid/path' }, { ...check, workspace: undefined }, { ...check, workspace: { id: '', slug: 'team', scopeName: 'team-a' } }, { ...check, workspace: { id: 'id', slug: 3, scopeName: 'team-a' } }]) {
            expect(() => parseShellSnapshot(snapshot({ ...warning, check: bad }))).toThrow(ContractViolation)
        }
    })

    it('rejects malformed recheck evidence instead of treating it as an authentication or scope result', () => {
        for (const bad of [null, {}, { ...recheck, checkedSecs: -1 }, { ...recheck, checkedSecs: 1.5 }, { ...recheck, state: 'authenticated' }, { ...recheck, equivalent: 'true' }, { ...recheck, reason: {} }, { ...recheck, basis: 'assumedSameTenant' }]) {
            expect(() => parseShellSnapshot(snapshot({ ...warning, recheck: bad }))).toThrow(ContractViolation)
        }
        for (const resolvedSecs of [null, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, 'now']) expect(() => parseShellSnapshot(snapshot({ ...warning, resolvedSecs }))).toThrow(ContractViolation)
    })
})
