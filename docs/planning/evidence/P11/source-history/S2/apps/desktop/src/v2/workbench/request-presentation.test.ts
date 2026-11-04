import { describe, expect, it } from 'vitest'
import { prettyJson } from './request-rows'

describe('API JSON presentation preserves JSON tokens', () => {
    it('indents without rounding large numbers or rewriting string escapes', () => {
        const raw = '{"integer":9007199254740993,"decimal":1.2300e+50,"escaped":"\\u0061\\/","duplicate":1,"duplicate":2}'
        const shown = prettyJson(raw)
        expect(shown).toContain('9007199254740993')
        expect(shown).toContain('1.2300e+50')
        expect(shown).toContain('"\\u0061\\/"')
        expect(shown?.match(/"duplicate"/g)).toHaveLength(2)
        expect(raw).toBe('{"integer":9007199254740993,"decimal":1.2300e+50,"escaped":"\\u0061\\/","duplicate":1,"duplicate":2}')
    })

    it('rejects invalid or truncated JSON instead of presenting a repaired response', () => {
        for (const raw of ['', '  \n', '{"x":', '{"x":1,}', 'true false', '[1,', '{"x":"unterminated']) expect(prettyJson(raw)).toBeNull()
    })

    it('formats empty/nested containers and primitive values without changing their tokens', () => {
        expect(prettyJson('[{},[],true,false,null,-0,1e3,"a b"]')).toBe('[\n  {},\n  [],\n  true,\n  false,\n  null,\n  -0,\n  1e3,\n  "a b"\n]')
        expect(prettyJson('  "\\u0061"  ')).toBe('"\\u0061"')
    })

    it('falls back to raw presentation for excessively large or deeply nested previews', () => {
        expect(prettyJson(JSON.stringify('x'.repeat(256 * 1024)))).toBeNull()
        expect(prettyJson('['.repeat(65) + '0' + ']'.repeat(65))).toBeNull()
    })
})
