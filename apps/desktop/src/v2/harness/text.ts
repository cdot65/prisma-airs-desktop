import type {
    ConversationView,
    HarnessFailure,
    HarnessPhase,
    HarnessStop,
    HistoryFailure,
    HistoryFormat,
    McpSignInFailure,
    NotContinuable,
    Refusal,
    SignInFailure,
    SignInStep,
    TurnEnd,
} from '@prisma-airs/desktop-contracts'
import { type DispatchOutcome, outcomeMessage } from '../../features/tenants/dispatch'

/**
 * The Harness's words, as the C19–C22 screens said them, kept here so the v2
 * screen imports no legacy view (C38). Each table is the one the core's
 * sanitized failure kinds map to; nothing here is model text.
 */

export const HARNESS_FAILURE_TEXT: Record<HarnessFailure, string> = {
    engineUnavailable: 'The bundled engine is missing or is not the one this app was built with. Nothing was started.',
    credentialMissing: 'The inference key is missing. Store it again in the AI Gateway Playground.',
    credentialLocked: 'The Keychain is locked. Unlock it and start again.',
    credentialDenied: 'macOS did not allow access to the inference key.',
    startFailed: 'The engine could not be started on this Mac.',
    bootstrapRejected: 'The engine refused this workspace, key or endpoint.',
    timeout: 'The engine did not answer in time and was stopped.',
    protocol: 'The engine sent something unexpected and was stopped.',
    bounds: 'The engine sent more than this app accepts and was stopped.',
    exited: 'The engine stopped unexpectedly. Anything being answered was not finished.',
    refused: 'The engine refused the request.',
    turnFailed: 'The answer failed. The Gateway or the model ended it with an error.',
    cleanupFailed: 'The engine could not be stopped and removed in time. Quit the app to end it.',
    restoreFailed: 'The saved conversation could not be continued in a new engine. It is still saved; start a new conversation.',
    signInNeeded: 'Your company sign-in must be renewed. Sign in again, then start the conversation.',
    mcpTooLarge: 'The workspace MCP sign-ins are too large to give the engine together. Turn some servers off and start again.',
}

export const PHASE_TEXT: Record<HarnessPhase, string> = {
    starting: 'Starting the engine…',
    ready: 'Ready.',
    answering: 'Answering…',
    interrupting: 'Stopping the answer…',
    renewing: 'Renewing your sign-in and restarting the engine with the conversation…',
    confirmWrite: 'Waiting for you to allow changes in the folder again.',
    ending: 'Ending the conversation…',
    ended: 'Ended.',
    failed: 'Failed.',
}

export const STOP_TEXT: Record<HarnessStop, string> = {
    ended: 'You ended this conversation.',
    sleep: 'This conversation ended when the Mac went to sleep.',
    quit: 'This conversation ended when the app quit.',
    deadline: 'This conversation reached its eight-hour limit and ended.',
}

export const TURN_TEXT: Record<TurnEnd, string> = {
    completed: 'Answered.',
    interrupted: 'The answer was stopped.',
    truncated: 'The answer reached 256 KiB and was stopped; the rest was not kept.',
    // D046: the engine retries a failing Gateway itself; the app stops it at
    // its first reconnect, so up to five requests may have reached it.
    failed:
        'The answer failed: the Gateway or the model ended it with an error. The engine may have sent the message to the Gateway up to five times before this app stopped it, and each may have been processed; nothing was undone.',
    refused: 'The engine did not take that message.',
}

/** How the conversation stands, in one line (the C19 Harness's words). */
export function conversationText(c: ConversationView): string {
    if (c.phase === 'failed' && c.failure) return HARNESS_FAILURE_TEXT[c.failure]
    if (c.phase === 'ended' && c.stop) return STOP_TEXT[c.stop]
    if (c.phase === 'ready' && c.lastTurn) return TURN_TEXT[c.lastTurn]
    return PHASE_TEXT[c.phase]
}

// ---- Company sign-in (C20) ----

export const SIGN_IN_FAILURE_TEXT: Record<SignInFailure, string> = {
    invalidConfig:
        'The identity provider’s configuration could not be used: it must name this issuer, support PKCE with S256 and keep its endpoints on the issuer’s own address.',
    network: 'The identity provider could not be reached. Try again.',
    invalidResponse: 'The identity provider returned something this app does not accept.',
    denied: 'Sign-in was declined.',
    timedOut: 'The browser did not come back within five minutes.',
    cancelled: 'Sign-in was cancelled.',
    browserUnavailable: 'The browser could not be opened.',
    callbackUnavailable: 'The sign-in callback could not be opened on this Mac.',
    invalidToken: 'A token from the identity provider did not verify (signature, issuer, audience, client or time).',
    opaqueToken: 'The identity provider’s Gateway token is not a signed JWT, which this app requires.',
    wrongWorkspace: 'The token is for another Gateway workspace.',
    differentAccount: 'A different account signed in. Sign out first to use another account.',
    sessionExpired: 'Your sign-in has expired. Sign in again to send.',
    storeFailed: 'The sign-in could not be kept in the Keychain.',
}

export const SIGN_IN_STEP_TEXT: Record<SignInStep, string> = {
    contacting: 'Contacting the identity provider…',
    browser: 'Continue in your browser. This app is waiting for it to come back.',
    completing: 'Verifying the sign-in…',
}

export const GENERIC_FAILURE = 'That was not done. Check the values and try again.'

/**
 * A panel action the desktop did not confirm (DESIGN-V2 §7): the action is
 * named as Not confirmed, never as not done, because it may have happened
 * (a permission change, a sign-out, a permanent deletion); the operator
 * reconciles through the state the panel shows, and nothing is resent.
 */
export function notConfirmedText(action: string, check: string): string {
    return `Not confirmed: ${action}. The desktop did not confirm it and it may have been done. ${check}`
}

/**
 * The line a panel shows for an action that did not apply: an unknown
 * delivery stays Not confirmed (above); a refusal says why (the refusal's
 * own reason, or the panel's wording for it); a delivery that failed before
 * it reached the desktop is not done.
 */
export function actionOutcomeText(outcome: DispatchOutcome, action: string, check: string, refusedLine?: (refusal: Refusal) => string | undefined): string | null {
    switch (outcome.kind) {
        case 'admitted':
            return null
        case 'unknown':
            return notConfirmedText(action, check)
        case 'refused':
            return refusedLine?.(outcome.refusal) ?? `That was not done. ${outcomeMessage(outcome)}`
        case 'failed':
            return `That was not done. ${outcomeMessage(outcome)}`
    }
}

export const SAVE_REFUSAL_TEXT: Partial<Record<Refusal, string>> = {
    invalidInput: 'Issuer and Gateway endpoint must be https URLs without a query string.',
    busy: 'Sign out of the current company sign-in first.',
    staleContext: 'The workspace changed; choose it again in AI Gateway.',
}

export function saveFailureText(outcome: DispatchOutcome): string {
    if (outcome.kind === 'refused') return SAVE_REFUSAL_TEXT[outcome.refusal] ?? outcomeMessage(outcome) ?? GENERIC_FAILURE
    return outcomeMessage(outcome) ?? GENERIC_FAILURE
}

export const SIGN_IN_NEEDS =
    'Company sign-in needs the identity provider’s issuer URL, a client ID, the Gateway audience and the Gateway endpoint. The provider must allow loopback redirects to http://127.0.0.1 on any port (path /callback).'

// ---- Workspace MCP servers (C22) ----

export const MCP_SIGN_IN_FAILURE_TEXT: Record<McpSignInFailure, string> = {
    notOffered: 'The server did not ask for a sign-in, so this app cannot use it.',
    denied: 'The server refused access.',
    notFound: 'The server was not found at its catalog address.',
    unavailable: 'The server is unavailable.',
    invalidMetadata: 'The server’s sign-in details point elsewhere or lack what this app needs, so they are not used.',
    registrationNotOffered: 'The server does not let this app register for a sign-in.',
    network: 'The server could not be reached. Try again.',
    invalidResponse: 'The server returned something this app does not accept.',
    timedOut: 'The browser did not come back within five minutes.',
    cancelled: 'Sign-in was cancelled.',
    browserUnavailable: 'The browser could not be opened.',
    callbackUnavailable: 'The sign-in callback could not be opened on this Mac.',
    sessionExpired: 'The sign-in has expired. Sign in again to use its tools.',
    storeFailed: 'The sign-in could not be kept in the Keychain.',
}

export const MCP_STEP_TEXT: Record<SignInStep, string> = {
    contacting: 'Contacting the server…',
    browser: 'Continue in your browser. This app is waiting for it to come back.',
    completing: 'Completing the sign-in…',
}

/** At most eight servers per conversation (the engine's bound). */
export const MCP_MAX_ON = 8

// ---- Saved conversations (C20) ----

export const HISTORY_FAILURE_TEXT: Record<HistoryFailure, string> = {
    locked: 'The Keychain is locked, so saved conversations cannot be read or saved. Nothing was written unencrypted.',
    denied: 'macOS did not allow access to the history key. Nothing was written unencrypted.',
    unavailable: 'Saved conversations are not available on this Mac right now.',
    full: 'This workspace already keeps 16 saved conversations. Delete one to keep new conversations.',
    tooLarge: 'This conversation is too large to save.',
    unreadable: 'This saved conversation cannot be read with this workspace’s key.',
    missing: 'That saved conversation no longer exists.',
}

export const NOT_CONTINUABLE_TEXT: Record<NotContinuable, string> = {
    running: 'This is the conversation running now.',
    otherCredential:
        'It ran with another key, sign-in or endpoint than this workspace uses now, so it can be read and exported but not continued.',
    otherProject:
        'It ran with another project folder, or with changes allowed differently, than the Harness uses now. Choose that folder and access again to continue it, or start a new conversation.',
    otherMcp:
        'It ran with other workspace MCP servers or sign-ins than this workspace uses now, or they could not be checked. Turn the same servers on and sign in to them to continue it, or start a new conversation.',
    engineUnavailable: 'The Harness engine is not bundled with this build.',
    workspaceUnavailable: 'AI Gateway’s workspaces could not be read, so it can be read and exported here but not continued.',
    notRestorable:
        'It was cut short before the engine saved anything to restore, so it can be read and exported but not continued. A call it shows as interrupted may have run.',
    imported: 'It was imported from v1, so it can be read and exported but not continued.',
}

export const HISTORY_FORMATS: readonly [HistoryFormat, string][] = [
    ['markdown', 'Markdown'],
    ['text', 'Plain text'],
    ['json', 'JSON'],
]

export function savedAt(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}
