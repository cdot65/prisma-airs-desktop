/**
 * Lab work kept on this Mac through the core's offline intents: saved as the
 * learner generation's JSON draft, read back only in its expected shape,
 * forgotten on request, and reported unkept when the core refuses or fails.
 */
import { describe, expect, it } from 'vitest'
import { nativeLocalDrafts } from './host-actions'
import { ACCOUNT, refuse, result, scriptedNative } from './test-support'

const ATTEMPT = '00000000-0000-4000-8000-0000000000a1'

describe('lab work kept on this Mac', () => {
    it('saves, reads back and forgets lab work as the learner generation', async () => {
        const kept = { attemptId: ATTEMPT, baseVersion: 3, draft: { debrief: 'kept' }, savedAt: '2026-10-18T12:00:00.000Z' }
        const native = scriptedNative({ '@saveLocalDraft': {}, '@loadLocalDraft': kept, '@clearLocalDraft': {} })
        const drafts = nativeLocalDrafts(native.client, ACCOUNT)
        await expect(drafts.save(ATTEMPT, 3, { debrief: 'kept' })).resolves.toBe(true)
        await expect(drafts.load(ATTEMPT)).resolves.toEqual({ baseVersion: 3, draft: { debrief: 'kept' }, savedAt: kept.savedAt })
        await expect(drafts.clear(ATTEMPT)).resolves.toBe(true)
        expect(native.intents).toEqual([
            { kind: 'saveLocalDraft', learner: ACCOUNT, attemptId: ATTEMPT, baseVersion: 3, draft: '{"debrief":"kept"}' },
            { kind: 'loadLocalDraft', learner: ACCOUNT, attemptId: ATTEMPT },
            { kind: 'clearLocalDraft', learner: ACCOUNT, attemptId: ATTEMPT },
        ])
    })

    it.each([
        ['nothing kept', null],
        ['a malformed version', { baseVersion: 'x', draft: {}, savedAt: 'now' }],
        ['a draft that is not an object', { baseVersion: 1, draft: 'text', savedAt: 'now' }],
    ])('reads back nothing for %s', async (_case, reply) => {
        const native = scriptedNative({ '@loadLocalDraft': reply })
        await expect(nativeLocalDrafts(native.client, ACCOUNT).load(ATTEMPT)).resolves.toBeNull()
    })

    it('reports work unkept, unreadable or not forgotten when the core refuses or fails', async () => {
        for (const reply of [refuse(), result({ kind: 'failed', failure: 'offlineFull', outcome: 'notSent' })]) {
            const native = scriptedNative({ '@saveLocalDraft': reply, '@loadLocalDraft': reply })
            const drafts = nativeLocalDrafts(native.client, ACCOUNT)
            await expect(drafts.save(ATTEMPT, 0, {})).resolves.toBe(false)
            // Unreadable kept work is reported, never answered as none.
            await expect(drafts.load(ATTEMPT)).rejects.toThrow()
        }
        // A clear the core did not confirm is reported, to be asked again.
        const failing = scriptedNative({ '@clearLocalDraft': result({ kind: 'failed', failure: 'offlineUnavailable', outcome: 'notSent' }) })
        await expect(nativeLocalDrafts(failing.client, ACCOUNT).clear(ATTEMPT)).resolves.toBe(false)
    })
})
