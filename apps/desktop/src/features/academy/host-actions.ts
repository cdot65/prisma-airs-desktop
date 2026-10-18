/**
 * Native host actions Academy's pages ask for (print sheet, avatar upload,
 * avatar readback), dispatched as intents of the current learner generation
 * and observed like learner calls. Each reports what is known of its
 * effect: done, cancelled in a native sheet, failed with nothing changed,
 * or possibly applied (Academy may have recorded it).
 */
import type { HostActionOutcome } from '@ai-security-academy/learner'
import type { AcademyCallResult, DesktopClient, Intent } from '@prisma-airs/desktop-contracts'

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
