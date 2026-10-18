/**
 * Native host actions Academy's pages ask for (print sheet, avatar upload,
 * avatar readback) and offline work (downloads, lab work kept on this Mac),
 * dispatched as intents of the current learner generation
 * and observed like learner calls. Each reports what is known of its
 * effect: done, cancelled in a native sheet, failed with nothing changed,
 * or possibly applied (Academy may have recorded it).
 */
import type { HostActionOutcome, LocalDraft, LocalDrafts } from '@ai-security-academy/learner'
import type { AcademyCallResult, DesktopClient, Intent, LearnerRef } from '@prisma-airs/desktop-contracts'

type HostIntent = Extract<Intent, { kind: 'printPage' | 'uploadAvatar' | 'loadAcademyImage' }>

async function run(client: DesktopClient, intent: HostIntent): Promise<AcademyCallResult | 'unconfirmed' | null> {
    let response
    try {
        response = await client.dispatch(intent)
    } catch {
        return 'unconfirmed'
    }
    if (response.outcome === 'refusal' || !response.receipt.operation) return null
    try {
        return await client.observeAcademyCall(response.receipt.operation)
    } catch {
        return 'unconfirmed'
    }
}

/** The outcome of a host action that may change learner state. */
export async function hostAction(client: DesktopClient, intent: HostIntent): Promise<HostActionOutcome> {
    const result = await run(client, intent)
    // Admission or the result could not be confirmed: only an action that
    // can change Academy state may have applied.
    if (result === 'unconfirmed') return intent.kind === 'uploadAvatar' ? 'mayHaveApplied' : 'failed'
    if (result === null) return 'failed'
    if (result.kind === 'data') return 'done'
    if (result.outcome === 'unknown') return 'mayHaveApplied'
    if (result.kind === 'failed' && result.failure === 'cancelled' && result.outcome === 'notSent') return 'cancelled'
    return 'failed'
}

/** An Academy avatar as a data URL, or null when it cannot be read. */
export async function loadAcademyImage(client: DesktopClient, intent: HostIntent): Promise<string | null> {
    const result = await run(client, intent)
    if (!result || result === 'unconfirmed' || result.kind !== 'data') return null
    try {
        const src = (JSON.parse(result.data) as { json?: { src?: unknown } }).json?.src
        return typeof src === 'string' && src.startsWith('data:image/') ? src : null
    } catch {
        return null
    }
}

export type OfflineIntent = Extract<
    Intent,
    { kind: 'downloadCourse' | 'removeDownload' | 'saveLocalDraft' | 'loadLocalDraft' | 'clearLocalDraft' }
>

/** An offline intent's result; null when it was refused or could not be confirmed. */
export async function offlineCall(client: DesktopClient, intent: OfflineIntent): Promise<AcademyCallResult | null> {
    try {
        const response = await client.dispatch(intent)
        if (response.outcome === 'refusal' || !response.receipt.operation) return null
        return await client.observeAcademyCall(response.receipt.operation)
    } catch {
        return null
    }
}

function localDraft(data: string): LocalDraft | null {
    try {
        const value = (JSON.parse(data) as { json?: unknown }).json
        if (!value || typeof value !== 'object') return null
        const { baseVersion, draft, savedAt } = value as Record<string, unknown>
        return Number.isSafeInteger(baseVersion) && typeof savedAt === 'string' && draft && typeof draft === 'object'
            ? { baseVersion: baseVersion as number, draft, savedAt }
            : null
    } catch {
        return null
    }
}

/**
 * Lab work Academy has not confirmed, sealed in the core for the signed-in
 * learner generation. The core never sends it by itself.
 */
export function nativeLocalDrafts(client: DesktopClient, learner: LearnerRef): LocalDrafts {
    return {
        async save(attemptId, baseVersion, draft) {
            const result = await offlineCall(client, {
                kind: 'saveLocalDraft',
                learner,
                attemptId,
                baseVersion,
                draft: JSON.stringify(draft ?? {}),
            })
            return result?.kind === 'data'
        },
        async load(attemptId) {
            const result = await offlineCall(client, { kind: 'loadLocalDraft', learner, attemptId })
            // Kept work that could not be read is not "none": the page leaves it untouched.
            if (result?.kind !== 'data') throw new Error('kept lab work could not be read')
            return localDraft(result.data)
        },
        async clear(attemptId) {
            const result = await offlineCall(client, { kind: 'clearLocalDraft', learner, attemptId })
            return result?.kind === 'data'
        },
    }
}
