/**
 * Academy's learner pages keep presentation-only state in WebView storage
 * under `academy-` keys: the per-account completion-moment latch
 * (`academy-completion-moments:<accountId>`) and the unit contents rail
 * preference. Server truth always wins, so the latch survives sign-out as
 * on Academy web; a completed local-data reset forgets it with everything
 * else the desktop kept about the learner.
 */
export const ACADEMY_STORAGE_PREFIX = 'academy-'

/** Removes every Academy presentation key; returns how many were removed. */
export function forgetAcademyPresentation(storage: Storage | undefined = globalThis.localStorage): number {
    if (!storage) return 0
    try {
        const keys: string[] = []
        for (let i = 0; i < storage.length; i += 1) {
            const key = storage.key(i)
            if (key?.startsWith(ACADEMY_STORAGE_PREFIX)) keys.push(key)
        }
        for (const key of keys) storage.removeItem(key)
        return keys.length
    } catch {
        // Storage unavailable: nothing was kept there.
        return 0
    }
}
