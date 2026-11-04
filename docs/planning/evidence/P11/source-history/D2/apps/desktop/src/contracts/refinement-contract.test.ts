import { describe, expect, it } from 'vitest'
import { ContractViolation, parseShellSnapshot } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant } from '../test/native'

describe('refinement native contract', () => {
    it('accepts only the paired host color-theme values', () => {
        expect(parseShellSnapshot({ ...baseSnapshot, colorTheme: 'prismaAirs' }).colorTheme).toBe('prismaAirs')
        expect(parseShellSnapshot({ ...baseSnapshot, colorTheme: 'gruvbox' }).colorTheme).toBe('gruvbox')
        for (const colorTheme of [undefined, null, 'future-theme', {}, 1]) {
            expect(() => parseShellSnapshot({ ...baseSnapshot, colorTheme })).toThrow(ContractViolation)
        }
    })
    it('requires a real boolean for the native-owned management requirement', () => {
        const setup = {
            mode: 'add', context: 'cx-draft', tenant: tenant(), products: ['runtimeSecurity'],
            scanRequired: false, managementRequired: true, packs: [], workspace: null,
            verify: null, completable: false,
        }
        expect(parseShellSnapshot({ ...baseSnapshot, setup }).setup?.managementRequired).toBe(true)
        for (const managementRequired of [undefined, null, 'true', {}, 1]) {
            expect(() => parseShellSnapshot({ ...baseSnapshot, setup: { ...setup, managementRequired } })).toThrow(ContractViolation)
        }
    })
})
