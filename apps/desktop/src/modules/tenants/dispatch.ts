import type { Intent, Refusal } from '@prisma-airs/desktop-contracts'

/** What the presentation learned about one delivered intent. */
export type DispatchOutcome =
    /** Admitted; `operation` names native work the intent started, if any. */
    | { kind: 'admitted'; operation: string | null }
    | { kind: 'refused'; refusal: Refusal }
    /** The desktop did not confirm admission; the current state is shown. */
    | { kind: 'unknown' }
    | { kind: 'failed' }

export type Dispatch = (intent: Intent) => Promise<DispatchOutcome>

const REFUSAL_TEXT: Record<Refusal, string> = {
    staleContext: 'The active tenant changed before this was applied. Check the current tenant and try again.',
    capabilityMissing: 'That is not available for the current tenant.',
    invalidInput: 'Check the entered values and try again.',
    reviewInvalid: 'This review is no longer valid. Start a new review.',
    reviewConsumed: 'This review was already used.',
    busy: 'The desktop is busy with a previous request. Try again in a moment.',
    lockedStore: 'The Keychain is locked. Unlock it and try again.',
    deniedStore: 'Local desktop data or the credential store is not available.',
    alreadyProcessed: 'That request was already processed.',
    sequenceGap: 'The request was not delivered in order. Try again.',
    shuttingDown: 'The desktop is shutting down.',
    decisionPending: 'A Harness decision is pending; resolve it before switching tenants.',
}

/** Sanitized operator text for an outcome that did not apply. */
export function outcomeMessage(outcome: DispatchOutcome): string | null {
    switch (outcome.kind) {
        case 'admitted':
            return null
        case 'refused':
            return REFUSAL_TEXT[outcome.refusal]
        case 'unknown':
            return 'The desktop did not confirm the request. The current state is shown; check it before trying again.'
        case 'failed':
            return 'The request could not be delivered.'
    }
}
